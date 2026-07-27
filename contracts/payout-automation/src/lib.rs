#![no_std]

#[cfg(test)]
mod tests;

#[cfg(test)]
mod payment_test;

#[cfg(test)]
mod test;

const MAX_BATCH_SIZE: u32 = 50;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short,
    token::Client as TokenClient, Address, BytesN, Env, Vec,
};
mod events;

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
    InsufficientTreasuryBalance = 8,
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
            .set(&DataKey::CourseFee(course_id), &fee_bps);
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
        events::emit_course_paid(&env, &student, course_id, price);
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

        let token: Address = env.storage().instance().get(&DataKey::Token).ok_or(PayoutError::NotInitialized)?;
        let client = TokenClient::new(&env, &token);

        // --- Pass 2: pre-flight treasury balance check (#731) ---
        // Sum the full batch and verify the contract holds enough before touching balances.
        // Prevents mid-batch panics from exhausted funds.
        let total: i128 = payouts.iter().map(|(_r, a)| a).sum();
        let balance: i128 = client.balance(&env.current_contract_address());
        if balance < total {
            return Err(PayoutError::InsufficientTreasuryBalance);
        }

        // --- Pass 3: all checks passed — execute all transfers ---
        for (recipient, amount) in payouts.iter() {
            client.transfer(&env.current_contract_address(), &recipient, &amount);
            events::emit_payout_sent(&env, &recipient, amount);
        }

        events::emit_batch_executed(&env, &caller, payouts.len(), total);
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
