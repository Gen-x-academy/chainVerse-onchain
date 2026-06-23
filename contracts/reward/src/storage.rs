use soroban_sdk::{contracttype, Env, Address, BytesN, symbol_short};
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
}

const REWARDED: soroban_sdk::Symbol = symbol_short!("REWARDED");
const TREASURY: soroban_sdk::Symbol = symbol_short!("TREASURY");
const TOKEN: soroban_sdk::Symbol = symbol_short!("TOKEN");
const REWARD_AMOUNT: soroban_sdk::Symbol = symbol_short!("REWARD_AMT");

// ~1 year in ledgers (5-second close time): fix #371 — persistent storage prevents
// reward flag reset on contract upgrade; fix #368 — TTL extended to prevent expiry.
const MIN_TTL: u32 = 6_307_200;  // ~1 year
const MAX_TTL: u32 = 12_614_400; // ~2 years

pub fn has_been_rewarded(env: &Env, user: &Address) -> bool {
    env.storage().persistent().get(&(REWARDED, user)).unwrap_or(false)
}

pub fn set_rewarded(env: &Env, user: &Address) {
    let key = (REWARDED, user.clone());
    env.storage().persistent().set(&key, &true);
    env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);
}

pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&TREASURY, treasury);
}

/// Returns `Error::NotInitialized` instead of panicking if treasury has not been set.
pub fn get_treasury(env: &Env) -> Result<Address, Error> {
    env.storage().instance().get(&TREASURY).ok_or(Error::NotInitialized)
}

pub fn set_token(env: &Env, token: &Address) {
    env.storage().instance().set(&TOKEN, token);
}

/// Returns `Error::NotInitialized` instead of panicking if token has not been set.
pub fn get_token(env: &Env) -> Result<Address, Error> {
    env.storage().instance().get(&TOKEN).ok_or(Error::NotInitialized)
}

pub fn set_reward_amount(env: &Env, amount: i128) {
    env.storage().instance().set(&REWARD_AMOUNT, &amount);
}

/// Returns `Error::NotInitialized` instead of panicking if reward amount has not been set.
pub fn get_reward_amount(env: &Env) -> Result<i128, Error> {
    env.storage().instance().get(&REWARD_AMOUNT).ok_or(Error::NotInitialized)
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