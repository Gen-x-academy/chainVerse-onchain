#![no_std]

//! Scholarship/bursary milestone evidence contract.
//!
//! Scope of this pass (issues #1094, #1095): let a recipient (or a
//! trusted system on their behalf) submit *privacy-minimized* evidence
//! against an active milestone, and let an authorized verifier approve,
//! reject, or request changes on that evidence with an explicit reason
//! code — without either side ever putting the underlying evidence
//! on-chain.
//!
//! - **#1094 — Submit milestone evidence.** `submit_evidence()` records a
//!   caller-supplied `data_hash: BytesN<32>` — an integrity commitment over
//!   off-chain (and, where needed, encrypted) evidence — against an active
//!   milestone, keyed by `(program_id, milestone_id, recipient)`. Evidence
//!   is *versioned*: a new submission for the same tuple creates version
//!   N+1, and re-submitting the exact same `data_hash` is idempotent and
//!   returns the existing version rather than creating a duplicate. Only
//!   the recipient themselves, or an admin-allowlisted trusted submitter,
//!   may submit for a recipient.
//! - **#1095 — Verify milestone evidence.** `verify_evidence()` lets an
//!   admin-allowlisted verifier decide a *specific evidence version*
//!   (`Approved` / `Rejected` / `ChangesRequested`) with an explicit
//!   `ReasonCode`. A verifier who is also the evidence's recipient or
//!   submitter is rejected as a conflict of interest. Each evidence
//!   version is decidable at most once, so an approval emits **at most one**
//!   payment-eligibility event (`EVPASS`) for that version — re-deciding is
//!   impossible and a changed decision requires a new evidence version.
//!   Every decision is recorded on the version and published as `EVDECID`.
//!
//! Privacy-minimized by design: on-chain state only ever contains stable
//! identifiers (addresses, IDs), a `BytesN<32>` commitment, status/reason
//! enums, and timestamps — never grades, documents, or any other evidence
//! content. See `contracts/docs/scholarship-milestones.md` for ownership,
//! privacy, migration, and operational notes.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling scholarship contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// #1094 — the caller is neither the recipient nor an authorized trusted submitter.
    NotAuthorizedSubmitter = 4,
    MilestoneNotFound = 5,
    /// #1094 — evidence may only be submitted against an `Active` milestone.
    MilestoneNotActive = 6,
    /// #1094 — a milestone with this `(program_id, milestone_id)` already exists.
    MilestoneAlreadyExists = 7,
    /// #1094 — the per-recipient evidence version counter would overflow `u32`.
    VersionOverflow = 8,
    EvidenceNotFound = 9,
    /// #1095 — the caller is not on the verifier allowlist.
    NotAuthorizedVerifier = 10,
    /// #1095 — a verifier may not decide evidence they submitted or that is theirs.
    VerifierConflict = 11,
    /// #1095 — this evidence version has already been decided; decisions are terminal.
    AlreadyDecided = 12,
    /// #1095 — the reason code is not coherent with the requested decision.
    InvalidReasonCode = 13,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1094 — a milestone for `(program_id, milestone_id)`.
    Milestone(BytesN<32>, u64),
    /// #1094 — address allowlisted to submit evidence on recipients' behalf.
    Submitter(Address),
    /// #1095 — address allowlisted to decide evidence.
    Verifier(Address),
    /// #1094 — latest evidence version for `(program_id, milestone_id, recipient)`.
    EvidenceVersion(BytesN<32>, u64, Address),
    /// #1094 — an immutable evidence version.
    Evidence(BytesN<32>, u64, Address, u32),
}

/// #1094 — a milestone can only accept evidence while `Active`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneStatus {
    Active,
    Closed,
}

/// #1095 — the lifecycle of a single evidence version. `Pending` is the
/// only non-terminal state; a decided version is never decided again.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceStatus {
    Pending,
    Approved,
    Rejected,
    ChangesRequested,
}

/// #1095 — what a verifier decided. Deliberately separate from
/// [`EvidenceStatus`] so a caller can never pass `Pending` as a decision.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationDecision {
    Approved,
    Rejected,
    ChangesRequested,
}

/// #1095 — explicit, closed reason codes. A bounded enum rather than free
/// text, so a verifier cannot accidentally publish sensitive detail
/// on-chain through a "reason" string; the real rationale stays off-chain.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    NotReviewed,
    MeetsCriteria,
    InsufficientEvidence,
    EvidenceMismatch,
    OutsideScope,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub program_id: BytesN<32>,
    pub milestone_id: u64,
    pub status: MilestoneStatus,
    pub created_by: Address,
    pub created_at: u64,
}

/// #1094/#1095 — one immutable version of a recipient's evidence for a
/// milestone, plus the (at most one) decision recorded against it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    pub program_id: BytesN<32>,
    pub milestone_id: u64,
    pub recipient: Address,
    /// Monotonic, starts at 1 — never mutated after creation.
    pub version: u32,
    /// Integrity commitment over off-chain evidence; never the evidence itself.
    pub data_hash: BytesN<32>,
    pub submitted_by: Address,
    pub submitted_at: u64,
    pub status: EvidenceStatus,
    /// False until a verifier decides this version.
    pub decided: bool,
    /// The submitter until decided, then the deciding verifier.
    pub decided_by: Address,
    pub decided_at: u64,
    pub reason_code: ReasonCode,
}

#[contract]
pub struct ScholarshipMilestonesContract;

#[contractimpl]
impl ScholarshipMilestonesContract {
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

    fn load_milestone(
        env: &Env,
        program_id: &BytesN<32>,
        milestone_id: u64,
    ) -> Result<Milestone, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Milestone(program_id.clone(), milestone_id))
            .ok_or(ContractError::MilestoneNotFound)
    }

    // ── #1094 — milestones ────────────────────────────────────────────────

    /// Admin-only: create a milestone for a program. The `(program_id,
    /// milestone_id)` pair is a stable identifier and cannot be re-created.
    pub fn create_milestone(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        milestone_id: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Milestone(program_id.clone(), milestone_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::MilestoneAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let milestone = Milestone {
            program_id: program_id.clone(),
            milestone_id,
            status: MilestoneStatus::Active,
            created_by: admin.clone(),
            created_at: now,
        };

        env.storage().persistent().set(&key, &milestone);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("MSTONE"),), (program_id, milestone_id));
        Ok(())
    }

    /// Admin-only: open or close a milestone. Evidence may only be
    /// submitted while a milestone is `Active`; closing one never removes
    /// or alters evidence already recorded against it.
    pub fn set_milestone_status(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        milestone_id: u64,
        status: MilestoneStatus,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Milestone(program_id.clone(), milestone_id);
        let mut milestone: Milestone = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::MilestoneNotFound)?;

        milestone.status = status;
        env.storage().persistent().set(&key, &milestone);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("MSTAT"),), (program_id, milestone_id, status));
        Ok(())
    }

    pub fn get_milestone(
        env: Env,
        program_id: BytesN<32>,
        milestone_id: u64,
    ) -> Result<Milestone, ContractError> {
        Self::load_milestone(&env, &program_id, milestone_id)
    }

    // ── submitter / verifier allowlists ───────────────────────────────────

    /// Admin-only: authorize `submitter` to submit evidence on a
    /// recipient's behalf (a "trusted system").
    pub fn add_submitter(
        env: Env,
        admin: Address,
        submitter: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Submitter(submitter);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn remove_submitter(
        env: Env,
        admin: Address,
        submitter: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Submitter(submitter), &false);
        Ok(())
    }

    pub fn is_authorized_submitter(env: Env, submitter: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Submitter(submitter))
            .unwrap_or(false)
    }

    /// Admin-only: authorize `verifier` to decide evidence.
    pub fn add_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Verifier(verifier);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn remove_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Verifier(verifier), &false);
        Ok(())
    }

    pub fn is_authorized_verifier(env: Env, verifier: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Verifier(verifier))
            .unwrap_or(false)
    }

    // ── #1094 — submit milestone evidence ─────────────────────────────────

    /// Submit privacy-minimized evidence against an active milestone.
    ///
    /// Authorization is exact: the caller must be `recipient` itself, or an
    /// admin-allowlisted trusted submitter acting on the recipient's behalf.
    ///
    /// Evidence is versioned per `(program_id, milestone_id, recipient)`.
    /// Re-submitting the *same* `data_hash` as the current version is
    /// idempotent — it returns that version and changes nothing (no new
    /// version, no event), so a retried call cannot create a duplicate.
    /// Submitting a different hash creates the next version.
    pub fn submit_evidence(
        env: Env,
        submitter: Address,
        recipient: Address,
        program_id: BytesN<32>,
        milestone_id: u64,
        data_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        submitter.require_auth();

        let milestone = Self::load_milestone(&env, &program_id, milestone_id)?;
        if milestone.status != MilestoneStatus::Active {
            return Err(ContractError::MilestoneNotActive);
        }

        if submitter != recipient && !Self::is_authorized_submitter(env.clone(), submitter.clone())
        {
            return Err(ContractError::NotAuthorizedSubmitter);
        }

        let version_key = DataKey::EvidenceVersion(
            program_id.clone(),
            milestone_id,
            recipient.clone(),
        );
        let latest: u32 = env
            .storage()
            .persistent()
            .get(&version_key)
            .unwrap_or(0);

        // Idempotent re-submission of the current version.
        if latest > 0 {
            let existing: EvidenceRecord = env
                .storage()
                .persistent()
                .get(&DataKey::Evidence(
                    program_id.clone(),
                    milestone_id,
                    recipient.clone(),
                    latest,
                ))
                .ok_or(ContractError::EvidenceNotFound)?;
            if existing.data_hash == data_hash {
                return Ok(latest);
            }
        }

        let version = latest
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let now = env.ledger().timestamp();
        let record = EvidenceRecord {
            program_id: program_id.clone(),
            milestone_id,
            recipient: recipient.clone(),
            version,
            data_hash,
            submitted_by: submitter.clone(),
            submitted_at: now,
            status: EvidenceStatus::Pending,
            decided: false,
            decided_by: submitter,
            decided_at: 0,
            reason_code: ReasonCode::NotReviewed,
        };

        let evidence_key =
            DataKey::Evidence(program_id.clone(), milestone_id, recipient.clone(), version);
        env.storage().persistent().set(&evidence_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&evidence_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&version_key, &version);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("EVSUBMIT"),),
            (program_id, milestone_id, recipient, version),
        );

        Ok(version)
    }

    pub fn latest_evidence_version(
        env: Env,
        program_id: BytesN<32>,
        milestone_id: u64,
        recipient: Address,
    ) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::EvidenceVersion(program_id, milestone_id, recipient))
            .unwrap_or(0)
    }

    pub fn get_evidence(
        env: Env,
        program_id: BytesN<32>,
        milestone_id: u64,
        recipient: Address,
        version: u32,
    ) -> Result<EvidenceRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Evidence(program_id, milestone_id, recipient, version))
            .ok_or(ContractError::EvidenceNotFound)
    }

    // ── #1095 — verify milestone evidence ─────────────────────────────────

    fn is_coherent(decision: VerificationDecision, reason: ReasonCode) -> bool {
        use ReasonCode::*;
        use VerificationDecision::*;
        matches!(
            (decision, reason),
            (Approved, MeetsCriteria)
                | (
                    Rejected | ChangesRequested,
                    InsufficientEvidence | EvidenceMismatch | OutsideScope
                )
        )
    }

    /// Decide a specific evidence version. Only an admin-allowlisted
    /// verifier may call this, and a verifier who is the evidence's
    /// recipient or submitter is rejected as a conflict of interest.
    ///
    /// Each version is decidable at most once (`AlreadyDecided` on any
    /// retry), which is what guarantees an approval emits **at most one**
    /// payment-eligibility event. A changed decision on the same recipient
    /// requires submitting a new evidence version, preserving history.
    ///
    /// `reason` must be coherent with `decision`: `Approved` requires
    /// `MeetsCriteria`; `Rejected`/`ChangesRequested` require one of
    /// `InsufficientEvidence`, `EvidenceMismatch`, or `OutsideScope`.
    #[allow(clippy::too_many_arguments)]
    pub fn verify_evidence(
        env: Env,
        verifier: Address,
        program_id: BytesN<32>,
        milestone_id: u64,
        recipient: Address,
        version: u32,
        decision: VerificationDecision,
        reason: ReasonCode,
    ) -> Result<(), ContractError> {
        verifier.require_auth();

        if !Self::is_authorized_verifier(env.clone(), verifier.clone()) {
            return Err(ContractError::NotAuthorizedVerifier);
        }
        if !Self::is_coherent(decision, reason) {
            return Err(ContractError::InvalidReasonCode);
        }

        let key = DataKey::Evidence(
            program_id.clone(),
            milestone_id,
            recipient.clone(),
            version,
        );
        let mut record: EvidenceRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EvidenceNotFound)?;

        // Conflict of interest: the verifier cannot vouch for their own
        // submission, nor decide evidence belonging to themselves.
        if verifier == record.submitted_by || verifier == record.recipient {
            return Err(ContractError::VerifierConflict);
        }

        if record.decided || record.status != EvidenceStatus::Pending {
            return Err(ContractError::AlreadyDecided);
        }

        let status = match decision {
            VerificationDecision::Approved => EvidenceStatus::Approved,
            VerificationDecision::Rejected => EvidenceStatus::Rejected,
            VerificationDecision::ChangesRequested => EvidenceStatus::ChangesRequested,
        };

        let now = env.ledger().timestamp();
        record.status = status;
        record.decided = true;
        record.decided_by = verifier.clone();
        record.decided_at = now;
        record.reason_code = reason;

        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("EVDECID"),),
            (program_id.clone(), milestone_id, recipient.clone(), version, status, reason),
        );

        // Approval is the one decision that makes an installment payable —
        // emitted exactly once per version, since a decided version is terminal.
        if status == EvidenceStatus::Approved {
            env.events().publish(
                (symbol_short!("EVPASS"),),
                (program_id, milestone_id, recipient, version),
            );
        }

        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
