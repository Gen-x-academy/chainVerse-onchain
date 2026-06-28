use soroban_sdk::{contracttype, Address, Bytes, Env};

use crate::{Certificate, ContractError};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    Certificate(Address, u64),
    BackendPubKey,
    NextTokenId,
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();

    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

pub fn require_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        Err(ContractError::ContractPaused)
    } else {
        Ok(())
    }
}

pub fn has_certificate(env: &Env, wallet: &Address, course_id: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Certificate(wallet.clone(), course_id))
}

pub fn load_certificate(env: &Env, wallet: &Address, course_id: u64) -> Option<Certificate> {
    let key = DataKey::Certificate(wallet.clone(), course_id);
    let cert = env.storage().persistent().get(&key);
    if cert.is_some() {
        env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    }
    cert
}

// ~1 year expressed in ledger entries (5-second close time)
pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;

pub fn save_certificate(env: &Env, wallet: &Address, course_id: u64, certificate: &Certificate) {
    let key = DataKey::Certificate(wallet.clone(), course_id);
    env.storage().persistent().set(&key, certificate);
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

pub fn remove_certificate(env: &Env, wallet: &Address, course_id: u64) {
    env.storage()
        .persistent()
        .remove(&DataKey::Certificate(wallet.clone(), course_id));
}

pub fn set_backend_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::BackendPubKey, pubkey);
}

pub fn get_backend_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::BackendPubKey)
}

pub fn next_token_id(env: &Env) -> u64 {
    let key = DataKey::NextTokenId;
    let id: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(id + 1));
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    id
}

pub fn get_certificate_by_key(env: &Env, wallet: &Address, course_id: u64) -> Option<Certificate> {
    load_certificate(env, wallet, course_id)
}

pub fn certificate_exists_by_key(env: &Env, wallet: &Address, course_id: u64) -> bool {
    has_certificate(env, wallet, course_id)
}
