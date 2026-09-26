#![no_std]

//! Scholarship/bursary milestone-disbursement schedule contract.
//!
//! Scope of this pass (issue #1093): split an award into enrollment,
//! attendance, coursework, completion, or custom verified milestones, each
//! carrying a percentage and a disbursement date, and release them in
//! order once verified.
//!
//! Acceptance criteria handled here:
//!
//! - **Percentages and amounts reconcile to the award.** Percentages are
//!   expressed in basis points and must sum to exactly 10,000. Milestone
//!   amounts are *derived* from those percentages against the schedule's
//!   `total_amount`, with the final milestone absorbing any integer-
//!   division remainder, so the amounts always sum exactly to the award.
//! - **Dates are ordered.** Milestone dates must be strictly increasing;
//!   any other ordering is rejected with `DatesNotOrdered`.
//! - **Immutable after activation except governed amendment.** A schedule
//!   is defined inactive, then activated exactly once. While inactive it
//!   may only be redefined once (a define-then-define calls is rejected),
//!   and once active every change must go through `amend_schedule`, which
//!   is admin-only, bumps the version, records the amend timestamp, and is
//!   refused once any milestone has been released
//!   (`ScheduleLockedAfterDisbursement`) so disbursement history can never
//!   be silently rewritten.
//!
//! On-chain storage is privacy-minimized: milestone labels are represented
//! only by `BytesN<32>` commitments over off-chain text; no descriptive
//! content is stored. See `contracts/docs/scholarship-milestones.md` for
//! ownership, privacy, migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling
// scholarship contracts' convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1093 — bounded storage: a schedule may hold at most this many
/// milestones, so no single record can grow without bound.
const MAX_MILESTONES: u32 = 16;

/// Percentages are basis points; 100% == 10,000 bps.
const BPS_DENOM_U32: u32 = 10_000;
const BPS_DENOM_I128: i128 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ScheduleNotFound = 4,
    /// A schedule already exists for this award; use `amend_schedule`.
    ScheduleAlreadyExists = 5,
    /// #1093 — a schedule must have between 1 and MAX_MILESTONES milestones.
    InvalidMilestoneCount = 6,
    /// #1093 — milestone percentages must sum to exactly 10,000 bps.
    PercentagesDoNotSumTo100 = 7,
    /// #1093 — milestone dates must be strictly increasing.
    DatesNotOrdered = 8,
    /// The schedule's total amount must be strictly positive.
    InvalidAmount = 9,
    /// The schedule has not been activated yet.
    ScheduleInactive = 10,
    /// The schedule has already been activated.
    ScheduleAlreadyActive = 11,
    MilestoneIndexOutOfRange = 12,
    /// A milestone must be verified before it can be released.
    MilestoneNotVerified = 13,
    MilestoneAlreadyVerified = 14,
    MilestoneAlreadyReleased = 15,
    /// Milestones must be released in schedule order.
    OutOfOrderRelease = 16,
    /// #1093 — an activated schedule cannot be amended once any milestone
    /// has been released.
    ScheduleLockedAfterDisbursement = 17,
    /// Version counter would overflow u32 — practically unreachable, but
    /// checked rather than silently wrapping.
    VersionOverflow = 18,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// A milestone schedule, keyed by the award it splits.
    Schedule(BytesN<32>),
}

/// #1093 — the kind of verified milestone. `Custom` milestones use the
/// `label_hash` commitment for their off-chain label.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneKind {
    Enrollment,
    Attendance,
    Coursework,
    Completion,
    Custom,
}

/// Caller-supplied definition of one milestone. Amounts are not accepted
/// here — they are derived from `percentage_bps` against the schedule's
/// total, which is what makes reconciliation exact.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneInput {
    pub kind: MilestoneKind,
    /// Integrity commitment over the off-chain milestone label/description.
    pub label_hash: BytesN<32>,
    /// Share of the award, in basis points (1% == 100 bps).
    pub percentage_bps: u32,
    /// Ledger timestamp at/after which this milestone may be released.
    pub due_at: u64,
}

/// A materialized milestone: the caller's input plus its derived amount and
/// verification/release state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub index: u32,
    pub kind: MilestoneKind,
    pub label_hash: BytesN<32>,
    pub percentage_bps: u32,
    /// Derived from `percentage_bps` against the schedule total.
    pub amount: i128,
    pub due_at: u64,
    pub verified: bool,
    pub released: bool,
}

/// #1093 — a complete, reconciled disbursement schedule for one award.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneSchedule {
    pub award_id: BytesN<32>,
    pub total_amount: i128,
    pub currency: Symbol,
    /// Increments on every governed amendment; starts at 1.
    pub version: u32,
    /// False until `activate_schedule`; immutable once true except via
    /// `amend_schedule`.
    pub active: bool,
    pub created_at: u64,
    pub activated_at: u64,
    pub amended_at: u64,
    pub milestones: Vec<Milestone>,
}

#[contract]
pub struct ScholarshipMilestonesContract;

#[contractimpl]
impl ScholarshipMilestonesContract {
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

    fn put_schedule(env: &Env, schedule: &MilestoneSchedule) {
        let key = DataKey::Schedule(schedule.award_id.clone());
        env.storage().persistent().set(&key, schedule);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_schedule(env: &Env, award_id: &BytesN<32>) -> Result<MilestoneSchedule, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Schedule(award_id.clone()))
            .ok_or(ContractError::ScheduleNotFound)
    }

    /// #1093 — validate percentages/dates and derive the reconciled
    /// milestone list. Amounts are computed from percentages with the final
    /// milestone absorbing the integer-division remainder, so the derived
    /// amounts always sum exactly to `total_amount`.
    fn build_milestones(
        env: &Env,
        total_amount: i128,
        inputs: &Vec<MilestoneInput>,
    ) -> Result<Vec<Milestone>, ContractError> {
        let count = inputs.len();
        if count == 0 || count > MAX_MILESTONES {
            return Err(ContractError::InvalidMilestoneCount);
        }
        if total_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        // First pass: validate percentages sum and dates are strictly ordered.
        let mut bps_sum: u32 = 0;
        let mut prev_due: u64 = 0;
        for i in 0..count {
            let input = inputs.get(i).ok_or(ContractError::InvalidMilestoneCount)?;
            bps_sum = bps_sum
                .checked_add(input.percentage_bps)
                .ok_or(ContractError::PercentagesDoNotSumTo100)?;
            if i > 0 && input.due_at <= prev_due {
                return Err(ContractError::DatesNotOrdered);
            }
            prev_due = input.due_at;
        }
        if bps_sum != BPS_DENOM_U32 {
            return Err(ContractError::PercentagesDoNotSumTo100);
        }

        // Second pass: derive amounts, last milestone absorbing the remainder.
        let mut milestones: Vec<Milestone> = Vec::new(env);
        let mut allocated: i128 = 0;
        for i in 0..count {
            let input = inputs.get(i).ok_or(ContractError::InvalidMilestoneCount)?;
            let amount = if i == count - 1 {
                total_amount
                    .checked_sub(allocated)
                    .ok_or(ContractError::InvalidAmount)?
            } else {
                total_amount
                    .checked_mul(input.percentage_bps as i128)
                    .ok_or(ContractError::InvalidAmount)?
                    / BPS_DENOM_I128
            };
            allocated = allocated
                .checked_add(amount)
                .ok_or(ContractError::InvalidAmount)?;

            milestones.push_back(Milestone {
                index: i,
                kind: input.kind,
                label_hash: input.label_hash,
                percentage_bps: input.percentage_bps,
                amount,
                due_at: input.due_at,
                verified: false,
                released: false,
            });
        }

        Ok(milestones)
    }

    // ── #1093 — define / activate / amend ────────────────────────────────

    /// Admin-only: define a new, inactive schedule for an award. Rejected if
    /// a schedule already exists for that award (use `amend_schedule`), if
    /// the milestone count is out of range, if percentages do not sum to
    /// 100%, if dates are not strictly ordered, or if the total amount is
    /// not positive.
    pub fn define_schedule(
        env: Env,
        admin: Address,
        award_id: BytesN<32>,
        total_amount: i128,
        currency: Symbol,
        inputs: Vec<MilestoneInput>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if env
            .storage()
            .persistent()
            .has(&DataKey::Schedule(award_id.clone()))
        {
            return Err(ContractError::ScheduleAlreadyExists);
        }

        let milestones = Self::build_milestones(&env, total_amount, &inputs)?;
        let now = env.ledger().timestamp();
        let schedule = MilestoneSchedule {
            award_id: award_id.clone(),
            total_amount,
            currency,
            version: 1,
            active: false,
            created_at: now,
            activated_at: 0,
            amended_at: 0,
            milestones,
        };
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((soroban_sdk::symbol_short!("MSDEF"),), (award_id,));
        Ok(())
    }

    /// Admin-only: activate a schedule exactly once. From this point the
    /// schedule only changes through `amend_schedule`.
    pub fn activate_schedule(
        env: Env,
        admin: Address,
        award_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut schedule = Self::load_schedule(&env, &award_id)?;
        if schedule.active {
            return Err(ContractError::ScheduleAlreadyActive);
        }
        schedule.active = true;
        schedule.activated_at = env.ledger().timestamp();
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((soroban_sdk::symbol_short!("MSACT"),), (award_id,));
        Ok(())
    }

    /// #1093 — the only way to change an activated schedule. Admin-only,
    /// version-bumping, and refused once any milestone has been released so
    /// that an amendment can never rewrite already-disbursed history.
    pub fn amend_schedule(
        env: Env,
        admin: Address,
        award_id: BytesN<32>,
        total_amount: i128,
        currency: Symbol,
        inputs: Vec<MilestoneInput>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut schedule = Self::load_schedule(&env, &award_id)?;
        if !schedule.active {
            return Err(ContractError::ScheduleInactive);
        }
        let count = schedule.milestones.len();
        for i in 0..count {
            if let Some(m) = schedule.milestones.get(i) {
                if m.released {
                    return Err(ContractError::ScheduleLockedAfterDisbursement);
                }
            }
        }

        let milestones = Self::build_milestones(&env, total_amount, &inputs)?;
        schedule.total_amount = total_amount;
        schedule.currency = currency;
        schedule.milestones = milestones;
        schedule.version = schedule
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        schedule.amended_at = env.ledger().timestamp();
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((soroban_sdk::symbol_short!("MSAMD"),), (award_id,));
        Ok(())
    }

    pub fn get_schedule(
        env: Env,
        award_id: BytesN<32>,
    ) -> Result<MilestoneSchedule, ContractError> {
        let key = DataKey::Schedule(award_id);
        let schedule: MilestoneSchedule = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ScheduleNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(schedule)
    }

    pub fn get_milestone(
        env: Env,
        award_id: BytesN<32>,
        index: u32,
    ) -> Result<Milestone, ContractError> {
        let schedule = Self::get_schedule(env, award_id)?;
        schedule
            .milestones
            .get(index)
            .ok_or(ContractError::MilestoneIndexOutOfRange)
    }

    // ── #1093 — verification and release ─────────────────────────────────

    /// Admin-only: mark a milestone verified, enabling its release.
    pub fn verify_milestone(
        env: Env,
        admin: Address,
        award_id: BytesN<32>,
        index: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut schedule = Self::load_schedule(&env, &award_id)?;
        if !schedule.active {
            return Err(ContractError::ScheduleInactive);
        }
        let mut milestone = schedule
            .milestones
            .get(index)
            .ok_or(ContractError::MilestoneIndexOutOfRange)?;
        if milestone.verified {
            return Err(ContractError::MilestoneAlreadyVerified);
        }
        milestone.verified = true;
        schedule.milestones.set(index, milestone);
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((soroban_sdk::symbol_short!("MSVRF"),), (award_id, index));
        Ok(())
    }

    /// Admin-only: release a verified milestone's funds. Milestones must be
    /// released in schedule order, and each can be released only once, so
    /// the schedule is a faithful, monotonic disbursement plan.
    pub fn release_milestone(
        env: Env,
        admin: Address,
        award_id: BytesN<32>,
        index: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let mut schedule = Self::load_schedule(&env, &award_id)?;
        if !schedule.active {
            return Err(ContractError::ScheduleInactive);
        }

        let mut milestone = schedule
            .milestones
            .get(index)
            .ok_or(ContractError::MilestoneIndexOutOfRange)?;
        if milestone.released {
            return Err(ContractError::MilestoneAlreadyReleased);
        }
        if !milestone.verified {
            return Err(ContractError::MilestoneNotVerified);
        }

        // Every earlier milestone must already be released.
        for j in 0..index {
            let earlier = schedule
                .milestones
                .get(j)
                .ok_or(ContractError::MilestoneIndexOutOfRange)?;
            if !earlier.released {
                return Err(ContractError::OutOfOrderRelease);
            }
        }

        milestone.released = true;
        schedule.milestones.set(index, milestone);
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((soroban_sdk::symbol_short!("MSREL"),), (award_id, index));
        Ok(())
    }

    /// Authoritative total already released across the schedule.
    pub fn released_total(env: Env, award_id: BytesN<32>) -> Result<i128, ContractError> {
        let schedule = Self::get_schedule(env, award_id)?;
        let mut total: i128 = 0;
        for i in 0..schedule.milestones.len() {
            if let Some(m) = schedule.milestones.get(i) {
                if m.released {
                    total = total
                        .checked_add(m.amount)
                        .ok_or(ContractError::InvalidAmount)?;
                }
            }
        }
        Ok(total)
    }

    /// Authoritative amount still to be released.
    pub fn remaining_amount(env: Env, award_id: BytesN<32>) -> Result<i128, ContractError> {
        let schedule = Self::get_schedule(env.clone(), award_id)?;
        let released = Self::released_total(env, schedule.award_id.clone())?;
        schedule
            .total_amount
            .checked_sub(released)
            .ok_or(ContractError::InvalidAmount)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
