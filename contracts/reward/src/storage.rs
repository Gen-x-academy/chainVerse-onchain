use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env};
use crate::errors::Error;

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    Initialized,
    BackendPubKey,
    BackendSigner,
    UsedNonce(BytesN<32>),
    Paused,
    Treasury,
    Token,
    RewardAmount,
    PenaltyPool,
}

const REWARDED: soroban_sdk::Symbol = symbol_short!("REWARDED");

/// TTL threshold / bump used for persistent configuration entries.
pub const MIN_TTL: u32 = 6_307_200;
pub const MAX_TTL: u32 = 12_614_400;

fn bump_persistent<K>(env: &Env, key: &K)
where
    K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_TTL, MAX_TTL);
}

pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

pub fn set_initialized(env: &Env) {
    env.storage()
        .persistent()
        .set(&DataKey::Initialized, &true);
    bump_persistent(env, &DataKey::Initialized);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().persistent().set(&DataKey::Admin, admin);
    bump_persistent(env, &DataKey::Admin);
}

pub fn has_been_rewarded(env: &Env, user: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&(REWARDED, user))
        .unwrap_or(false)
}

pub fn set_rewarded(env: &Env, user: &Address) {
    let key = (REWARDED, user.clone());
    env.storage().persistent().set(&key, &true);
    bump_persistent(env, &key);
}

pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage()
        .persistent()
        .set(&DataKey::Treasury, treasury);
    bump_persistent(env, &DataKey::Treasury);
}

pub fn get_treasury(env: &Env) -> Result<Address, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Treasury)
        .ok_or(Error::NotInitialized)
}

pub fn set_token(env: &Env, token: &Address) {
    env.storage().persistent().set(&DataKey::Token, token);
    bump_persistent(env, &DataKey::Token);
}

pub fn get_token(env: &Env) -> Result<Address, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::Token)
        .ok_or(Error::NotInitialized)
}

pub fn set_reward_amount(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::RewardAmount, &amount);
    bump_persistent(env, &DataKey::RewardAmount);
}

pub fn get_reward_amount(env: &Env) -> Result<i128, Error> {
    env.storage()
        .persistent()
        .get(&DataKey::RewardAmount)
        .ok_or(Error::NotInitialized)
}

pub fn get_penalty_pool(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::PenaltyPool)
        .unwrap_or(0)
}

pub fn set_penalty_pool(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::PenaltyPool, &amount);
    bump_persistent(env, &DataKey::PenaltyPool);
}

pub fn set_backend_pubkey(env: &Env, key: &BytesN<32>) {
    env.storage()
        .persistent()
        .set(&DataKey::BackendPubKey, key);
    bump_persistent(env, &DataKey::BackendPubKey);
}

/// Returns the stored backend public key, or None if not set.
pub fn get_backend_pubkey(env: &Env) -> Option<BytesN<32>> {
    env.storage().persistent().get(&DataKey::BackendPubKey)
}

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage()
        .persistent()
        .set(&DataKey::Paused, &paused);
    bump_persistent(env, &DataKey::Paused);
}
