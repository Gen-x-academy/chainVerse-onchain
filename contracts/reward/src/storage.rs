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
    /// Per-student claim flag — must be persistent so it survives WASM upgrades (#299).
    Rewarded(Address),
}

const TREASURY: soroban_sdk::Symbol = symbol_short!("TREASURY");
const TOKEN: soroban_sdk::Symbol = symbol_short!("TOKEN");
const REWARD_AMOUNT: soroban_sdk::Symbol = symbol_short!("REWARD_AMT");
const PENALTY_POOL: soroban_sdk::Symbol = symbol_short!("PENALTIES");

/// TTL threshold / bump for persistent entries (ledgers).
pub const MIN_TTL: u32 = 6_307_200;
pub const MAX_TTL: u32 = 12_614_400;

pub fn is_initialized(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Initialized).unwrap_or(false)
}

pub fn set_initialized(env: &Env) {
    env.storage().instance().set(&DataKey::Initialized, &true);
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn has_been_rewarded(env: &Env, student: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Rewarded(student.clone()))
        .unwrap_or(false)
}

pub fn set_rewarded_flag(env: &Env, student: &Address, value: bool) {
    let key = DataKey::Rewarded(student.clone());
    env.storage().persistent().set(&key, &value);
    env.storage()
        .persistent()
        .extend_ttl(&key, MIN_TTL, MAX_TTL);
}

/// Marks a student as rewarded (claim completed).
pub fn set_rewarded(env: &Env, student: &Address) {
    set_rewarded_flag(env, student, true);
}

pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&TREASURY, treasury);
}

pub fn get_treasury(env: &Env) -> Result<Address, Error> {
    env.storage().instance().get(&TREASURY).ok_or(Error::NotInitialized)
}

pub fn set_token(env: &Env, token: &Address) {
    env.storage().instance().set(&TOKEN, token);
}

pub fn get_token(env: &Env) -> Result<Address, Error> {
    env.storage().instance().get(&TOKEN).ok_or(Error::NotInitialized)
}

pub fn set_reward_amount(env: &Env, amount: i128) {
    env.storage().instance().set(&REWARD_AMOUNT, &amount);
}

pub fn get_reward_amount(env: &Env) -> Result<i128, Error> {
    env.storage().instance().get(&REWARD_AMOUNT).ok_or(Error::NotInitialized)
}

pub fn get_penalty_pool(env: &Env) -> i128 {
    env.storage().instance().get(&PENALTY_POOL).unwrap_or(0)
}

pub fn set_penalty_pool(env: &Env, amount: i128) {
    env.storage().instance().set(&PENALTY_POOL, &amount);
}

pub fn set_backend_pubkey(env: &Env, key: &BytesN<32>) {
    env.storage().instance().set(&DataKey::BackendPubKey, key);
}

/// Returns the stored backend public key, or None if not initialized.
pub fn get_backend_pubkey(env: &Env) -> Option<BytesN<32>> {
    env.storage().instance().get(&DataKey::BackendPubKey)
}

pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}
