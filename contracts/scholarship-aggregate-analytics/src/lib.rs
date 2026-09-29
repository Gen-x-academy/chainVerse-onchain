#![no_std]

mod errors;
mod storage;
mod types;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::{
    CohortMetrics, CohortResult, DisbursementBatch, MetricDefinition, MetricField,
    SuppressedResult,
};

use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, String, Vec};

use storage::{
    assert_not_paused, bump_instance, cohort_exists, get_cohort, get_k_threshold,
    get_metric_definition, require_admin, require_recorder, save_cohort,
    save_metric_definition, set_admin, set_k_threshold, set_paused, set_recorder,
};
use types::{
    CohortMetrics as CM, CohortResult as CR, DisbursementBatch as DB, MetricDefinition as MD,
    MetricField as MF, SuppressedResult as SR,
};

/// Minimum k-anonymity threshold that the admin may configure.
/// Prevents accidental suppression bypass via threshold=1.
const MINIMUM_K_THRESHOLD: u64 = 5;

const MAX_COHORT_TAG_BYTES: u32 = 64;
const MAX_METRIC_KEY_BYTES: u32 = 32;
const MAX_METRIC_DEF_BYTES: u32 = 512;

#[contract]
pub struct ScholarshipAggregateAnalytics;

#[contractimpl]
impl ScholarshipAggregateAnalytics {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract.
    ///
    /// * `admin`     — governance address.
    /// * `recorder`  — the only address authorised to push metric updates.
    ///   Should be a trusted backend service account, not an EOA.
    /// * `k_threshold` — minimum cohort size before data is returned to
    ///   callers.  Must be ≥ 5 (hard floor enforced on-chain).
    ///
    /// # Privacy note
    /// This contract stores **only** aggregate counts and totals.
    /// Individual applicant addresses, application IDs, or identifiers must
    /// never be passed to any function on this contract.
    pub fn init(
        env: Env,
        admin: Address,
        recorder: Address,
        k_threshold: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();

        let effective_k = if k_threshold < MINIMUM_K_THRESHOLD {
            MINIMUM_K_THRESHOLD
        } else {
            k_threshold
        };

        set_admin(&env, &admin);
        set_recorder(&env, &recorder);
        set_k_threshold(&env, effective_k);
        set_paused(&env, false);

        env.events().publish(
            (symbol_short!("AA_INIT"),),
            (admin, recorder, effective_k),
        );
        Ok(())
    }

    // ── Metric recording (recorder-gated) ─────────────────────────────────────

    /// Create a new cohort bucket.  Must be called before any counter updates.
    pub fn create_cohort(
        env: Env,
        caller: Address,
        cohort_tag: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_recorder(&env, &caller)?;

        if cohort_tag.len() > MAX_COHORT_TAG_BYTES {
            return Err(ContractError::CohortTagTooLong);
        }
        if cohort_exists(&env, &cohort_tag) {
            // Idempotent — already exists is fine.
            return Ok(());
        }

        let now = env.ledger().timestamp();
        let cm = CM {
            cohort_tag: cohort_tag.clone(),
            total_applicants: 0,
            eligible_count: 0,
            review_completed_count: 0,
            approved_count: 0,
            rejected_count: 0,
            withdrawn_count: 0,
            disbursed_count: 0,
            total_disbursed_amount: 0,
            created_at: now,
            updated_at: now,
        };
        save_cohort(&env, &cm);

        env.events()
            .publish((symbol_short!("AA_CRTC"),), (cohort_tag, now));
        Ok(())
    }

    /// Increment a single metric counter for a cohort.
    ///
    /// Overflow is checked — the call returns `ArithmeticOverflow` rather
    /// than silently wrapping.
    pub fn increment(
        env: Env,
        caller: Address,
        cohort_tag: String,
        field: MetricField,
        delta: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_recorder(&env, &caller)?;

        let mut cm = get_cohort(&env, &cohort_tag)
            .ok_or(ContractError::CohortNotFound)?;

        Self::apply_increment(&mut cm, &field, delta)?;
        cm.updated_at = env.ledger().timestamp();
        save_cohort(&env, &cm);

        env.events().publish(
            (symbol_short!("AA_INCR"),),
            (cohort_tag, field, delta),
        );
        Ok(())
    }

    /// Decrement a single metric counter (e.g. to correct an over-count).
    pub fn decrement(
        env: Env,
        caller: Address,
        cohort_tag: String,
        field: MetricField,
        delta: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_recorder(&env, &caller)?;

        let mut cm = get_cohort(&env, &cohort_tag)
            .ok_or(ContractError::CohortNotFound)?;

        Self::apply_decrement(&mut cm, &field, delta)?;
        cm.updated_at = env.ledger().timestamp();
        save_cohort(&env, &cm);

        env.events().publish(
            (symbol_short!("AA_DECR"),),
            (cohort_tag, field, delta),
        );
        Ok(())
    }

    /// Record a batch of disbursement amounts for a cohort.
    ///
    /// Each element in `batch.amounts` is an i128 in the smallest token unit.
    /// Counts and total are accumulated atomically.  Overflow panics are
    /// impossible: amounts are i128 (positive by contract), totals are i128
    /// with checked arithmetic.
    pub fn record_disbursements(
        env: Env,
        caller: Address,
        batch: DisbursementBatch,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_recorder(&env, &caller)?;

        let mut cm = get_cohort(&env, &batch.cohort_tag)
            .ok_or(ContractError::CohortNotFound)?;

        let count = batch.amounts.len() as u64;
        cm.disbursed_count = cm
            .disbursed_count
            .checked_add(count)
            .ok_or(ContractError::ArithmeticOverflow)?;

        for amt in batch.amounts.iter() {
            cm.total_disbursed_amount = cm
                .total_disbursed_amount
                .checked_add(amt)
                .ok_or(ContractError::ArithmeticOverflow)?;
        }

        cm.updated_at = env.ledger().timestamp();
        save_cohort(&env, &cm);

        env.events().publish(
            (symbol_short!("AA_DISB"),),
            (batch.cohort_tag, count, cm.total_disbursed_amount),
        );
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return cohort metrics or a suppression notice.
    ///
    /// If `total_applicants < k_threshold` the function returns
    /// `CohortResult::Suppressed` with no underlying data exposed.
    /// The caller must always inspect the variant before using data.
    pub fn get_cohort_metrics(
        env: Env,
        cohort_tag: String,
    ) -> Result<CohortResult, ContractError> {
        bump_instance(&env);
        let cm = get_cohort(&env, &cohort_tag).ok_or(ContractError::CohortNotFound)?;
        let k = get_k_threshold(&env);
        if cm.total_applicants < k {
            return Ok(CR::Suppressed(SR {
                cohort_tag,
                k_threshold: k,
                reason: String::from_str(
                    &env,
                    "Cohort size is below the k-anonymity suppression threshold.",
                ),
            }));
        }
        Ok(CR::Metrics(cm))
    }

    /// Return the current k-anonymity threshold.
    pub fn k_threshold(env: Env) -> u64 {
        bump_instance(&env);
        get_k_threshold(&env)
    }

    // ── Metric definitions ────────────────────────────────────────────────────

    /// Upsert a metric definition (admin only).
    pub fn upsert_metric_definition(
        env: Env,
        caller: Address,
        key: String,
        name: String,
        definition: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_admin(&env, &caller)?;

        if key.len() > MAX_METRIC_KEY_BYTES {
            return Err(ContractError::MetricKeyTooLong);
        }
        if definition.len() > MAX_METRIC_DEF_BYTES {
            return Err(ContractError::MetricDefinitionTooLong);
        }

        let md = MD {
            key: key.clone(),
            name,
            definition,
            updated_at: env.ledger().timestamp(),
        };
        save_metric_definition(&env, &md);

        env.events()
            .publish((symbol_short!("AA_MDEF"),), (key, caller));
        Ok(())
    }

    /// Retrieve a metric definition.
    pub fn get_metric_definition(
        env: Env,
        key: String,
    ) -> Option<MetricDefinition> {
        bump_instance(&env);
        get_metric_definition(&env, &key)
    }

    // ── Admin operations ──────────────────────────────────────────────────────

    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, true);
        env.events().publish((symbol_short!("AA_PAUS"),), caller);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, false);
        env.events().publish((symbol_short!("AA_UPAU"),), caller);
        Ok(())
    }

    /// Update the k-anonymity threshold (admin only).
    /// New value must be ≥ `MINIMUM_K_THRESHOLD` (5).
    pub fn set_k_threshold(
        env: Env,
        caller: Address,
        new_k: u64,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        let effective = if new_k < MINIMUM_K_THRESHOLD {
            MINIMUM_K_THRESHOLD
        } else {
            new_k
        };
        set_k_threshold(&env, effective);
        env.events()
            .publish((symbol_short!("AA_SETK"),), (caller, effective));
        Ok(())
    }

    pub fn set_recorder(
        env: Env,
        caller: Address,
        new_recorder: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_recorder(&env, &new_recorder);
        env.events()
            .publish((symbol_short!("AA_SREC"),), (caller, new_recorder));
        Ok(())
    }

    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("AA_UPGD"),), new_wasm_hash);
        Ok(())
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn apply_increment(
        cm: &mut CM,
        field: &MF,
        delta: u64,
    ) -> Result<(), ContractError> {
        let target = Self::field_mut(cm, field);
        *target = target
            .checked_add(delta)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Ok(())
    }

    fn apply_decrement(
        cm: &mut CM,
        field: &MF,
        delta: u64,
    ) -> Result<(), ContractError> {
        let target = Self::field_mut(cm, field);
        *target = target
            .checked_sub(delta)
            .ok_or(ContractError::CounterUnderflow)?;
        Ok(())
    }

    fn field_mut<'a>(cm: &'a mut CM, field: &MF) -> &'a mut u64 {
        match field {
            MF::TotalApplicants => &mut cm.total_applicants,
            MF::EligibleCount => &mut cm.eligible_count,
            MF::ReviewCompletedCount => &mut cm.review_completed_count,
            MF::ApprovedCount => &mut cm.approved_count,
            MF::RejectedCount => &mut cm.rejected_count,
            MF::WithdrawnCount => &mut cm.withdrawn_count,
            MF::DisbursedCount => &mut cm.disbursed_count,
        }
    }
}
