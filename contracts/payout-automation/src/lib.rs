#![no_std]

#[cfg(test)]
mod tests;

#[cfg(test)]
mod payment_test;

#[cfg(test)]
mod test;

const MAX_BATCH_SIZE: u32 = 100;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, require, symbol_short,
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
    CourseInactive = 8,
    FeeTooHigh = 9,
    NothingToWithdraw = 10,
}

#[contracttype]
#[derive(Clone)]
pub struct Course {
    pub price: i128,
    pub instructor: Address,
    pub is_active: bool,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Token,
    Initialized,
    Course(u64),
    CourseFee(u64),
    Enrollment(Address, u64),
    InstructorBalance(Address),
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
    /// `fee_bps` is the platform fee in basis points (0–2000). 100 bps = 1%, max 20%.
    pub fn register_course(
        env: Env,
        caller: Address,
        course_id: u64,
        price: i128,
        instructor: Address,
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

        require!(fee_bps <= 2_000, PayoutError::FeeTooHigh);

        let course = Course {
            price,
            instructor,
            is_active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Course(course_id), &course);
        env.storage()
            .persistent()
            .set(&DataKey::CourseFee(course_id), &fee_bps);
        Ok(())
    }

    /// Admin-only: deactivate a course to prevent new enrollments.
    pub fn deactivate_course(
        env: Env,
        caller: Address,
        course_id: u64,
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

        let mut course: Course = env
            .storage()
            .persistent()
            .get(&DataKey::Course(course_id))
            .ok_or(PayoutError::CourseNotFound)?;

        course.is_active = false;

        env.storage()
            .persistent()
            .set(&DataKey::Course(course_id), &course);
        Ok(())
    }

    /// Pay for a course. Verifies the course exists (#684), is active (#684), and the student isn't
    /// already enrolled (#685) before charging.
    ///
    /// If the course has a non-zero `fee_bps`, the platform fee is forwarded to
    /// the treasury address. The instructor share is recorded in a balance (pull-payment model,
    /// issue #687) preventing re-entrancy attacks.
    pub fn pay_for_course(
        env: Env,
        student: Address,
        course_id: u64,
    ) -> Result<(), PayoutError> {
        student.require_auth();

        // #684 — phantom course guard and active status check
        let course: Course = env
            .storage()
            .persistent()
            .get(&DataKey::Course(course_id))
            .ok_or(PayoutError::CourseNotFound)?;

        require!(course.is_active, PayoutError::CourseInactive);

        // #685 — double-payment prevention
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
        token_client.transfer(&student, &env.current_contract_address(), &course.price);

        // If a platform fee (in bps) is configured, forward it to the treasury.
        let fee_bps: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::CourseFee(course_id))
            .unwrap_or(0);

        if fee_bps > 0 {
            let fee_amount = course.price * (fee_bps as i128) / 10_000_i128;
            if fee_amount > 0 {
                let treasury: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::Treasury)
                    .ok_or(PayoutError::NotInitialized)?;
                token_client.transfer(&env.current_contract_address(), &treasury, &fee_amount);
            }
        }

        // #687 — pull-payment model: record instructor balance instead of direct transfer
        let instructor_share = course.price - (course.price * (fee_bps as i128) / 10_000_i128);
        let balance_key = DataKey::InstructorBalance(course.instructor.clone());
        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&balance_key, &(current_balance + instructor_share));

        env.storage()
            .persistent()
            .set(&DataKey::Enrollment(student, course_id), &true);
        Ok(())
    }

    /// Instructor-callable: withdraw accumulated earnings from course sales.
    pub fn withdraw_earnings(env: Env, instructor: Address) -> Result<(), PayoutError> {
        instructor.require_auth();

        let balance_key = DataKey::InstructorBalance(instructor.clone());
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .ok_or(PayoutError::NothingToWithdraw)?;

        require!(balance > 0, PayoutError::NothingToWithdraw);

        // Clear balance before transfer to prevent re-entrancy
        env.storage()
            .persistent()
            .set(&balance_key, &0_i128);

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(PayoutError::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token);

        token_client.transfer(&env.current_contract_address(), &instructor, &balance);
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
