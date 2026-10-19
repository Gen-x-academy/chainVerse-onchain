#![no_std]

//! Participant agreement record export — issue #1248 (**MENTOR-052**,
//! Agreements track).
//!
//! ChainVerse advertises 1-on-1 mentorship with smart-contract-backed
//! payments. When a mentorship engagement ends — or a participant simply
//! wants their record — both sides need a portable, auditable copy of the
//! agreement: the accepted terms, its amendments, the approvals, a session
//! summary and a financial summary. This contract is the privacy-minimized
//! on-chain primitive for that export.
//!
//! Scope of this pass (MENTOR-052):
//!
//! - **Only authorized participants can export.** The export flow is gated
//!   on the requesting address being the mentor or mentee of the agreement
//!   (`require_auth`); grants are personal, so someone else's grant cannot
//!   be downloaded (`NotAuthorized`).
//! - **Generation is rate-limited and audited.** Each participant gets at
//!   most `max_exports_per_window` grants per rolling window; every
//!   request and every download appends to a bounded on-chain audit log
//!   and emits an event.
//! - **Expired links cannot download.** A grant carries `expires_at`;
//!   `get_export` after that instant fails with `GrantExpired`. Expired
//!   grants are terminal — no renewal in place, a fresh grant must be
//!   requested (and re-counted against the quota).
//! - **Private internal notes are excluded.** The exportable record is
//!   commitments, statuses, timestamps, and amounts by construction: no
//!   entry point accepts free text and `ExportManifest` has no free-text
//!   field, so there is nothing for a private note to leak into. Notes
//!   (MENTOR-064) and messages (MENTOR-101) live off-chain, and the
//!   on-chain record only ever commits to their hashes.
//! - **Safe retries.** Re-requesting with the same grant id and scope
//!   returns the same grant without consuming quota twice or duplicating
//!   the audit entry; reusing a grant id for a different scope fails with
//!   a stable `GrantAlreadyExists`.
//! - **Bounded storage.** Amendments, approvals, session summaries, and
//!   audit entries are all capped; every persistent entry is TTL-bumped.
//!
//! Agreement state is anchored here by a registrar address (admin-set,
//! deny-by-default) so the canonical agreements contracts (MENTOR-045,
//! MENTOR-046, MENTOR-047) can take over the role when they land; the
//! export surface stays unchanged. See
//! `contracts/docs/mentorship-agreement-export.md` for the storage
//! layout, event schema, off-chain boundary, and rollout notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling
// contract convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1248 — bounded storage per agreement record.
const MAX_AMENDMENTS: u32 = 32;
const MAX_APPROVALS: u32 = 32;
const MAX_SESSION_SUMMARIES: u32 = 64;
const MAX_AUDIT_ENTRIES: u32 = 96;

/// #1248 — bounds on the export policy so it cannot be misconfigured into
/// never-limiting or never-expiring grants.
const MIN_WINDOW_SECONDS: u32 = 60;
const MAX_WINDOW_SECONDS: u32 = 2_592_000; // 30 days
const MIN_GRANT_TTL_SECONDS: u32 = 60;
const MAX_GRANT_TTL_SECONDS: u32 = 604_800; // 7 days

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// Only the registrar (admin-set, deny-by-default) may project
    /// agreement facts into this contract.
    NotRegistrar = 4,
    /// The caller is not the mentor or mentee of the agreement, or not the
    /// requester of the grant.
    NotAuthorized = 5,
    AgreementNotFound = 6,
    AgreementAlreadyRegistered = 7,
    /// Amendment versions must be strictly increasing.
    AmendmentVersionNotIncreasing = 8,
    /// The agreement already holds MAX_AMENDMENTS amendments.
    AmendmentLimitReached = 9,
    /// The same approver already approved the same terms version.
    DuplicateApproval = 10,
    /// The agreement already holds MAX_SESSION_SUMMARIES summaries.
    SessionSummaryLimitReached = 11,
    /// Amounts must be non-negative and fees cannot exceed gross.
    InvalidAmount = 12,
    /// The rolling export quota for this participant is exhausted.
    ExportRateLimited = 13,
    GrantNotFound = 14,
    /// The grant id is taken by a different agreement or requester.
    GrantAlreadyExists = 15,
    /// The grant expired; request a fresh one.
    GrantExpired = 16,
    /// The export policy value is outside its allowed bounds.
    InvalidPolicy = 17,
    /// The agreement already holds MAX_AUDIT_ENTRIES audit entries.
    AuditLimitReached = 18,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Registrar,
    /// Export policy, stored as one record.
    Policy,
    /// One anchored agreement record.
    Agreement(BytesN<32>),
    /// One export grant ("link").
    Grant(BytesN<32>),
    /// Per-participant rolling quota window.
    ExportQuota(Address),
    /// Bounded audit log per agreement.
    Audit(BytesN<32>),
}

/// #1248 — the export policy. `max_exports_per_window` caps grant
/// generation, `window_seconds` rolls the quota, `grant_ttl_seconds`
/// bounds how long a grant can be downloaded.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportPolicy {
    pub max_exports_per_window: u32,
    pub window_seconds: u32,
    pub grant_ttl_seconds: u32,
}

/// #1248 — one recorded amendment of the accepted terms. Only commitments
/// and version metadata; the amended text stays off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Amendment {
    pub version: u32,
    pub terms_hash: BytesN<32>,
    pub reason_hash: BytesN<32>,
    pub amended_at: u64,
}

/// #1248 — one participant's approval of a terms version (MENTOR-046's
/// acceptance, projected here). `approval_hash` commits to the signed
/// acceptance artifact kept off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Approval {
    pub approver: Address,
    pub terms_version: u32,
    pub approval_hash: BytesN<32>,
    pub approved_at: u64,
}

/// #1248 — a per-session summary line. `summary_hash` commits to the
/// off-chain summary text (MENTOR-065); only timing facts live here.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSummary {
    pub session_id: BytesN<32>,
    pub summary_hash: BytesN<32>,
    pub held_at: u64,
    pub duration_minutes: u32,
}

/// #1248 — the financial summary: settlement facts, accumulated from
/// recorded session summaries. Amounts are integers in the smallest unit;
/// this contract never moves funds.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancialSummary {
    pub currency: Symbol,
    pub gross: i128,
    pub fees: i128,
    pub net: i128,
}

/// #1248 — the anchored agreement record. Everything here is a commitment,
/// a status, a timestamp, or an amount — never private content.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementRecord {
    pub agreement_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    /// Commitment over the accepted terms text.
    pub terms_hash: BytesN<32>,
    pub terms_version: u32,
    pub accepted_at: u64,
    pub amendments: Vec<Amendment>,
    pub approvals: Vec<Approval>,
    pub session_summaries: Vec<SessionSummary>,
    pub financial: FinancialSummary,
    pub registered_at: u64,
}

/// #1248 — the exportable bundle. Note what is *not* here: no free-text
/// field of any kind, so private internal notes are excluded by
/// construction rather than by filtering.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportManifest {
    pub agreement_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub terms_hash: BytesN<32>,
    pub terms_version: u32,
    pub accepted_at: u64,
    pub amendments: Vec<Amendment>,
    pub approvals: Vec<Approval>,
    pub session_summaries: Vec<SessionSummary>,
    pub financial: FinancialSummary,
    pub generated_at: u64,
    pub generated_for: Address,
}

/// #1248 — a personal, expiring export grant ("link").
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportGrant {
    pub grant_id: BytesN<32>,
    pub agreement_id: BytesN<32>,
    pub requester: Address,
    pub created_at: u64,
    pub expires_at: u64,
}

/// #1248 — rolling quota state for one participant.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportQuota {
    pub window_started_at: u64,
    pub used: u32,
}

/// #1248 — what happened in the export lifecycle, recorded on-chain.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditKind {
    Requested,
    Downloaded,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub at: u64,
    pub actor: Address,
    pub grant_id: BytesN<32>,
    pub kind: AuditKind,
}

#[contract]
pub struct MentorshipAgreementExportContract;

#[contractimpl]
impl MentorshipAgreementExportContract {
    /// Initialize the contract admin and export policy. Run once.
    pub fn initialize(
        env: Env,
        admin: Address,
        max_exports_per_window: u32,
        window_seconds: u32,
        grant_ttl_seconds: u32,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        let policy = ExportPolicy {
            max_exports_per_window,
            window_seconds,
            grant_ttl_seconds,
        };
        Self::validate_policy(&policy)?;
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Registrar, &admin);
        env.storage().instance().set(&DataKey::Policy, &policy);
        Ok(())
    }

    pub fn admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn registrar(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Registrar)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn policy(env: Env) -> Result<ExportPolicy, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Policy)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    /// Admin-only: move the registrar role to the canonical agreements
    /// contract when it ships (deny-by-default until then).
    pub fn set_registrar(
        env: Env,
        admin: Address,
        registrar: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Registrar, &registrar);
        env.events()
            .publish((symbol_short!("REGISTRAR"),), (registrar,));
        Ok(())
    }

    /// Admin-only: update the export policy.
    pub fn set_policy(
        env: Env,
        admin: Address,
        max_exports_per_window: u32,
        window_seconds: u32,
        grant_ttl_seconds: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let policy = ExportPolicy {
            max_exports_per_window,
            window_seconds,
            grant_ttl_seconds,
        };
        Self::validate_policy(&policy)?;
        env.storage().instance().set(&DataKey::Policy, &policy);
        env.events().publish(
            (symbol_short!("EXPOLICY"),),
            (max_exports_per_window, window_seconds, grant_ttl_seconds),
        );
        Ok(())
    }

    // ── #1248 — agreement projection (registrar-gated) ──────────────────

    /// Registrar-only: anchor an agreement so its participants can export
    /// it. The accepted terms are referenced by commitment only.
    pub fn register_agreement(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
        terms_hash: BytesN<32>,
        terms_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Agreement(agreement_id.clone()))
        {
            return Err(ContractError::AgreementAlreadyRegistered);
        }
        if mentor == mentee {
            return Err(ContractError::NotAuthorized);
        }
        let record = AgreementRecord {
            agreement_id: agreement_id.clone(),
            mentor,
            mentee,
            terms_hash,
            terms_version,
            accepted_at: env.ledger().timestamp(),
            amendments: Vec::new(&env),
            approvals: Vec::new(&env),
            session_summaries: Vec::new(&env),
            financial: FinancialSummary {
                currency: Symbol::new(&env, "XLM"),
                gross: 0,
                fees: 0,
                net: 0,
            },
            registered_at: env.ledger().timestamp(),
        };
        Self::put_agreement(&env, &record);
        env.events()
            .publish((symbol_short!("AGREG"),), (agreement_id,));
        Ok(())
    }

    /// Participant-only: record this participant's approval of the current
    /// terms version. Replay-protected: one approval per
    /// (agreement, approver, terms_version).
    pub fn record_approval(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        approval_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut record = Self::load_agreement(&env, &agreement_id)?;
        if caller != record.mentor && caller != record.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        let version = record.terms_version;
        for i in 0..record.approvals.len() {
            let approval = record
                .approvals
                .get(i)
                .ok_or(ContractError::DuplicateApproval)?;
            if approval.approver == caller && approval.terms_version == version {
                return Err(ContractError::DuplicateApproval);
            }
        }
        if record.approvals.len() >= MAX_APPROVALS {
            return Err(ContractError::DuplicateApproval);
        }
        record.approvals.push_back(Approval {
            approver: caller.clone(),
            terms_version: version,
            approval_hash,
            approved_at: env.ledger().timestamp(),
        });
        Self::put_agreement(&env, &record);
        env.events().publish(
            (symbol_short!("APPROVED"),),
            (agreement_id, caller, version),
        );
        Ok(())
    }

    /// Registrar-only: project an amendment (MENTOR-047) into the record.
    /// Versions must be strictly increasing; the agreement's current terms
    /// pointer moves to the amendment.
    pub fn record_amendment(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        new_version: u32,
        terms_hash: BytesN<32>,
        reason_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        let mut record = Self::load_agreement(&env, &agreement_id)?;
        if new_version <= record.terms_version {
            return Err(ContractError::AmendmentVersionNotIncreasing);
        }
        if record.amendments.len() >= MAX_AMENDMENTS {
            return Err(ContractError::AmendmentLimitReached);
        }
        record.amendments.push_back(Amendment {
            version: new_version,
            terms_hash: terms_hash.clone(),
            reason_hash,
            amended_at: env.ledger().timestamp(),
        });
        record.terms_hash = terms_hash;
        record.terms_version = new_version;
        Self::put_agreement(&env, &record);
        env.events()
            .publish((symbol_short!("AMENDED"),), (agreement_id, new_version));
        Ok(())
    }

    /// Registrar-only: project a completed session's summary (MENTOR-065)
    /// and its financial line into the record. Bounded, and the running
    /// financial summary is checked arithmetic.
    pub fn record_session_summary(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        session_id: BytesN<32>,
        summary_hash: BytesN<32>,
        held_at: u64,
        duration_minutes: u32,
        gross: i128,
        fees: i128,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        if gross < 0 || fees < 0 || fees > gross {
            return Err(ContractError::InvalidAmount);
        }
        let mut record = Self::load_agreement(&env, &agreement_id)?;
        if record.session_summaries.len() >= MAX_SESSION_SUMMARIES {
            return Err(ContractError::SessionSummaryLimitReached);
        }
        record.session_summaries.push_back(SessionSummary {
            session_id: session_id.clone(),
            summary_hash,
            held_at,
            duration_minutes,
        });
        record.financial.gross = record
            .financial
            .gross
            .checked_add(gross)
            .ok_or(ContractError::InvalidAmount)?;
        record.financial.fees = record
            .financial
            .fees
            .checked_add(fees)
            .ok_or(ContractError::InvalidAmount)?;
        record.financial.net = record
            .financial
            .net
            .checked_add(
                gross
                    .checked_sub(fees)
                    .ok_or(ContractError::InvalidAmount)?,
            )
            .ok_or(ContractError::InvalidAmount)?;
        Self::put_agreement(&env, &record);
        env.events()
            .publish((symbol_short!("SUMMARY"),), (agreement_id, session_id));
        Ok(())
    }

    // ── #1248 — export flow ─────────────────────────────────────────────

    /// Participant-only: request an export grant for an agreement. The
    /// rolling quota is enforced here; every grant is audited and emits
    /// one `EXPREQ` event. Retrying with the same grant id and scope
    /// returns the same grant without consuming quota twice.
    pub fn request_export(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        grant_id: BytesN<32>,
    ) -> Result<ExportGrant, ContractError> {
        let record = Self::load_agreement(&env, &agreement_id)?;
        if caller != record.mentor && caller != record.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        let now = env.ledger().timestamp();

        // Idempotent replay: the same grant id under the same scope is the
        // same link — no new quota consumption, no duplicate audit entry.
        let grant_key = DataKey::Grant(grant_id.clone());
        if let Some(existing) = env.storage().persistent().get::<_, ExportGrant>(&grant_key) {
            env.storage()
                .persistent()
                .extend_ttl(&grant_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            if existing.agreement_id == agreement_id && existing.requester == caller {
                return Ok(existing);
            }
            return Err(ContractError::GrantAlreadyExists);
        }

        let policy: ExportPolicy = env
            .storage()
            .instance()
            .get(&DataKey::Policy)
            .ok_or(ContractError::NotInitialized)?;

        // Rolling quota: reset when the window has elapsed.
        let quota_key = DataKey::ExportQuota(caller.clone());
        let mut quota: ExportQuota =
            env.storage()
                .persistent()
                .get(&quota_key)
                .unwrap_or(ExportQuota {
                    window_started_at: now,
                    used: 0,
                });
        let window_elapsed = now
            .checked_sub(quota.window_started_at)
            .map(|age| age >= u64::from(policy.window_seconds))
            .unwrap_or(true);
        if window_elapsed {
            quota = ExportQuota {
                window_started_at: now,
                used: 0,
            };
        }
        if quota.used >= policy.max_exports_per_window {
            return Err(ContractError::ExportRateLimited);
        }
        quota.used = quota
            .used
            .checked_add(1)
            .ok_or(ContractError::ExportRateLimited)?;
        env.storage().persistent().set(&quota_key, &quota);
        env.storage()
            .persistent()
            .extend_ttl(&quota_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let expires_at = now
            .checked_add(u64::from(policy.grant_ttl_seconds))
            .ok_or(ContractError::GrantExpired)?;
        let grant = ExportGrant {
            grant_id: grant_id.clone(),
            agreement_id: agreement_id.clone(),
            requester: caller.clone(),
            created_at: now,
            expires_at,
        };
        env.storage().persistent().set(&grant_key, &grant);
        env.storage()
            .persistent()
            .extend_ttl(&grant_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        Self::push_audit(
            &env,
            &agreement_id,
            &AuditEntry {
                at: now,
                actor: caller.clone(),
                grant_id: grant_id.clone(),
                kind: AuditKind::Requested,
            },
        )?;
        env.events().publish(
            (symbol_short!("EXPREQ"), grant_id),
            (agreement_id, caller, expires_at),
        );
        Ok(grant)
    }

    /// Download the export bundle for a grant. Only the grant's requester
    /// may download, and only before `expires_at` — an expired link fails
    /// with `GrantExpired` rather than returning stale data. Every
    /// download is audited and emits one `EXPDL` event.
    pub fn get_export(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
    ) -> Result<ExportManifest, ContractError> {
        let grant: ExportGrant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id.clone()))
            .ok_or(ContractError::GrantNotFound)?;
        if caller != grant.requester {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        let now = env.ledger().timestamp();
        if now > grant.expires_at {
            return Err(ContractError::GrantExpired);
        }

        let record = Self::load_agreement(&env, &grant.agreement_id)?;
        Self::push_audit(
            &env,
            &grant.agreement_id,
            &AuditEntry {
                at: now,
                actor: caller.clone(),
                grant_id: grant_id.clone(),
                kind: AuditKind::Downloaded,
            },
        )?;
        env.events().publish(
            (symbol_short!("EXPDL"), grant_id),
            (grant.agreement_id.clone(), caller.clone(), now),
        );

        Ok(ExportManifest {
            agreement_id: record.agreement_id,
            mentor: record.mentor,
            mentee: record.mentee,
            terms_hash: record.terms_hash,
            terms_version: record.terms_version,
            accepted_at: record.accepted_at,
            amendments: record.amendments,
            approvals: record.approvals,
            session_summaries: record.session_summaries,
            financial: record.financial,
            generated_at: now,
            generated_for: caller,
        })
    }

    // ── views ───────────────────────────────────────────────────────────

    pub fn get_agreement(
        env: Env,
        agreement_id: BytesN<32>,
    ) -> Result<AgreementRecord, ContractError> {
        Self::load_agreement(&env, &agreement_id)
    }

    /// The grant's metadata, readable only by its requester (deny-by-default
    /// reads: the manifest itself is never readable through this view).
    pub fn get_grant(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
    ) -> Result<ExportGrant, ContractError> {
        let grant: ExportGrant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(ContractError::GrantNotFound)?;
        if caller != grant.requester {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        Ok(grant)
    }

    /// The bounded audit log for an agreement, readable only by its
    /// participants.
    pub fn get_audit(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
    ) -> Result<Vec<AuditEntry>, ContractError> {
        let record = Self::load_agreement(&env, &agreement_id)?;
        if caller != record.mentor && caller != record.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        let key = DataKey::Audit(agreement_id);
        let entries: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        Ok(entries)
    }

    /// Admin-only: upgrade the contract to `new_wasm_hash`. The export
    /// schema is versioned by `version()` for client compatibility.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("UPGRADED"),), (new_wasm_hash,));
        Ok(())
    }

    // ── internals ───────────────────────────────────────────────────────

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

    fn require_registrar(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let registrar: Address = env
            .storage()
            .instance()
            .get(&DataKey::Registrar)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != registrar {
            return Err(ContractError::NotRegistrar);
        }
        caller.require_auth();
        Ok(())
    }

    fn validate_policy(policy: &ExportPolicy) -> Result<(), ContractError> {
        if policy.max_exports_per_window == 0
            || policy.window_seconds < MIN_WINDOW_SECONDS
            || policy.window_seconds > MAX_WINDOW_SECONDS
            || policy.grant_ttl_seconds < MIN_GRANT_TTL_SECONDS
            || policy.grant_ttl_seconds > MAX_GRANT_TTL_SECONDS
        {
            return Err(ContractError::InvalidPolicy);
        }
        Ok(())
    }

    fn load_agreement(
        env: &Env,
        agreement_id: &BytesN<32>,
    ) -> Result<AgreementRecord, ContractError> {
        let key = DataKey::Agreement(agreement_id.clone());
        let record: AgreementRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AgreementNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    fn put_agreement(env: &Env, record: &AgreementRecord) {
        let key = DataKey::Agreement(record.agreement_id.clone());
        env.storage().persistent().set(&key, record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    /// Append to the bounded per-agreement audit log. The log is capped so
    /// a long-lived agreement cannot grow it without bound; once full,
    /// further export activity is refused loudly rather than silently
    /// dropping audit history.
    fn push_audit(
        env: &Env,
        agreement_id: &BytesN<32>,
        entry: &AuditEntry,
    ) -> Result<(), ContractError> {
        let key = DataKey::Audit(agreement_id.clone());
        let mut entries: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if entries.len() >= MAX_AUDIT_ENTRIES {
            return Err(ContractError::AuditLimitReached);
        }
        entries.push_back(entry.clone());
        env.storage().persistent().set(&key, &entries);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
