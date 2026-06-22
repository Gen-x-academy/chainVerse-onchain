use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env};

const REWARDED: soroban_sdk::Symbol = symbol_short!("REWARDED");
const TREASURY: soroban_sdk::Symbol = symbol_short!("TREASURY");
const TOKEN: soroban_sdk::Symbol = symbol_short!("TOKEN");
const REWARD_AMOUNT: soroban_sdk::Symbol = symbol_short!("REWARD_AMT");
const PENALTY_POOL: soroban_sdk::Symbol = symbol_short!("PENALTIES");

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    BackendPubKey,
    UsedNonce(BytesN<32>),
}

pub fn has_been_rewarded(env: &Env, user: &Address) -> bool {
    env.storage().instance().get(&(REWARDED, user)).unwrap_or(false)
}

pub fn set_rewarded(env: &Env, user: &Address) {
    env.storage().instance().set(&(REWARDED, user), &true);
}

pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&TREASURY, treasury);
}

pub fn get_treasury(env: &Env) -> Address {
    env.storage().instance().get(&TREASURY).unwrap()
}

pub fn set_token(env: &Env, token: &Address) {
    env.storage().instance().set(&TOKEN, token);
}

pub fn get_token(env: &Env) -> Address {
    env.storage().instance().get(&TOKEN).unwrap()
}

pub fn set_reward_amount(env: &Env, amount: i128) {
    env.storage().instance().set(&REWARD_AMOUNT, &amount);
}

pub fn get_reward_amount(env: &Env) -> i128 {
    env.storage().instance().get(&REWARD_AMOUNT).unwrap()
}

pub fn set_penalty_pool(env: &Env, amount: i128) {
    env.storage().instance().set(&PENALTY_POOL, &amount);
}

pub fn get_penalty_pool(env: &Env) -> i128 {
    env.storage().instance().get(&PENALTY_POOL).unwrap_or(0)
}
