use soroban_sdk::{contracttype, Address, Env, BytesN, Symbol,symbol_short};

use crate::types::Certificate;
use crate::shared::types::Certificate;

const CERTIFICATE_KEY: soroban_sdk::Symbol = symbol_short!("CERT");
const CERT_OWNER: soroban_sdk::Symbol = symbol_short!("CERTOWN");

#[contracttype]
pub enum DataKey {
    Certificate(BytesN<32>),             
    WalletCourse(Address, u32),     
    Admin,
    Paused,     
}


pub fn set_certificate_owner(e: &Env, cert_id: u64, owner: &Address) {
    e.storage().instance().set(&(CERT_OWNER, cert_id), owner);
}

pub fn get_certificate_owner(e: &Env, cert_id: u64) -> Option<Address> {
    e.storage().instance().get(&(CERT_OWNER, cert_id))
}

pub fn get_certificate(
    env: &Env,
    wallet: &Address,
    course_id: u64,
) -> Option<Certificate> {
    env.storage().persistent().get(&(CERTIFICATE_KEY, wallet, course_id))
}

pub fn store_certificate(
    env: &Env,
    wallet: &Address,
    course_id: u64,
    cert: &Certificate,
) {
    env.storage()
        .persistent()
        .set(&(CERTIFICATE_KEY, wallet, course_id), cert);
}