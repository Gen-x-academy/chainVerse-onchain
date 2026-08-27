use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env};

use crate::{Certificate, ContractError};

// ~1 year expressed in ledger entries (5-second close time)
pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;
// Fix #841: bounded validity window for a pending admin transfer (~30 days, 5s/entry).
pub const ADMIN_TRANSFER_TTL: u64 = 518_400;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    Certificate(Address, BytesN<32>),
    BackendPubKey,
    /// Fix #628: persistent counter — survives contract upgrades (unlike instance storage)
    NextTokenId,
    /// Fix #691: separate minter authorization for mint_certificate
    Minter,
    ConsumedNonce(BytesN<32>),
    /// Fix #841: nominated pending admin for the two-step admin transfer.
    PendingAdmin,
    /// Fix #841: ledger timestamp at which the pending admin proposal expires.
    PendingAdminExpiry,
}

pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

pub fn set_pending_admin(env: &Env, new_admin: &Address, expiry: u64) {
    env.storage().instance().set(&DataKey::PendingAdmin, new_admin);
    env.storage().instance().set(&DataKey::PendingAdminExpiry, &expiry);
}

pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
    env.storage().instance().remove(&DataKey::PendingAdminExpiry);
}

pub fn get_pending_admin_expiry(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::PendingAdminExpiry)
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_minter(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Minter)
}

pub fn set_minter(env: &Env, minter: &Address) {
    env.storage().instance().set(&DataKey::Minter, minter);
}

pub fn get_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
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

pub fn certificate_exists(env: &Env, key: &(Address, BytesN<32>)) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Certificate(key.0.clone(), key.1.clone()))
}

pub fn get_certificate(env: &Env, key: &(Address, BytesN<32>)) -> Option<Certificate> {
    let dk = DataKey::Certificate(key.0.clone(), key.1.clone());
    let cert = env.storage().persistent().get(&dk);
    if cert.is_some() {
        env.storage().persistent().extend_ttl(&dk, MIN_TTL, MAX_TTL);
    }
    cert
}

pub fn save_certificate(env: &Env, key: (Address, BytesN<32>), cert: &Certificate) {
    let dk = DataKey::Certificate(key.0, key.1);
    env.storage().persistent().set(&dk, cert);
    env.storage().persistent().extend_ttl(&dk, MIN_TTL, MAX_TTL);
}

pub fn remove_certificate(env: &Env, wallet: &Address, course_id: &BytesN<32>) {
    env.storage()
        .persistent()
        .remove(&DataKey::Certificate(wallet.clone(), course_id.clone()));
}

pub fn set_backend_pubkey(env: &Env, pubkey: &Bytes) {
    env.storage().instance().set(&DataKey::BackendPubKey, pubkey);
}

pub fn get_backend_pubkey(env: &Env) -> Option<Bytes> {
    env.storage().instance().get(&DataKey::BackendPubKey)
}

pub fn nonce_consumed(env: &Env, nonce: &BytesN<32>) -> bool {
    env.storage().persistent().has(&DataKey::ConsumedNonce(nonce.clone()))
}

pub fn consume_nonce(env: &Env, nonce: &BytesN<32>) {
    let key = DataKey::ConsumedNonce(nonce.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

/// Fix #628: Returns the next token ID and increments the counter in persistent storage.
/// Persistent storage survives contract WASM upgrades, preventing duplicate IDs.
pub fn next_token_id(env: &Env) -> u64 {
    let key = DataKey::NextTokenId;
    let id: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(id + 1));
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    id
}
