use soroban_sdk::{contracttype, Address, Env, BytesN, Symbol};

use crate::types::Certificate;

#[contracttype]
pub enum DataKey {
    Certificate(BytesN<32>),             // certificate_id -> Certificate
    WalletCourse(Address, u32),          // (wallet, course_id) -> certificate_id
}