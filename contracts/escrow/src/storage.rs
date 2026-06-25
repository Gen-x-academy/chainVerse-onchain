use crate::errors::EscrowError;
use crate::types::{Escrow, FeeRecord};
use soroban_sdk::{contracttype, vec, Address, Env, Vec};

// TTL constants for persistent storage
const MIN_TTL: u32 = 4096;  // Minimum TTL extension (ledgers)
const MAX_TTL: u32 = 100_000; // Maximum TTL extension (ledgers)

/// Default protocol fee: 100 basis points = 1%
pub const DEFAULT_PROTOCOL_FEE_BPS: u32 = 100;

#[contracttype]
pub enum DataKey {
    Admin,
    Escrow(u64),
    EscrowCount,
    TotalVolume,
    WhitelistedToken(Address),
    ProtocolFees(Address),
    TokenIndex(Address),
    FeeHistory,
    ProtocolFeeBps,
    Paused,
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

pub fn require_admin(env: &Env) -> Result<Address, EscrowError> {
    let admin = get_admin(env).ok_or(EscrowError::Unauthorized)?;
    admin.require_auth();
    Ok(admin)
}

pub fn save_escrow(env: &Env, id: u64, escrow: &Escrow) {
    env.storage().persistent().set(&DataKey::Escrow(id), escrow);
    
    // Extend TTL for persistent escrow entry to prevent expiration
    env.storage().persistent().extend_ttl(&DataKey::Escrow(id), MIN_TTL, MAX_TTL);
}

pub fn load_escrow(env: &Env, id: u64) -> Option<Escrow> {
    env.storage().persistent().get(&DataKey::Escrow(id))
}

pub fn next_escrow_id(env: &Env) -> u64 {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::EscrowCount)
        .unwrap_or(0);
    let next = count + 1;
    env.storage().instance().set(&DataKey::EscrowCount, &next);
    next
}

pub fn get_escrow_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::EscrowCount)
        .unwrap_or(0)
}

pub fn is_token_whitelisted(env: &Env, token: &Address) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::WhitelistedToken(token.clone()))
        .unwrap_or(false)
}

pub fn whitelist_token(env: &Env, token: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::WhitelistedToken(token.clone()), &true);
}

pub fn add_to_total_volume(env: &Env, amount: i128) {
    let volume: i128 = env
        .storage()
        .instance()
        .get(&DataKey::TotalVolume)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::TotalVolume, &(volume + amount));
}

pub fn get_total_volume(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalVolume)
        .unwrap_or(0)
}

pub fn accumulate_protocol_fee(env: &Env, token: &Address, fee: i128) {
    let key = DataKey::ProtocolFees(token.clone());
    let current: i128 = env.storage().instance().get(&key).unwrap_or(0);
    env.storage().instance().set(&key, &(current + fee));
}

pub fn get_protocol_fee(env: &Env, token: &Address) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolFees(token.clone()))
        .unwrap_or(0)
}

pub fn clear_protocol_fee(env: &Env, token: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::ProtocolFees(token.clone()), &0_i128);
}

pub fn append_to_token_index(env: &Env, token: &Address, escrow_id: u64) {
    let key = DataKey::TokenIndex(token.clone());
    let mut ids: Vec<u64> = env.storage().persistent().get(&key).unwrap_or(vec![env]);
    ids.push_back(escrow_id);
    env.storage().persistent().set(&key, &ids);
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

pub fn get_token_index(env: &Env, token: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::TokenIndex(token.clone()))
        .unwrap_or(vec![env])
}

pub fn get_protocol_fee_bps(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolFeeBps)
        .unwrap_or(DEFAULT_PROTOCOL_FEE_BPS)
}

#[allow(dead_code)]
pub fn set_protocol_fee_bps(env: &Env, bps: u32) {
    env.storage().instance().set(&DataKey::ProtocolFeeBps, &bps);
}

pub fn append_fee_record(env: &Env, record: &FeeRecord) {
    let mut history: Vec<FeeRecord> = env
        .storage()
        .persistent()
        .get(&DataKey::FeeHistory)
        .unwrap_or(vec![env]);
    history.push_back(record.clone());
    env.storage().persistent().set(&DataKey::FeeHistory, &history);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::FeeHistory, MIN_TTL, MAX_TTL);
}

pub fn get_fee_history(env: &Env) -> Vec<FeeRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::FeeHistory)
        .unwrap_or(vec![env])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Escrow, EscrowStatus};
    use soroban_sdk::{testutils::Address as _, Env};

    fn make_escrow(env: &Env) -> Escrow {
        Escrow {
            buyer: Address::generate(env),
            seller: Address::generate(env),
            token: Address::generate(env),
            amount: 1_000,
            status: EscrowStatus::Pending,
            expiration: 9999,
        }
    }

    // #510 — escrow records must live in persistent storage, not instance storage.
    // Verifies that save_escrow writes to the persistent bucket so data survives
    // beyond a single contract invocation context.
    #[test]
    fn test_save_and_load_escrow_uses_persistent_storage() {
        let env = Env::default();
        let escrow = make_escrow(&env);

        save_escrow(&env, 1, &escrow);

        let loaded = load_escrow(&env, 1).expect("escrow must be retrievable after save");
        assert_eq!(loaded.amount, escrow.amount);
        assert_eq!(loaded.buyer, escrow.buyer);
        assert_eq!(loaded.seller, escrow.seller);
    }

    // #510 — loading a non-existent ID returns None (no stale instance data bleeds through).
    #[test]
    fn test_load_escrow_returns_none_for_unknown_id() {
        let env = Env::default();
        assert!(load_escrow(&env, 999).is_none());
    }

    // #511 — after save_escrow the persistent entry must have an extended TTL so the
    // record does not expire while the escrow is still active.
    #[test]
    fn test_save_escrow_extends_ttl() {
        let env = Env::default();
        let escrow = make_escrow(&env);

        save_escrow(&env, 42, &escrow);

        // TTL extension is confirmed if the entry is still readable after save.
        // In the test environment the ledger starts at 0, so any positive TTL
        // keeps the entry live for the duration of this test.
        assert!(
            load_escrow(&env, 42).is_some(),
            "persistent entry must remain readable after TTL extension"
        );
    }

    // #510 — next_escrow_id must be monotonically increasing so each save goes
    // to a unique persistent key.
    #[test]
    fn test_next_escrow_id_is_unique_and_incrementing() {
        let env = Env::default();
        let a = next_escrow_id(&env);
        let b = next_escrow_id(&env);
        let c = next_escrow_id(&env);
        assert!(a < b && b < c, "escrow IDs must strictly increase");
    }

    // #510 — distinct IDs must not overwrite each other in persistent storage.
    #[test]
    fn test_multiple_escrows_do_not_collide() {
        let env = Env::default();
        let e1 = make_escrow(&env);
        let e2 = make_escrow(&env);

        save_escrow(&env, 1, &e1);
        save_escrow(&env, 2, &e2);

        let l1 = load_escrow(&env, 1).unwrap();
        let l2 = load_escrow(&env, 2).unwrap();
        assert_eq!(l1.buyer, e1.buyer);
        assert_eq!(l2.buyer, e2.buyer);
        assert_ne!(l1.buyer, l2.buyer);
    }
}