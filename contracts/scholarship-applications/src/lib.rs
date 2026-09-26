#![no_std]

//! Scholarship/bursary application intake contract.
//!
//! Scope of this pass (issues #1070, #1073, plus the #1074/#1075
//! foundation they build on): publish versioned, immutable form schemas
//! and consent-terms bundles per program, collect affirmative
//! per-version applicant consent, and submit exactly one application per
//! (applicant, program) tied to the accepted form version and a valid
//! (non-revoked, current-version) consent record.
//!
//! On-chain storage is privacy-minimized: it never holds form schemas,
//! consent terms text, or application answers — only caller-supplied
//! integrity commitments (`BytesN<32>` hashes) over that off-chain
//! content, plus the metadata needed to enforce versioning, uniqueness,
//! deadlines, and consent. See
//! `contracts/docs/scholarship-applications.md` for ownership, privacy,
//! migration, and operational notes.
//!
//! Answer validation (#1071) and secure document upload (#1072) are
//! tracked separately and are not implemented here.
//!
//! Withdrawal (#1076) and tamper-evident receipts (#1077) are tracked
//! separately and are not implemented here.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

// -----------------------------------------------------------------------
// Issues #1078/#1079/#1080/#1081 — reviewer pools, deterministic
// assignment, conflict-of-interest, and blind/double-blind review.
//
// Self-contained additions: own error enum and storage keys, kept
// separate from `ContractError`/`DataKey` above (which already has
// duplicate-discriminant issues predating this change) rather than
// risking a collision. Privacy-minimized like the rest of this file:
// applicant/reviewer identities are Addresses and off-chain identifiers
// are only ever handled as `BytesN<32>` commitments, never raw data.
// -----------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ReviewError {
    NotAdmin = 1,
    ReviewerNotFound = 2,
    ReviewerInactive = 3,
    ReviewerAtCapacity = 4,
    ReviewerConflicted = 5,
    NoEligibleReviewer = 6,
    AssignmentNotFound = 7,
    AlreadyAssigned = 8,
    Unauthorized = 9,
}

/// #1078 — a qualified reviewer's pool membership: capacity and current
/// load. Expertise tags and availability windows are tracked off-chain
/// against this Address; only what's needed to enforce capacity and
/// active/inactive status lives on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerProfile {
    pub active: bool,
    pub max_assignments: u32,
    pub current_assignments: u32,
}

/// #1079 — how a reviewer is selected for an application.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignmentMode {
    RoundRobin,
    LoadBalanced,
    /// Seeded-random: `seed` is combined with the application hash so the
    /// choice is reproducible from the same inputs (#1079).
    SeededRandom(u64),
}

/// #1081 — review visibility for a program. `Blind` hides the applicant's
/// identity from the reviewer's on-chain-visible context; `DoubleBlind`
/// additionally hides the reviewer's identity from applicant-facing
/// reads. Enforcement of the *hiding* itself is split between this
/// contract (never storing/returning the hidden identifier from a
/// gated accessor) and the off-chain trust boundary (front-ends must not
/// display it either) — see the acceptance criteria's "explicit
/// off-chain trust boundary" language.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewMode {
    Open,
    Blind,
    DoubleBlind,
}

#[contracttype]
#[derive(Clone)]
pub enum ReviewDataKey {
    /// #1078 — reviewer directory entry.
    Reviewer(Address),
    /// #1078 — ordered pool of reviewer addresses eligible for a program.
    ReviewerPool(BytesN<32>),
    /// #1080 — a reviewer's declared conflict for a program + applicant
    /// commitment. Presence alone means "conflicted"; the value is unused.
    Conflict(Address, BytesN<32>, BytesN<32>),
    /// #1079 — the reviewer assigned to a given (program, application hash).
    Assignment(BytesN<32>, BytesN<32>),
    /// #1079 — round-robin cursor into a program's reviewer pool.
    RoundRobinCursor(BytesN<32>),
    /// #1081 — review visibility mode for a program (defaults to `Open`).
    ReviewMode(BytesN<32>),
}

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ProgramNotFound = 4,
    ProgramAlreadyExists = 5,
    ProgramInactive = 6,
    DeadlinePassed = 7,
    /// #1074 — an application already exists for this (applicant, program) pair.
    DuplicateApplication = 9,
    ApplicationNotFound = 10,
    /// #1070 — no form schema has ever been published for this program.
    NoFormSchemaPublished = 11,
    /// #1070 — the requested form schema version does not exist.
    FormSchemaNotFound = 12,
    /// #1073 — no consent-terms bundle has ever been published for this program.
    NoConsentTermsPublished = 13,
    /// #1073 — the requested consent-terms version does not exist.
    ConsentTermsNotFound = 14,
    /// #1073 — the applicant has never recorded consent for this program.
    ConsentNotFound = 15,
    /// #1073 — the applicant's consent was explicitly revoked.
    ConsentRevoked = 16,
    /// #1073 — the applicant's consent is for an older terms version; the
    /// terms changed since, so re-consent is required before submitting.
    ConsentOutOfDate = 17,
    /// Version counter would overflow u32 — practically unreachable, but
    /// checked rather than silently wrapping.
    VersionOverflow = 18,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Program(BytesN<32>),
    Application(Address, BytesN<32>),
    /// #1070 — latest published form-schema version number for a program.
    FormVersion(BytesN<32>),
    /// #1070 — an immutable, published form-schema version.
    FormSchema(BytesN<32>, u32),
    /// #1073 — latest published consent-terms version number for a program.
    ConsentVersion(BytesN<32>),
    /// #1073 — an immutable, published consent-terms version.
    ConsentTerms(BytesN<32>, u32),
    /// #1073 — an applicant's current consent state for a program.
    Consent(Address, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub active: bool,
    /// Ledger timestamp (seconds) after which submissions are rejected.
    pub deadline: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationStatus {
    Submitted,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application {
    pub applicant: Address,
    pub program_id: BytesN<32>,
    pub status: ApplicationStatus,
    pub submitted_at: u64,
    /// Integrity commitment over the off-chain application content.
    pub data_hash: BytesN<32>,
    /// #1070 — the form schema version the applicant's answers were
    /// validated against off-chain; the answers "remain tied to the
    /// accepted form version" even if a newer version is published later.
    pub form_version: u32,
    /// #1073 — the consent-terms version the applicant accepted.
    pub consent_version: u32,
}

/// #1070 — an immutable, versioned commitment to a program's application
/// form schema (sections/fields/conditional logic all live off-chain;
/// this is just an integrity hash plus the version number and publish time).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormSchema {
    pub version: u32,
    pub schema_hash: BytesN<32>,
    pub published_at: u64,
}

/// #1073 — an immutable, versioned commitment to a program's terms bundle
/// (terms of service, privacy notice, data-sharing and sponsor-disclosure
/// text — all off-chain; this is the hash applicants consent to).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentTerms {
    pub version: u32,
    pub terms_hash: BytesN<32>,
    pub published_at: u64,
}

/// #1073 — an applicant's consent state for one program. Recording new
/// consent (`record_consent`) always targets the *latest* published terms
/// version and replaces any prior record, so accepting a re-published
/// terms bundle is itself the re-consent action.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentRecord {
    pub version: u32,
    pub accepted_at: u64,
    pub revoked: bool,
    pub revoked_at: u64,
}

#[contract]
pub struct ScholarshipApplicationsContract;

#[contractimpl]
impl ScholarshipApplicationsContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    // ── Programs ──────────────────────────────────────────────────────────

    /// Admin-only: register a program that applications can be submitted
    /// against.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        deadline: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        let program = Program {
            active: true,
            deadline,
        };
        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Admin-only: activate/deactivate a program (e.g. to stop intake early).
    pub fn set_program_active(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        let mut program: Program = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        program.active = active;

        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_program(env: Env, program_id: BytesN<32>) -> Result<Program, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Program(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    // ── #1070 — versioned form schemas ───────────────────────────────────

    /// Admin-only: publish a new, immutable form schema version for a
    /// program. Versions are never overwritten — publishing again always
    /// creates version N+1, so any application already tied to an older
    /// version keeps referencing an unchanged schema.
    pub fn publish_form_schema(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        schema_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }

        let version_key = DataKey::FormVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let schema = FormSchema {
            version: next_version,
            schema_hash,
            published_at: env.ledger().timestamp(),
        };

        let schema_key = DataKey::FormSchema(program_id.clone(), next_version);
        env.storage().persistent().set(&schema_key, &schema);
        env.storage()
            .persistent()
            .extend_ttl(&schema_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&version_key, &next_version);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("FORMPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    pub fn get_latest_form_version(env: Env, program_id: BytesN<32>) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FormVersion(program_id))
            .ok_or(ContractError::NoFormSchemaPublished)
    }

    pub fn get_form_schema(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<FormSchema, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FormSchema(program_id, version))
            .ok_or(ContractError::FormSchemaNotFound)
    }

    // ── #1073 — versioned consent terms + applicant consent ─────────────

    /// Admin-only: publish a new, immutable consent-terms version for a
    /// program (terms of service, privacy notice, data-sharing and
    /// sponsor-disclosure text, bundled off-chain behind `terms_hash`).
    pub fn publish_consent_terms(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        terms_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }

        let version_key = DataKey::ConsentVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let terms = ConsentTerms {
            version: next_version,
            terms_hash,
            published_at: env.ledger().timestamp(),
        };

        let terms_key = DataKey::ConsentTerms(program_id.clone(), next_version);
        env.storage().persistent().set(&terms_key, &terms);
        env.storage()
            .persistent()
            .extend_ttl(&terms_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&version_key, &next_version);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("CONSPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    pub fn get_latest_consent_version(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id))
            .ok_or(ContractError::NoConsentTermsPublished)
    }

    pub fn get_consent_terms(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<ConsentTerms, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ConsentTerms(program_id, version))
            .ok_or(ContractError::ConsentTermsNotFound)
    }

    /// #1073 — affirmatively record the applicant's consent to the
    /// *latest* published terms version for this program. Recording
    /// consent again (e.g. after the admin publishes a new terms version)
    /// replaces any prior record — accepting the new bundle is itself the
    /// re-consent action the issue requires.
    pub fn record_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        applicant.require_auth();

        let latest_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
            .ok_or(ContractError::NoConsentTermsPublished)?;

        let record = ConsentRecord {
            version: latest_version,
            accepted_at: env.ledger().timestamp(),
            revoked: false,
            revoked_at: 0,
        };

        let key = DataKey::Consent(applicant.clone(), program_id.clone());
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("CONSENT"),),
            (applicant, program_id, latest_version),
        );

        Ok(latest_version)
    }

    /// #1073 — revoke the applicant's current consent record ("revocable
    /// where allowed"). Revoking does not retroactively invalidate an
    /// application already submitted under that consent — it only blocks
    /// *future* submissions until fresh consent is recorded.
    pub fn revoke_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let key = DataKey::Consent(applicant, program_id);
        let mut record: ConsentRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ConsentNotFound)?;

        record.revoked = true;
        record.revoked_at = env.ledger().timestamp();

        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<ConsentRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Consent(applicant, program_id))
            .ok_or(ContractError::ConsentNotFound)
    }

    /// #1073 — true iff the applicant has a non-revoked consent record at
    /// the program's *current* terms version. False if the terms have
    /// since changed and the applicant hasn't re-consented.
    pub fn has_valid_consent(env: Env, applicant: Address, program_id: BytesN<32>) -> bool {
        let latest_version: u32 = match env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
        {
            Some(v) => v,
            None => return false,
        };
        let record: Option<ConsentRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Consent(applicant, program_id));
        match record {
            Some(r) => !r.revoked && r.version == latest_version,
            None => false,
        }
    }

    // ── #1075/#1074 — atomic, deduplicated submission ────────────────────

    /// #1075 — submit an application atomically: eligibility (program
    /// active), deadline, form-version validity, current-and-valid consent,
    /// and uniqueness (#1074) are all validated before any state is written.
    /// If any check fails, the whole invocation reverts (standard Soroban
    /// semantics) and no partial record — and no submission receipt — is ever
    /// created, so a failed check is safe to retry and a retry after a
    /// transient failure cannot duplicate a successful submission.
    ///
    /// Consent is proven by the applicant's own on-chain `ConsentRecord`,
    /// not by a caller-supplied boolean, so there is no way to submit
    /// without having affirmatively consented.
    pub fn submit_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        data_hash: BytesN<32>,
        form_version: u32,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let program: Program = env
            .storage()
            .persistent()
            .get(&DataKey::Program(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;

        if !program.active {
            return Err(ContractError::ProgramInactive);
        }
        if env.ledger().timestamp() > program.deadline {
            return Err(ContractError::DeadlinePassed);
        }

        // #1070 — answers must be tied to a real, published form version.
        if !env
            .storage()
            .persistent()
            .has(&DataKey::FormSchema(program_id.clone(), form_version))
        {
            return Err(ContractError::FormSchemaNotFound);
        }

        // #1073 — consent must exist, be unrevoked, and match the
        // program's current terms version.
        let consent_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
            .ok_or(ContractError::NoConsentTermsPublished)?;
        let consent: ConsentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Consent(applicant.clone(), program_id.clone()))
            .ok_or(ContractError::ConsentNotFound)?;
        if consent.revoked {
            return Err(ContractError::ConsentRevoked);
        }
        if consent.version != consent_version {
            return Err(ContractError::ConsentOutOfDate);
        }

        // #1074 — the (applicant, program_id) key itself is the uniqueness
        // constraint: a prior successful submission always leaves this key
        // set, so a retried or duplicate submit is rejected here before any
        // write happens, rather than after.
        let application_key = DataKey::Application(applicant.clone(), program_id.clone());
        if env.storage().persistent().has(&application_key) {
            return Err(ContractError::DuplicateApplication);
        }

        let application = Application {
            applicant: applicant.clone(),
            program_id: program_id.clone(),
            status: ApplicationStatus::Submitted,
            submitted_at: env.ledger().timestamp(),
            data_hash,
            form_version,
            consent_version: consent.version,
        };

        env.storage()
            .persistent()
            .set(&application_key, &application);
        env.storage()
            .persistent()
            .extend_ttl(&application_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("SUBMIT"),),
            (applicant, program_id),
        );

        Ok(())
    }

    pub fn get_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<Application, ContractError> {
        let key = DataKey::Application(applicant, program_id);
        let application = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ApplicationNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(application)
    }

    /// #1074 — cheap existence check for callers that only need to know
    /// whether a duplicate would be rejected, without fetching the record.
    pub fn has_applied(env: Env, applicant: Address, program_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Application(applicant, program_id))
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    // -------------------------------------------------------------------
    // #1078 — reviewer pools and workload limits
    // -------------------------------------------------------------------

    /// Admin-only: register (or update) a reviewer's pool membership and
    /// capacity. `max_assignments` bounds concurrent load; a reviewer with
    /// `current_assignments >= max_assignments` is skipped by
    /// [`assign_reviewer`].
    pub fn register_reviewer(
        env: Env,
        admin: Address,
        reviewer: Address,
        max_assignments: u32,
    ) -> Result<(), ReviewError> {
        Self::require_review_admin(&env, &admin)?;
        let current_assignments = env
            .storage()
            .persistent()
            .get::<_, ReviewerProfile>(&ReviewDataKey::Reviewer(reviewer.clone()))
            .map(|p| p.current_assignments)
            .unwrap_or(0);
        env.storage().persistent().set(
            &ReviewDataKey::Reviewer(reviewer),
            &ReviewerProfile {
                active: true,
                max_assignments,
                current_assignments,
            },
        );
        Ok(())
    }

    /// Admin-only: activate or deactivate a reviewer. An inactive reviewer
    /// is never selected by [`assign_reviewer`], regardless of capacity.
    pub fn set_reviewer_active(
        env: Env,
        admin: Address,
        reviewer: Address,
        active: bool,
    ) -> Result<(), ReviewError> {
        Self::require_review_admin(&env, &admin)?;
        let key = ReviewDataKey::Reviewer(reviewer);
        let mut profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReviewError::ReviewerNotFound)?;
        profile.active = active;
        env.storage().persistent().set(&key, &profile);
        Ok(())
    }

    /// Admin-only: set the ordered pool of reviewers eligible for a
    /// program. Replaces any existing pool for this program.
    pub fn set_reviewer_pool(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        reviewers: Vec<Address>,
    ) -> Result<(), ReviewError> {
        Self::require_review_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&ReviewDataKey::ReviewerPool(program_id), &reviewers);
        Ok(())
    }

    // -------------------------------------------------------------------
    // #1080 — reviewer conflict-of-interest
    // -------------------------------------------------------------------

    /// A reviewer declares a conflict against a specific (program,
    /// applicant-commitment) pair. Self-authorized: only the reviewer
    /// themselves can declare their own conflict. Once declared, the
    /// reviewer is excluded from assignment for that pair and, if already
    /// assigned, any existing assignment must be re-run by the admin
    /// (declaring a conflict does not itself unassign — see acceptance
    /// criteria's "overrides require documented approval").
    pub fn declare_conflict(
        env: Env,
        reviewer: Address,
        program_id: BytesN<32>,
        applicant_commitment: BytesN<32>,
    ) -> Result<(), ReviewError> {
        reviewer.require_auth();
        env.storage().persistent().set(
            &ReviewDataKey::Conflict(reviewer, program_id, applicant_commitment),
            &true,
        );
        Ok(())
    }

    /// Whether `reviewer` has declared a conflict for this (program,
    /// applicant-commitment) pair.
    pub fn has_conflict(
        env: Env,
        reviewer: Address,
        program_id: BytesN<32>,
        applicant_commitment: BytesN<32>,
    ) -> bool {
        env.storage()
            .persistent()
            .has(&ReviewDataKey::Conflict(reviewer, program_id, applicant_commitment))
    }

    // -------------------------------------------------------------------
    // #1079 — deterministic reviewer assignment
    // -------------------------------------------------------------------

    /// Admin-only: assign a reviewer to `application_commitment` under
    /// `program_id`, respecting active status, capacity, and declared
    /// conflicts. Deterministic for a given pool/cursor/mode state, so the
    /// same inputs always reproduce the same assignment; every successful
    /// assignment increments the chosen reviewer's load.
    ///
    /// # Errors
    /// `AlreadyAssigned` if this (program, application) pair already has
    /// an assignment; `NoEligibleReviewer` if every pool member is
    /// inactive, at capacity, or conflicted.
    pub fn assign_reviewer(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        application_commitment: BytesN<32>,
        mode: AssignmentMode,
    ) -> Result<Address, ReviewError> {
        Self::require_review_admin(&env, &admin)?;

        let assignment_key = ReviewDataKey::Assignment(program_id.clone(), application_commitment.clone());
        if env.storage().persistent().has(&assignment_key) {
            return Err(ReviewError::AlreadyAssigned);
        }

        let pool: Vec<Address> = env
            .storage()
            .persistent()
            .get(&ReviewDataKey::ReviewerPool(program_id.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        if pool.is_empty() {
            return Err(ReviewError::NoEligibleReviewer);
        }

        let is_eligible = |env: &Env, reviewer: &Address| -> bool {
            let profile: Option<ReviewerProfile> =
                env.storage().persistent().get(&ReviewDataKey::Reviewer(reviewer.clone()));
            let profile = match profile {
                Some(p) => p,
                None => return false,
            };
            if !profile.active || profile.current_assignments >= profile.max_assignments {
                return false;
            }
            !env.storage().persistent().has(&ReviewDataKey::Conflict(
                reviewer.clone(),
                program_id.clone(),
                application_commitment.clone(),
            ))
        };

        let n = pool.len();
        let start_offset: u64 = match mode {
            AssignmentMode::RoundRobin | AssignmentMode::LoadBalanced => env
                .storage()
                .persistent()
                .get(&ReviewDataKey::RoundRobinCursor(program_id.clone()))
                .unwrap_or(0u64),
            AssignmentMode::SeededRandom(seed) => {
                // Fold the seed and the application commitment's bytes into
                // a single deterministic offset (#1079).
                let mut acc = seed;
                for byte in application_commitment.to_array() {
                    acc = acc.wrapping_mul(1_000_003).wrapping_add(byte as u64);
                }
                acc
            }
        };

        let mut chosen: Option<Address> = None;
        for step in 0..n {
            let idx = ((start_offset + step as u64) % n as u64) as u32;
            let candidate = pool.get(idx).unwrap();
            if is_eligible(&env, &candidate) {
                chosen = Some(candidate);
                break;
            }
        }
        let chosen = chosen.ok_or(ReviewError::NoEligibleReviewer)?;

        let mut profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&ReviewDataKey::Reviewer(chosen.clone()))
            .ok_or(ReviewError::ReviewerNotFound)?;
        profile.current_assignments += 1;
        env.storage()
            .persistent()
            .set(&ReviewDataKey::Reviewer(chosen.clone()), &profile);

        env.storage()
            .persistent()
            .set(&assignment_key, &chosen);

        if matches!(mode, AssignmentMode::RoundRobin | AssignmentMode::LoadBalanced) {
            let next_cursor = (start_offset + 1) % (n as u64).max(1);
            env.storage()
                .persistent()
                .set(&ReviewDataKey::RoundRobinCursor(program_id), &next_cursor);
        }

        Ok(chosen)
    }

    /// Returns the reviewer assigned to `application_commitment` under
    /// `program_id`, if any. Callers are responsible for enforcing the
    /// program's [`ReviewMode`] (e.g. a double-blind program's front-end
    /// must not surface this to the applicant) — this accessor itself
    /// makes no distinction, per this file's privacy-minimized,
    /// explicit-off-chain-trust-boundary convention.
    pub fn get_assignment(
        env: Env,
        program_id: BytesN<32>,
        application_commitment: BytesN<32>,
    ) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&ReviewDataKey::Assignment(program_id, application_commitment))
    }

    // -------------------------------------------------------------------
    // #1081 — blind / double-blind review mode
    // -------------------------------------------------------------------

    /// Admin-only: set the review visibility mode for a program.
    pub fn set_review_mode(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        mode: ReviewMode,
    ) -> Result<(), ReviewError> {
        Self::require_review_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&ReviewDataKey::ReviewMode(program_id), &mode);
        Ok(())
    }

    /// Returns a program's review visibility mode, defaulting to `Open`
    /// when never explicitly set.
    pub fn get_review_mode(env: Env, program_id: BytesN<32>) -> ReviewMode {
        env.storage()
            .persistent()
            .get(&ReviewDataKey::ReviewMode(program_id))
            .unwrap_or(ReviewMode::Open)
    }

    fn require_review_admin(env: &Env, caller: &Address) -> Result<(), ReviewError> {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ReviewError::NotAdmin)?;
        if admin != *caller {
            return Err(ReviewError::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
