use crate::errors::EscrowError;
use crate::types::Escrow;
use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Admin,
    Escrow(u64),
    EscrowCount,
    TotalVolume,
    WhitelistedToken(Address),
    ProtocolFees(Address),
    Admin,
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn require_admin(env: &Env) -> Result<Address, EscrowError> {
    let admin = get_admin(env).ok_or(EscrowError::Unauthorized)?;
    admin.require_auth();
    Ok(admin)
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

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}