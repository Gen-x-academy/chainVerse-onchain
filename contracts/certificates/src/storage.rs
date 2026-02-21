use soroban_sdk::{contracttype, Address, Env, BytesN, Symbol,symbol_short};

use crate::types::Certificate;

const CERT_OWNER: soroban_sdk::Symbol = symbol_short!("CERTOWN");

#[contracttype]
pub enum DataKey {
    Certificate(BytesN<32>),             
    WalletCourse(Address, u32),          
}


pub fn set_certificate_owner(e: &Env, cert_id: u64, owner: &Address) {
    e.storage().instance().set(&(CERT_OWNER, cert_id), owner);
}

pub fn get_certificate_owner(e: &Env, cert_id: u64) -> Option<Address> {
    e.storage().instance().get(&(CERT_OWNER, cert_id))
}