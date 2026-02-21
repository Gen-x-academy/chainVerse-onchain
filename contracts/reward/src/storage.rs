use soroban_sdk::{Env, Address, symbol_short};

const REWARDED: soroban_sdk::Symbol = symbol_short!("REWARDED");
const TREASURY: soroban_sdk::Symbol = symbol_short!("TREASURY");
const TOKEN: soroban_sdk::Symbol = symbol_short!("TOKEN");
const REWARD_AMOUNT: soroban_sdk::Symbol = symbol_short!("REWARD_AMT");

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