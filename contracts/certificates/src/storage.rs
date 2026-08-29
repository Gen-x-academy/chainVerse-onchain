use soroban_sdk::{contracttype, Address, Bytes, BytesN, Env};

use crate::{Certificate, ContractError};

// ~1 year expressed in ledger entries (5-second close time)
pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;
// Fix #841: bounded validity window for a pending admin transfer (~30 days, 5s/entry).
pub const ADMIN_TRANSFER_TTL: u64 = 518_400;
// Fix #835/#836: timelock window for key/minter rotation proposals (~7 days, 5s/entry).
pub const ROTATION_PROPOSAL_TTL: u64 = 120_960;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    Certificate(Address, BytesN<32>),
    /// Fix #834: stored as fixed-size BytesN<32> after validation in init.
    BackendPubKey,
    /// Fix #835: proposed new backend signing key (pending activation).
    PendingBackendPubKey,
    /// Fix #835: ledger timestamp at which the pending key rotation proposal expires.
    PendingBackendPubKeyExpiry,
    /// Fix #628: persistent counter — survives contract upgrades (unlike instance storage)
    NextTokenId,
    /// Fix #691: separate minter authorization for mint_certificate
    Minter,
    /// Fix #836: proposed new minter address (pending activation).
    PendingMinter,
    /// Fix #836: ledger timestamp at which the pending minter rotation proposal expires.
    PendingMinterExpiry,
    ConsumedNonce(BytesN<32>),
    /// Fix #839: reverse index — token ID to (recipient, course_id), so a
    /// certificate can be looked up deterministically by its token ID
    /// without an external indexer.
    TokenIndex(u64),
    /// Fix #841: nominated pending admin for the two-step admin transfer.
    PendingAdmin,
    /// Fix #841: ledger timestamp at which the pending admin proposal expires.
    PendingAdminExpiry,
}

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

// ---------------------------------------------------------------------------
// Pending admin transfer (Fix #841)
// ---------------------------------------------------------------------------

pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

pub fn get_pending_admin_expiry(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::PendingAdminExpiry)
}

pub fn set_pending_admin(env: &Env, new_admin: &Address, expiry: u64) {
    env.storage().instance().set(&DataKey::PendingAdmin, new_admin);
    env.storage().instance().set(&DataKey::PendingAdminExpiry, &expiry);
}

pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
    env.storage().instance().remove(&DataKey::PendingAdminExpiry);
}

// ---------------------------------------------------------------------------
// Backend public key (Fix #834: BytesN<32> validated at init)
// ---------------------------------------------------------------------------

/// Stores the active backend signing public key as a fixed-size 32-byte value.
pub fn set_backend_pubkey(env: &Env, pubkey: &BytesN<32>) {
    env.storage().instance().set(&DataKey::BackendPubKey, pubkey);
}

/// Returns the active backend signing public key.
pub fn get_backend_pubkey(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&DataKey::BackendPubKey)
}

// ---------------------------------------------------------------------------
// Pending backend key rotation (Fix #835)
// ---------------------------------------------------------------------------

pub fn set_pending_backend_pubkey(env: &Env, pubkey: &BytesN<32>, expiry: u64) {
    env.storage().instance().set(&DataKey::PendingBackendPubKey, pubkey);
    env.storage().instance().set(&DataKey::PendingBackendPubKeyExpiry, &expiry);
}

pub fn get_pending_backend_pubkey(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&DataKey::PendingBackendPubKey)
}

pub fn get_pending_backend_pubkey_expiry(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::PendingBackendPubKeyExpiry)
}

pub fn clear_pending_backend_pubkey(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingBackendPubKey);
    env.storage().instance().remove(&DataKey::PendingBackendPubKeyExpiry);
}

// ---------------------------------------------------------------------------
// Minter (Fix #691 + #836)
// ---------------------------------------------------------------------------

pub fn get_minter(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Minter)
}

pub fn set_minter(env: &Env, minter: &Address) {
    env.storage().instance().set(&DataKey::Minter, minter);
}

// ---------------------------------------------------------------------------
// Pending minter rotation (Fix #836)
// ---------------------------------------------------------------------------

pub fn set_pending_minter(env: &Env, minter: &Address, expiry: u64) {
    env.storage().instance().set(&DataKey::PendingMinter, minter);
    env.storage().instance().set(&DataKey::PendingMinterExpiry, &expiry);
}

pub fn get_pending_minter(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingMinter)
}

pub fn get_pending_minter_expiry(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::PendingMinterExpiry)
}

pub fn clear_pending_minter(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingMinter);
    env.storage().instance().remove(&DataKey::PendingMinterExpiry);
}

// ---------------------------------------------------------------------------
// Pause
// ---------------------------------------------------------------------------

pub fn get_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

// ---------------------------------------------------------------------------
// Certificates
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Nonces
// ---------------------------------------------------------------------------

pub fn nonce_consumed(env: &Env, nonce: &BytesN<32>) -> bool {
    env.storage().persistent().has(&DataKey::ConsumedNonce(nonce.clone()))
}

pub fn consume_nonce(env: &Env, nonce: &BytesN<32>) {
    let key = DataKey::ConsumedNonce(nonce.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

// ---------------------------------------------------------------------------
// Token ID counter (Fix #628)
// ---------------------------------------------------------------------------

/// Returns the next token ID and increments the counter in persistent storage.
/// Persistent storage survives contract WASM upgrades, preventing duplicate IDs.
pub fn next_token_id(env: &Env) -> u64 {
    let key = DataKey::NextTokenId;
    let id: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    env.storage().persistent().set(&key, &(id + 1));
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    id
}

// ---------------------------------------------------------------------------
// Helper: convert raw Bytes to BytesN<32> for public key validation (#834)
// ---------------------------------------------------------------------------

/// Validates that `raw` is exactly 32 bytes and returns a fixed-size BytesN<32>.
pub fn validate_pubkey(env: &Env, raw: &Bytes) -> Result<BytesN<32>, ContractError> {
    if raw.len() != 32 {
        return Err(ContractError::InvalidPublicKey);
    }
    let mut arr = [0u8; 32];
    raw.copy_into_slice(&mut arr);
    Ok(BytesN::from_array(env, &arr))
}
/// Fix #839: Records (or overwrites) the reverse index entry mapping
/// `token_id` to the `(wallet, course_id)` key of the certificate that
/// owns it. Called whenever a certificate is minted or its owner changes.
pub fn set_token_index(env: &Env, token_id: u64, wallet: &Address, course_id: &BytesN<32>) {
    let key = DataKey::TokenIndex(token_id);
    env.storage().persistent().set(&key, &(wallet.clone(), course_id.clone()));
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

/// Fix #839: Deterministically resolves a `token_id` to its `(wallet,
/// course_id)` certificate key via the reverse index, refreshing the
/// entry's TTL on a hit so actively-queried tokens are not evicted.
pub fn get_token_index(env: &Env, token_id: u64) -> Option<(Address, BytesN<32>)> {
    let key = DataKey::TokenIndex(token_id);
    let entry = env.storage().persistent().get(&key);
    if entry.is_some() {
        env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
    }
    entry
}

/// Fix #839: Removes the reverse index entry for `token_id`. Called on
/// revocation so the index never resolves to a certificate that no longer
/// exists.
pub fn remove_token_index(env: &Env, token_id: u64) {
    env.storage().persistent().remove(&DataKey::TokenIndex(token_id));
}
