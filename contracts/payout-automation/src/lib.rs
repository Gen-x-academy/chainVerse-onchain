#![no_std]

#[cfg(test)]
mod tests;

#[cfg(test)]
mod payment_test;

#[cfg(test)]
mod test;

#[cfg(test)]
mod suite;

const MAX_BATCH_SIZE: u32 = 100;

// TTL constants: ~1 year at 6-second ledgers (issue #735)
const PAYOUT_MIN_TTL: u32 = 3_110_400;
const PAYOUT_MAX_TTL: u32 = 6_220_800;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short,
    token::Client as TokenClient, Address, BytesN, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PayoutError {
    Unauthorized = 1,
    NotInitialized = 2,
    AlreadyInitialized = 3,
    BatchTooLarge = 4,
    NegativeAmount = 5,
    CourseNotFound = 6,
    AlreadyEnrolled = 7,
    InsufficientTreasury = 8,
    TooEarly = 9,
    NotScheduled = 10,
    ScheduleInPast = 11,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Initialized,
    Course(u64),
    CourseFee(u64),
    Enrollment(Address, u64),
    Treasury,
    ScheduledPayout(u64),
    ScheduledPayoutCounter,
}

/// A payout scheduled for a future ledger timestamp (issue #734).
/// Requires `#[contracttype]` for stable XDR serialization (issue #738).
#[contracttype]
#[derive(Clone)]
pub struct ScheduledPayout {
    pub recipient: Address,
    pub amount: i128,
    /// Unix timestamp (seconds) at or after which the payout may be executed.
    pub execute_after: u64,
}

#[contract]
pub struct PayoutAutomation;

#[contractimpl]
impl PayoutAutomation {
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), PayoutError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(PayoutError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        // Default treasury to admin; can be overridden via set_treasury.
        env.storage().instance().set(&DataKey::Treasury, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    /// Admin-only: set the treasury address that receives platform fees.
    pub fn set_treasury(env: Env, caller: Address, treasury: Address) -> Result<(), PayoutError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(PayoutError::NotInitialized)?;
        if caller != admin {
            return Err(PayoutError::Unauthorized);
        }
        caller.require_auth();
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        Ok(())
    }

    /// Register a course so students can enroll in it.
    /// `fee_bps` is the platform fee in basis points (0–10000). 100 bps = 1%.
    pub fn register_course(
        env: Env,
        caller: Address,
        course_id: u64,
        price: i128,
        fee_bps: u32,
    ) -> Result<(), PayoutError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(PayoutError::NotInitialized)?;
        if caller != admin {
            return Err(PayoutError::Unauthorized);
        }
        caller.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::Course(course_id), &price);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Course(course_id), PAYOUT_MIN_TTL, PAYOUT_MAX_TTL);
        env.storage()
            .persistent()
            .set(&DataKey::CourseFee(course_id), &fee_bps);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::CourseFee(course_id), PAYOUT_MIN_TTL, PAYOUT_MAX_TTL);
        Ok(())
    }

    /// Pay for a course. Verifies the course exists (#625) and the student isn't
    /// already enrolled (#626) before charging.
    ///
    /// If the course has a non-zero `fee_bps`, the platform fee is forwarded to
    /// the treasury address. The remainder stays in the contract (for creator payouts).
    pub fn pay_for_course(
        env: Env,
        student: Address,
        course_id: u64,
    ) -> Result<(), PayoutError> {
        student.require_auth();

        // #625 — phantom course guard
        let price: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Course(course_id))
            .ok_or(PayoutError::CourseNotFound)?;

        // #626 — double-payment prevention
        if env
            .storage()
            .persistent()
            .has(&DataKey::Enrollment(student.clone(), course_id))
        {
            return Err(PayoutError::AlreadyEnrolled);
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(PayoutError::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token);

        // Transfer the full course price from the student to the contract.
        token_client.transfer(&student, &env.current_contract_address(), &price);

        // If a platform fee (in bps) is configured, forward it to the treasury.
        let fee_bps: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::CourseFee(course_id))
            .unwrap_or(0);

        if fee_bps > 0 {
            let fee_amount = price * (fee_bps as i128) / 10_000_i128;
            if fee_amount > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::Treasury)
                    .ok_or(PayoutError::NotInitialized)?;
                token_client.transfer(&env.current_contract_address(), &treasury, &fee_amount);
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::Enrollment(student.clone(), course_id), &true);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Enrollment(student, course_id), PAYOUT_MIN_TTL, PAYOUT_MAX_TTL);
        Ok(())
    }

    /// Executes a batch of payouts. Batch size must not exceed MAX_BATCH_SIZE (100).
    ///
    /// Uses a two-pass approach: ALL amounts are validated before ANY transfer is
    /// executed. This guarantees atomicity — a batch with even one invalid entry is
    /// rejected in full with no tokens moved (fixes issue #301 / #729).
    pub fn execute(
        env: Env,
        caller: Address,
        payouts: Vec<(Address, i128)>,
    ) -> Result<(), PayoutError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(PayoutError::NotInitialized)?;
        if caller != admin { return Err(PayoutError::Unauthorized); }
        caller.require_auth();
        if payouts.len() > MAX_BATCH_SIZE {
            return Err(PayoutError::BatchTooLarge);
        }

        // --- Pass 1: validate ALL amounts before touching any balances ---
        // This ensures a batch with any invalid entry is rejected entirely,
        // preventing partial execution that could leave funds in an inconsistent state.
        for (_recipient, amount) in payouts.iter() {
            if amount <= 0 {
                return Err(PayoutError::NegativeAmount);
            }
        }

        // --- Pass 1b: verify the contract holds enough funds for the full batch ---
        let token: Address = env.storage().instance().get(&DataKey::Token).ok_or(PayoutError::NotInitialized)?;
        let client = TokenClient::new(&env, &token);
        let contract_balance = client.balance(&env.current_contract_address());
        let mut total: i128 = 0;
        for (_recipient, amount) in payouts.iter() {
            total += amount;
        }
        if contract_balance < total {
            return Err(PayoutError::InsufficientTreasury);
        }

        // --- Pass 2: all amounts are valid — now execute all transfers ---
        for (recipient, amount) in payouts.iter() {
            client.transfer(&env.current_contract_address(), &recipient, &amount);
        }
        Ok(())
    }

    /// Schedule a single payout to be executed no earlier than `execute_after`
    /// (Unix seconds). Returns the schedule ID.
    ///
    /// `execute_after` must be strictly in the future (> current ledger timestamp).
    pub fn schedule_payout(
        env: Env,
        caller: Address,
        recipient: Address,
        amount: i128,
        execute_after: u64,
    ) -> Result<u64, PayoutError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(PayoutError::NotInitialized)?;
        if caller != admin {
            return Err(PayoutError::Unauthorized);
        }
        caller.require_auth();

        if amount <= 0 {
            return Err(PayoutError::NegativeAmount);
        }

        // Reject schedules set in the past to prevent accidental immediate execution.
        if execute_after <= env.ledger().timestamp() {
            return Err(PayoutError::ScheduleInPast);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ScheduledPayoutCounter)
            .unwrap_or(0_u64);

        let schedule = ScheduledPayout { recipient, amount, execute_after };
        let key = DataKey::ScheduledPayout(id);
        env.storage().persistent().set(&key, &schedule);
        env.storage().persistent().extend_ttl(&key, PAYOUT_MIN_TTL, PAYOUT_MAX_TTL);

        // Increment counter in instance storage.
        env.storage().instance().set(&DataKey::ScheduledPayoutCounter, &(id + 1));

        env.events().publish((symbol_short!("scheduled"),), id);
        Ok(id)
    }

    /// Execute a previously scheduled payout by ID.
    ///
    /// Fails with `TooEarly` if the current ledger timestamp is before
    /// `execute_after`, and `NotScheduled` if no such schedule exists.
    pub fn execute_scheduled(
        env: Env,
        caller: Address,
        schedule_id: u64,
    ) -> Result<(), PayoutError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(PayoutError::NotInitialized)?;
        if caller != admin {
            return Err(PayoutError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::ScheduledPayout(schedule_id);
        let schedule: ScheduledPayout = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(PayoutError::NotScheduled)?;

        if env.ledger().timestamp() < schedule.execute_after {
            return Err(PayoutError::TooEarly);
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(PayoutError::NotInitialized)?;
        let client = TokenClient::new(&env, &token);

        let contract_balance = client.balance(&env.current_contract_address());
        if contract_balance < schedule.amount {
            return Err(PayoutError::InsufficientTreasury);
        }

        client.transfer(&env.current_contract_address(), &schedule.recipient, &schedule.amount);

        // Remove the schedule entry after execution to reclaim storage.
        env.storage().persistent().remove(&key);

        env.events().publish((symbol_short!("paid_out"),), schedule_id);
        Ok(())
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), PayoutError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(PayoutError::NotInitialized)?;
        if caller != admin { return Err(PayoutError::Unauthorized); }
        caller.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }
}
