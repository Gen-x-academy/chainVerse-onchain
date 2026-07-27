#![no_std]
use soroban_sdk::{contracttype, Address, Symbol};

/// A strongly-typed course identifier.
#[contracttype]
pub struct CourseId(pub Symbol);

/// A strongly-typed token amount (7 decimal places, matching CHV precision).
#[contracttype]
pub struct TokenAmount(pub i128);

/// Core course metadata shared across contracts.
#[contracttype]
pub struct CourseInfo {
    pub id:         CourseId,
    pub instructor: Address,
    pub price:      TokenAmount,
    pub is_active:  bool,
}

/// A student's enrollment record.
#[contracttype]
pub struct StudentRecord {
    pub student:   Address,
    pub course_id: CourseId,
    pub enrolled:  bool,
}
