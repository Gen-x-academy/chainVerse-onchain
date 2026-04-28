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
}

const REWARDED: soroban_sdk::Symbol = symbol_short!("REWARDED");
const TREASURY: soroban_sdk::Symbol = symbol_short!("TREASURY");
const TOKEN: soroban_sdk::Symbol = symbol_short!("TOKEN");
const REWARD_AMOUNT: soroban_sdk::Symbol = symbol_short!("REWARD_AMT");

pub fn has_been_rewarded(env: &Env, user: &Address) -> bool {
    env.storage().persistent().get(&(REWARDED, user)).unwrap_or(false)
}

pub fn set_rewarded(env: &Env, user: &Address) {
    env.storage().persistent().set(&(REWARDED, user), &true);
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