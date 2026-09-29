#![no_std]

mod errors;
mod storage;
mod types;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::{ReviewOutcome, RiskLevel, RuleVersion, RuleVersionSummary};

use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, Bytes, BytesN, Env, String,
};

use storage::{
    assert_not_paused, bump_instance, get_active_version, get_high_risk_approver,
    get_pending_admin, get_pending_admin_expiry, get_reviewer, get_rule_version,
    next_version, require_admin, require_reviewer, rule_version_exists, save_rule_version,
    set_active_version, set_admin, set_high_risk_approver, set_paused, set_pending_admin,
    set_reviewer, clear_pending_admin, peek_next_version,
};
use types::{ReviewOutcome as RO, RiskLevel as RL, RuleVersion as RV, RuleVersionSummary as RVS};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Max bytes for rule_payload (4 KiB).
const MAX_PAYLOAD_BYTES: u32 = 4_096;
/// Max bytes for cohort tag.
const MAX_COHORT_TAG_BYTES: u32 = 64;
/// 30-day pending admin TTL in ledger seconds.
const ADMIN_TRANSFER_TTL: u64 = 2_592_000;

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct ScholarshipFairnessReview;

#[contractimpl]
impl ScholarshipFairnessReview {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract.  Must be called exactly once.
    ///
    /// * `admin`            — address that governs the contract.
    /// * `reviewer`         — address authorised to record review outcomes.
    /// * `high_risk_approver` — second approver required for `RiskLevel::High`
    ///   changes before activation.  May equal `admin` only if explicitly
    ///   desired; best practice is a different address.
    pub fn init(
        env: Env,
        admin: Address,
        reviewer: Address,
        high_risk_approver: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        set_reviewer(&env, &reviewer);
        set_high_risk_approver(&env, &high_risk_approver);
        set_paused(&env, false);
        env.events().publish(
            (symbol_short!("FR_INIT"),),
            (admin, reviewer, high_risk_approver),
        );
        Ok(())
    }

    // ── Rule version management ───────────────────────────────────────────────

    /// Propose a new rule version.
    ///
    /// The caller becomes the **owner** of the version.  For `High` risk
    /// changes, the version cannot be activated until both the reviewer
    /// approves AND a second approver (different from the owner) confirms.
    ///
    /// # Privacy note
    /// `rule_payload` is stored verbatim on-chain.  Proposers **must not**
    /// include personal data, applicant identifiers, or biometric hashes in
    /// the payload.  Use a content-addressed CID or opaque identifier and
    /// keep the actual rule document off-chain.
    pub fn propose_rule_version(
        env: Env,
        caller: Address,
        risk_level: RiskLevel,
        rule_payload: Bytes,
        change_summary: String,
        test_cohort_tag: String,
        replaces_version: u64,
    ) -> Result<u64, ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;

        // Payload bounds
        if rule_payload.is_empty() {
            return Err(ContractError::EmptyRulePayload);
        }
        if rule_payload.len() > MAX_PAYLOAD_BYTES {
            return Err(ContractError::RulePayloadTooLarge);
        }
        if test_cohort_tag.len() > MAX_COHORT_TAG_BYTES {
            return Err(ContractError::CohortTagTooLong);
        }
        // Validate replaces_version if non-zero
        if replaces_version > 0 && !rule_version_exists(&env, replaces_version) {
            return Err(ContractError::RuleVersionNotFound);
        }

        let version = next_version(&env);
        let now = env.ledger().timestamp();

        let rv = RV {
            version,
            owner: caller.clone(),
            risk_level: risk_level.clone(),
            rule_payload,
            change_summary: change_summary.clone(),
            test_cohort_tag: test_cohort_tag.clone(),
            proposed_at: now,
            review_outcome: RO::Pending,
            review_notes: String::from_str(&env, ""),
            reviewed_at: 0,
            reviewer: None,
            second_approver: None,
            is_active: false,
            replaces_version,
        };
        save_rule_version(&env, &rv);

        env.events().publish(
            (symbol_short!("FR_PROP"),),
            (version, caller, risk_level, test_cohort_tag, replaces_version, now),
        );
        Ok(version)
    }

    /// Record a review outcome for an existing (Pending) rule version.
    ///
    /// Only the designated reviewer may call this.
    ///
    /// # Disparate-impact gate
    /// The reviewer is expected to have evaluated the proposed rules off-chain
    /// for prohibited proxies (geography, name patterns, institution tiers)
    /// and disparate-impact ratios before calling `Approved`.  The `notes`
    /// field should reference the off-chain analysis artefact (e.g. a
    /// content-addressed report hash).
    pub fn record_review(
        env: Env,
        caller: Address,
        version: u64,
        outcome: ReviewOutcome,
        notes: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_reviewer(&env, &caller)?;

        let mut rv = get_rule_version(&env, version)
            .ok_or(ContractError::RuleVersionNotFound)?;

        if rv.review_outcome != RO::Pending {
            return Err(ContractError::ReviewAlreadyRecorded);
        }

        rv.review_outcome = outcome.clone();
        rv.review_notes = notes.clone();
        rv.reviewed_at = env.ledger().timestamp();
        rv.reviewer = Some(caller.clone());
        save_rule_version(&env, &rv);

        env.events().publish(
            (symbol_short!("FR_RVWD"),),
            (version, caller, outcome, rv.reviewed_at),
        );
        Ok(())
    }

    /// Second-approver sign-off for High-risk versions.
    ///
    /// The second approver must differ from the version owner (proposer).
    /// Once both the reviewer has approved AND the second approver has
    /// signed off, the version becomes eligible for activation.
    pub fn second_approve(
        env: Env,
        caller: Address,
        version: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;

        // Only the designated high-risk approver may sign off.
        caller.require_auth();
        let approver = get_high_risk_approver(&env)
            .ok_or(ContractError::NotInitialized)?;
        if caller != approver {
            return Err(ContractError::Unauthorized);
        }

        let mut rv = get_rule_version(&env, version)
            .ok_or(ContractError::RuleVersionNotFound)?;

        // Must already be reviewer-approved
        if rv.review_outcome != RO::Approved
            && rv.review_outcome != RO::ApprovedWithConditions
        {
            return Err(ContractError::VersionNotApproved);
        }

        // Second approver must not be the proposer
        if caller == rv.owner {
            return Err(ContractError::ApproverMustDifferFromProposer);
        }

        rv.second_approver = Some(caller.clone());
        save_rule_version(&env, &rv);

        env.events().publish(
            (symbol_short!("FR_2APP"),),
            (version, caller, env.ledger().timestamp()),
        );
        Ok(())
    }

    /// Activate an approved rule version, making it the live rule-set.
    ///
    /// For `High` risk versions, `second_approve` must have been called first.
    /// The previously active version is not deleted — it remains in storage
    /// as a rollback target.
    pub fn activate_version(
        env: Env,
        caller: Address,
        version: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;

        let mut rv = get_rule_version(&env, version)
            .ok_or(ContractError::RuleVersionNotFound)?;

        // Must be reviewer-approved
        if rv.review_outcome != RO::Approved
            && rv.review_outcome != RO::ApprovedWithConditions
        {
            return Err(ContractError::VersionNotApproved);
        }

        // High-risk gate
        if rv.risk_level == RL::High && rv.second_approver.is_none() {
            return Err(ContractError::HighRiskApprovalRequired);
        }

        // Deactivate current version
        let prev_active = get_active_version(&env);
        if prev_active > 0 {
            if let Some(mut prev) = get_rule_version(&env, prev_active) {
                prev.is_active = false;
                save_rule_version(&env, &prev);
            }
        }

        rv.is_active = true;
        save_rule_version(&env, &rv);
        set_active_version(&env, version);

        env.events().publish(
            (symbol_short!("FR_ACTV"),),
            (version, caller, prev_active, env.ledger().timestamp()),
        );
        Ok(())
    }

    /// Roll back to a previously approved version.
    ///
    /// Target version must be `Approved` or `ApprovedWithConditions` and
    /// must not be the currently active version.  The rollback itself is
    /// treated as a new activation event (audited via `FR_RLLB` event).
    pub fn rollback_to(
        env: Env,
        caller: Address,
        target_version: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;

        let target = get_rule_version(&env, target_version)
            .ok_or(ContractError::RuleVersionNotFound)?;

        if target.review_outcome != RO::Approved
            && target.review_outcome != RO::ApprovedWithConditions
        {
            return Err(ContractError::RollbackTargetNotApproved);
        }

        let prev_active = get_active_version(&env);
        if prev_active > 0 {
            if let Some(mut prev) = get_rule_version(&env, prev_active) {
                prev.is_active = false;
                save_rule_version(&env, &prev);
            }
        }

        let mut rv = target;
        rv.is_active = true;
        save_rule_version(&env, &rv);
        set_active_version(&env, target_version);

        env.events().publish(
            (symbol_short!("FR_RLLB"),),
            (target_version, caller, prev_active, env.ledger().timestamp()),
        );
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return the summary (no review notes) for a specific version.
    pub fn get_version_summary(
        env: Env,
        version: u64,
    ) -> Result<RuleVersionSummary, ContractError> {
        bump_instance(&env);
        let rv = get_rule_version(&env, version)
            .ok_or(ContractError::RuleVersionNotFound)?;
        Ok(RVS {
            version: rv.version,
            owner: rv.owner,
            risk_level: rv.risk_level,
            change_summary: rv.change_summary,
            test_cohort_tag: rv.test_cohort_tag,
            proposed_at: rv.proposed_at,
            review_outcome: rv.review_outcome,
            reviewed_at: rv.reviewed_at,
            is_active: rv.is_active,
            replaces_version: rv.replaces_version,
        })
    }

    /// Return the currently active version number (0 = none).
    pub fn active_version(env: Env) -> u64 {
        bump_instance(&env);
        get_active_version(&env)
    }

    /// Return the next version number that would be assigned to a new proposal.
    pub fn next_version_preview(env: Env) -> u64 {
        bump_instance(&env);
        peek_next_version(&env)
    }

    /// Return whether the contract is paused.
    pub fn paused(env: Env) -> bool {
        bump_instance(&env);
        storage::is_paused(&env)
    }

    // ── Admin operations ──────────────────────────────────────────────────────

    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, true);
        env.events().publish((symbol_short!("FR_PAUS"),), caller);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, false);
        env.events().publish((symbol_short!("FR_UPAU"),), caller);
        Ok(())
    }

    pub fn set_reviewer(
        env: Env,
        caller: Address,
        new_reviewer: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;
        set_reviewer(&env, &new_reviewer);
        env.events()
            .publish((symbol_short!("FR_SRVW"),), (caller, new_reviewer));
        Ok(())
    }

    pub fn set_high_risk_approver(
        env: Env,
        caller: Address,
        new_approver: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;
        set_high_risk_approver(&env, &new_approver);
        env.events()
            .publish((symbol_short!("FR_SHKA"),), (caller, new_approver));
        Ok(())
    }

    /// Two-step admin transfer — propose.
    pub fn propose_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        let expiry = env
            .ledger()
            .timestamp()
            .saturating_add(ADMIN_TRANSFER_TTL);
        set_pending_admin(&env, &new_admin, expiry);
        env.events()
            .publish((symbol_short!("FR_PADM"),), (caller, new_admin, expiry));
        Ok(())
    }

    /// Two-step admin transfer — accept (new admin calls this).
    pub fn accept_admin(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        caller.require_auth();
        let pending = get_pending_admin(&env).ok_or(ContractError::Unauthorized)?;
        if caller != pending {
            return Err(ContractError::Unauthorized);
        }
        let expiry = get_pending_admin_expiry(&env);
        if env.ledger().timestamp() > expiry {
            clear_pending_admin(&env);
            return Err(ContractError::Unauthorized);
        }
        set_admin(&env, &caller);
        clear_pending_admin(&env);
        env.events()
            .publish((symbol_short!("FR_AADM"),), caller);
        Ok(())
    }

    /// WASM upgrade — admin only.
    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("FR_UPGD"),), new_wasm_hash);
        Ok(())
    }
}
