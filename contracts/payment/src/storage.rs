use soroban_sdk::{contracttype, Address, Env, Symbol};
use crate::errors::ContractError;

pub const MIN_TTL: u32 = 4096;
pub const MAX_TTL: u32 = 100_000;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Token,
    FeePercent,
    RefundWindowSeconds,
    Enrollment(Address, Symbol),
    PaymentRecord(Address, Symbol),
    InstructorBalance(Address),
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentRecord {
    pub student: Address,
    pub course_id: Symbol,
    pub amount: i128,
    pub paid_at: u64,
}

pub fn read_admin(env: &Env) -> Result<Address, crate::ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::Admin)
        .ok_or(crate::ContractError::NotInitialized)
}

pub fn write_admin(env: &Env, admin: &Address) {
    env.storage().persistent().set(&DataKey::Admin, admin);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::Admin, MIN_TTL, MAX_TTL);
}

pub fn read_token(env: &Env) -> Address {
    env.storage()
        .persistent()
        .get(&DataKey::Token)
        .unwrap_or(Address::from_contract_id(env, &[0; 32]))
}

pub fn write_token(env: &Env, token: &Address) {
    env.storage().persistent().set(&DataKey::Token, token);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::Token, MIN_TTL, MAX_TTL);
}

pub fn read_fee_percent(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::FeePercent)
        .unwrap_or(0)
}

pub fn write_fee_percent(env: &Env, fee: u32) {
    env.storage().persistent().set(&DataKey::FeePercent, &fee);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::FeePercent, MIN_TTL, MAX_TTL);
}

pub fn read_refund_window_seconds(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::RefundWindowSeconds)
        .unwrap_or(86400) // default 1 day
}

pub fn write_refund_window_seconds(env: &Env, seconds: u64) {
    env.storage()
        .persistent()
        .set(&DataKey::RefundWindowSeconds, &seconds);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::RefundWindowSeconds, MIN_TTL, MAX_TTL);
}

pub fn is_enrolled(env: &Env, student: &Address, course_id: &Symbol) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Enrollment(student.clone(), course_id.clone()))
}

pub fn set_enrollment(env: &Env, student: &Address, course_id: &Symbol) {
    env.storage()
        .persistent()
        .set(&DataKey::Enrollment(student.clone(), course_id.clone()), &true);
    env.storage().persistent().extend_ttl(
        &DataKey::Enrollment(student.clone(), course_id.clone()),
        MIN_TTL,
        MAX_TTL,
    );
}

pub fn remove_enrollment(env: &Env, student: &Address, course_id: &Symbol) {
    env.storage()
        .persistent()
        .remove(&DataKey::Enrollment(student.clone(), course_id.clone()));
}

pub fn read_payment_record(
    env: &Env,
    student: &Address,
    course_id: &Symbol,
) -> Result<PaymentRecord, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::PaymentRecord(student.clone(), course_id.clone()))
        .ok_or(ContractError::PaymentFailed)
}

pub fn write_payment_record(env: &Env, record: &PaymentRecord) {
    env.storage().persistent().set(
        &DataKey::PaymentRecord(record.student.clone(), record.course_id.clone()),
        record,
    );
    env.storage().persistent().extend_ttl(
        &DataKey::PaymentRecord(record.student.clone(), record.course_id.clone()),
        MIN_TTL,
        MAX_TTL,
    );
}

pub fn read_instructor_balance(env: &Env, instructor: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::InstructorBalance(instructor.clone()))
        .unwrap_or(0)
}

pub fn write_instructor_balance(env: &Env, instructor: &Address, balance: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::InstructorBalance(instructor.clone()), &balance);
    env.storage().persistent().extend_ttl(
        &DataKey::InstructorBalance(instructor.clone()),
        MIN_TTL,
        MAX_TTL,
    );
}
