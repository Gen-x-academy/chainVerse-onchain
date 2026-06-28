#![no_std]

const MAX_BATCH_SIZE: u32 = 100;
const AUTH_MIN_TTL: u32 = 17_280;
const AUTH_MAX_TTL: u32 = 518_400;

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
}

#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Initialized,
    Course(u64),
    Enrollment(Address, u64),
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
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    /// Register a course so students can enroll in it.
    pub fn register_course(env: Env, caller: Address, course_id: u64, price: i128) -> Result<(), PayoutError> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(PayoutError::NotInitialized)?;
        if caller != admin { return Err(PayoutError::Unauthorized); }
        caller.require_auth();
        env.storage().persistent().set(&DataKey::Course(course_id), &price);
        Ok(())
    }

    /// Pay for a course. Verifies the course exists (#625) and the student isn't
    /// already enrolled (#626) before charging.
    pub fn pay_for_course(
        env: Env,
        student: Address,
        course_id: u64,
    ) -> Result<(), PayoutError> {
        student.require_auth();

        // #625 — phantom course guard
        let price: i128 = env.storage().persistent()
            .get(&DataKey::Course(course_id))
            .ok_or(PayoutError::CourseNotFound)?;

        // #626 — double-payment prevention
        if env.storage().persistent().has(&DataKey::Enrollment(student.clone(), course_id)) {
            return Err(PayoutError::AlreadyEnrolled);
        }

        let token: Address = env.storage().instance().get(&DataKey::Token).ok_or(PayoutError::NotInitialized)?;
        TokenClient::new(&env, &token).transfer(&student, &env.current_contract_address(), &price);

        env.storage().persistent().set(&DataKey::Enrollment(student, course_id), &true);
        Ok(())
    }

    /// Executes a batch of payouts. Batch size must not exceed MAX_BATCH_SIZE (100).
    /// Each payout amount must be positive.
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
        let token: Address = env.storage().instance().get(&DataKey::Token).ok_or(PayoutError::NotInitialized)?;
        let client = TokenClient::new(&env, &token);
        for (recipient, amount) in payouts.iter() {
            if amount <= 0 { return Err(PayoutError::NegativeAmount); }
            client.transfer(&env.current_contract_address(), &recipient, &amount);
        }
        Ok(())
    }
}
