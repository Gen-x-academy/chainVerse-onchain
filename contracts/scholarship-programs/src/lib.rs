#![no_std]

//! Scholarship/bursary program configuration contract.
//!
//! Scope of this pass (issues #1062, #1064): let an admin define a
//! program's application window (open/close instants, timezone offset,
//! grace period) and its award inventory/budget ceiling (max recipients,
//! per-award amount, total budget), then let callers deterministically
//! check window state and atomically reserve/release award slots without
//! ever over-committing capacity or budget.
//!
//! This contract holds no applicant data at all — it is pure program
//! configuration and aggregate counters, so there is nothing
//! privacy-sensitive to minimize here beyond keeping it that way. See
//! `contracts/docs/scholarship-programs.md` for ownership, privacy,
//! migration, and operational notes.
//!
//! Cohort/academic-term scoping (#1063) and versioned program terms
//! (#1065) are tracked separately and are not implemented here.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// #1062 — opens_at must be strictly before closes_at.
    InvalidWindow = 4,
    WindowNotFound = 5,
    /// #1062 — closes_at + grace_period_seconds would overflow u64.
    WindowOverflow = 6,
    BudgetNotFound = 7,
    /// #1064 — max_recipients/per_award_amount must be > 0, and
    /// total_budget must cover max_recipients * per_award_amount.
    InvalidBudgetConfig = 8,
    /// #1064 — reserving another award would exceed max_recipients.
    CapacityExceeded = 9,
    /// #1064 — reserving another award would exceed total_budget.
    BudgetExceeded = 10,
    /// #1064 — releasing an award when none are currently reserved.
    NoAwardsReserved = 11,
    /// #1064 — `configure_award_budget` was called on a program that
    /// already has reserved awards. The budget may be replaced only
    /// before any award is reserved; see the note on the function.
    BudgetAlreadyCommitted = 12,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1062 — a program's application window.
    Window(BytesN<32>),
    /// #1064 — a program's award inventory/budget state.
    Budget(BytesN<32>),
}

/// #1062 — a program's application-window configuration. `opens_at`/
/// `closes_at` are ledger timestamps (seconds, UTC); `timezone_offset_minutes`
/// is display-only metadata for off-chain UIs (all on-chain comparisons use
/// the UTC instants, which is what makes them deterministic).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramWindow {
    pub opens_at: u64,
    pub closes_at: u64,
    pub timezone_offset_minutes: i32,
    pub grace_period_seconds: u64,
}

/// #1064 — a program's award inventory and budget ceiling. `awarded_count`
/// and `committed_amount` are the authoritative "remaining capacity"
/// source — never derived from anything off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwardBudget {
    pub max_recipients: u32,
    pub per_award_amount: i128,
    pub total_budget: i128,
    pub awarded_count: u32,
    pub committed_amount: i128,
}

#[contract]
pub struct ScholarshipProgramsContract;

#[contractimpl]
impl ScholarshipProgramsContract {
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

    // ── #1062 — application windows ──────────────────────────────────────

    /// Admin-only: create or replace a program's application window.
    /// Changing the window has no effect on any already-submitted
    /// application — this contract holds no application records at all,
    /// so there is nothing here that could "silently invalidate" one.
    pub fn set_program_window(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        opens_at: u64,
        closes_at: u64,
        timezone_offset_minutes: i32,
        grace_period_seconds: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if opens_at >= closes_at {
            return Err(ContractError::InvalidWindow);
        }
        // Checked so a pathological grace period can't silently wrap.
        closes_at
            .checked_add(grace_period_seconds)
            .ok_or(ContractError::WindowOverflow)?;

        let window = ProgramWindow {
            opens_at,
            closes_at,
            timezone_offset_minutes,
            grace_period_seconds,
        };

        let key = DataKey::Window(program_id.clone());
        env.storage().persistent().set(&key, &window);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("WINSET"),), (program_id,));
        Ok(())
    }

    pub fn get_program_window(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramWindow, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Window(program_id))
            .ok_or(ContractError::WindowNotFound)
    }

    /// True iff the current ledger time falls within `[opens_at, closes_at]`
    /// inclusive. Deterministic: depends only on ledger time and stored
    /// config, never on caller input or off-chain data.
    pub fn is_open(env: Env, program_id: BytesN<32>) -> Result<bool, ContractError> {
        let window = Self::get_program_window(env.clone(), program_id)?;
        let now = env.ledger().timestamp();
        Ok(now >= window.opens_at && now <= window.closes_at)
    }

    /// True iff the current ledger time is after `closes_at` but still
    /// within the configured grace period.
    pub fn is_within_grace(env: Env, program_id: BytesN<32>) -> Result<bool, ContractError> {
        let window = Self::get_program_window(env.clone(), program_id)?;
        let now = env.ledger().timestamp();
        if now <= window.closes_at {
            return Ok(false);
        }
        let grace_end = window
            .closes_at
            .checked_add(window.grace_period_seconds)
            .ok_or(ContractError::WindowOverflow)?;
        Ok(now <= grace_end)
    }

    // ── #1064 — award inventory / budget ceilings ────────────────────────

    /// Admin-only: configure (or replace, before any awards are reserved)
    /// a program's award inventory. Rejects a configuration where the
    /// stated budget couldn't even cover `max_recipients` at
    /// `per_award_amount` each — a reserve-rule sanity check up front,
    /// rather than discovering it mid-intake.
    ///
    /// A program that already has reserved awards cannot be reconfigured.
    /// Replacing the budget mid-batch would reset `awarded_count` and
    /// `committed_amount` to zero, which is not a neutral edit: it hands
    /// back capacity that has genuinely been committed and lets a program
    /// over-commit against its own `total_budget` — a single admin call
    /// would turn a four-slot batch into an unbounded one. Sponsors who
    /// need to change the terms of a live batch must close it and open a
    /// new program, which is the same rule the lifecycle already enforces.
    pub fn configure_award_budget(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        max_recipients: u32,
        per_award_amount: i128,
        total_budget: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if max_recipients == 0 || per_award_amount <= 0 || total_budget <= 0 {
            return Err(ContractError::InvalidBudgetConfig);
        }
        let required = (max_recipients as i128)
            .checked_mul(per_award_amount)
            .ok_or(ContractError::InvalidBudgetConfig)?;
        if required > total_budget {
            return Err(ContractError::InvalidBudgetConfig);
        }

        // Refuse to overwrite a budget that is already in use, rather than
        // silently zeroing the commitment it records.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, AwardBudget>(&DataKey::Budget(program_id.clone()))
        {
            if existing.awarded_count > 0 || existing.committed_amount > 0 {
                return Err(ContractError::BudgetAlreadyCommitted);
            }
        }

        let budget = AwardBudget {
            max_recipients,
            per_award_amount,
            total_budget,
            awarded_count: 0,
            committed_amount: 0,
        };

        let key = DataKey::Budget(program_id.clone());
        env.storage().persistent().set(&key, &budget);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("BUDGSET"),), (program_id,));
        Ok(())
    }

    /// Admin-only: atomically reserve one award slot. Fails rather than
    /// over-committing if either `max_recipients` or `total_budget` would
    /// be exceeded. Soroban invocations are serialized by the ledger, so
    /// this single read-check-write is safe under concurrent callers —
    /// there is no interleaving window where two reservations could both
    /// read the same "remaining capacity" and both succeed.
    pub fn reserve_award(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Budget(program_id.clone());
        let mut budget: AwardBudget = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BudgetNotFound)?;

        if budget.awarded_count >= budget.max_recipients {
            return Err(ContractError::CapacityExceeded);
        }
        let new_committed = budget
            .committed_amount
            .checked_add(budget.per_award_amount)
            .ok_or(ContractError::BudgetExceeded)?;
        if new_committed > budget.total_budget {
            return Err(ContractError::BudgetExceeded);
        }

        budget.awarded_count = budget
            .awarded_count
            .checked_add(1)
            .ok_or(ContractError::CapacityExceeded)?;
        budget.committed_amount = new_committed;

        env.storage().persistent().set(&key, &budget);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("AWDRSV"),),
            (program_id, budget.awarded_count),
        );

        Ok(budget.awarded_count)
    }

    /// Admin-only: release one previously-reserved award slot (e.g. a
    /// recipient declined), returning its amount to the available budget.
    pub fn release_award(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Budget(program_id.clone());
        let mut budget: AwardBudget = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BudgetNotFound)?;

        if budget.awarded_count == 0 {
            return Err(ContractError::NoAwardsReserved);
        }

        budget.awarded_count = budget
            .awarded_count
            .checked_sub(1)
            .ok_or(ContractError::NoAwardsReserved)?;
        budget.committed_amount = budget
            .committed_amount
            .checked_sub(budget.per_award_amount)
            .ok_or(ContractError::NoAwardsReserved)?;

        env.storage().persistent().set(&key, &budget);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_award_budget(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<AwardBudget, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Budget(program_id))
            .ok_or(ContractError::BudgetNotFound)
    }

    /// Authoritative remaining recipient capacity: `max_recipients -
    /// awarded_count`. Never derived off-chain.
    ///
    /// `saturating_sub` rather than `checked_sub().unwrap_or(0)` because the
    /// two are equivalent here and the saturating form states the intent:
    /// capacity can never go negative, it just stops at zero.
    pub fn remaining_capacity(env: Env, program_id: BytesN<32>) -> Result<u32, ContractError> {
        let budget = Self::get_award_budget(env, program_id)?;
        Ok(budget.max_recipients.saturating_sub(budget.awarded_count))
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
