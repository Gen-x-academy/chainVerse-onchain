#![no_std]

mod course;
mod errors;
mod events;
mod fee;
mod storage;

#[cfg(test)]
mod test;

pub use course::Course;
pub use errors::ContractError;
pub use storage::{PaymentRecord, DataKey};

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, String, Symbol};
use storage::{
    read_admin, write_admin, read_token, write_token, read_fee_percent, write_fee_percent,
    read_refund_window_seconds, write_refund_window_seconds, is_enrolled, set_enrollment,
    remove_enrollment, read_payment_record, write_payment_record, read_instructor_balance,
    write_instructor_balance, MIN_TTL, MAX_TTL,
};

const CONTRACT_VERSION: &str = "1.0.0";

#[contract]
pub struct PaymentContract;

#[contractimpl]
impl PaymentContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        fee_percent: u32,
        refund_window_seconds: u64,
    ) -> Result<(), ContractError> {
        admin.require_auth();

        if env.storage().persistent().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }

        if fee_percent > 2000 {
            return Err(ContractError::InvalidFee);
        }

        write_admin(&env, &admin);
        write_token(&env, &token);
        write_fee_percent(&env, fee_percent);
        write_refund_window_seconds(&env, refund_window_seconds);

        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Admin, MIN_TTL, MAX_TTL);

        Ok(())
    }

    pub fn set_fee(env: Env, caller: Address, fee_percent: u32) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();

        if fee_percent > 2000 {
            return Err(ContractError::InvalidFee);
        }

        write_fee_percent(&env, fee_percent);
        events::fee_set(&env, fee_percent);
        Ok(())
    }

    pub fn pay_for_course(
        env: Env,
        student: Address,
        course_id: Symbol,
        instructor: Address,
        amount: i128,
    ) -> Result<(), ContractError> {
        student.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }

        if is_enrolled(&env, &student, &course_id) {
            return Err(ContractError::AlreadyEnrolled);
        }

        let token = read_token(&env);
        let fee_percent = read_fee_percent(&env);

        let fee = fee::calculate_fee(amount, fee_percent)?;
        let instructor_amount = amount - fee;

        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&student, &env.current_contract_address(), &amount);

        set_enrollment(&env, &student, &course_id);

        let record = PaymentRecord {
            student: student.clone(),
            course_id: course_id.clone(),
            amount,
            paid_at: env.ledger().timestamp(),
        };
        write_payment_record(&env, &record);

        let current_balance = read_instructor_balance(&env, &instructor);
        write_instructor_balance(&env, &instructor, current_balance + instructor_amount);

        events::payment_recorded(&env, student, course_id, amount, instructor);

        Ok(())
    }

    pub fn refund(
        env: Env,
        student: Address,
        course_id: Symbol,
    ) -> Result<(), ContractError> {
        let admin = read_admin(&env)?;
        admin.require_auth();

        if !is_enrolled(&env, &student, &course_id) {
            return Err(ContractError::NotEnrolled);
        }

        let payment = read_payment_record(&env, &student, &course_id)?;
        let refund_window: u64 = read_refund_window_seconds(&env);

        if env.ledger().timestamp() > payment.paid_at + refund_window {
            return Err(ContractError::RefundWindowExpired);
        }

        remove_enrollment(&env, &student, &course_id);

        let token = read_token(&env);
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &student, &payment.amount);

        events::refund_issued(&env, student, course_id, payment.amount);

        Ok(())
    }

    pub fn withdraw_earnings(env: Env, instructor: Address) -> Result<(), ContractError> {
        instructor.require_auth();

        let balance = read_instructor_balance(&env, &instructor);
        if balance <= 0 {
            return Err(ContractError::InsufficientBalance);
        }

        write_instructor_balance(&env, &instructor, 0);

        let token = read_token(&env);
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &instructor, &balance);

        events::withdrawal_processed(&env, instructor, balance);

        Ok(())
    }

    pub fn get_instructor_balance(env: Env, instructor: Address) -> i128 {
        read_instructor_balance(&env, &instructor)
    }

    pub fn is_enrolled(env: Env, student: Address, course_id: Symbol) -> bool {
        is_enrolled(&env, &student, &course_id)
    }

    pub fn get_payment_record(
        env: Env,
        student: Address,
        course_id: Symbol,
    ) -> Result<PaymentRecord, ContractError> {
        read_payment_record(&env, &student, &course_id)
    }

    pub fn get_fee_percent(env: Env) -> u32 {
        read_fee_percent(&env)
    }

    pub fn get_refund_window_seconds(env: Env) -> u64 {
        read_refund_window_seconds(&env)
    }

    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}
