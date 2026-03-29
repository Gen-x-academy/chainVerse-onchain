use soroban_sdk::{contracttype, Address, Env};
use crate::types::Escrow;

#[contracttype]
pub enum DataKey {
    Escrow(u64),
    EscrowCount,
    ActiveEscrowCount,
    WhitelistedToken(Address),
}

pub fn save_escrow(env: &Env, id: u64, escrow: &Escrow) {
    env.storage().instance().set(&DataKey::Escrow(id), escrow);
}

pub fn load_escrow(env: &Env, id: u64) -> Option<Escrow> {
    env.storage().instance().get(&DataKey::Escrow(id))
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

pub fn increment_active_escrows(env: &Env) {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::ActiveEscrowCount)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::ActiveEscrowCount, &(count + 1));
}

pub fn decrement_active_escrows(env: &Env) {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::ActiveEscrowCount)
        .unwrap_or(0);
    if count > 0 {
        env.storage()
            .instance()
            .set(&DataKey::ActiveEscrowCount, &(count - 1));
    }
}

pub fn get_active_escrow_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::ActiveEscrowCount)
        .unwrap_or(0)
}
