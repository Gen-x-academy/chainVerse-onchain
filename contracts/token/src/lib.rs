#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, Env, Symbol
};

#[contract]
pub struct TokenContract;

#[contracttype]
enum DataKey {
    Balance(Address),
    TotalSupply,
    Initialized,
}

#[contractimpl]
impl TokenContract {

    pub fn initialize(env: Env, admin: Address, total_supply: i128) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::TotalSupply, &total_supply);
        env.storage().instance().set(&DataKey::Balance(admin.clone()), &total_supply);
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    pub fn total_supply(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap()
    }

    pub fn balance(env: Env, user: Address) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Balance(user))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("insufficient balance");
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .instance()
            .set(&DataKey::Balance(from), &(from_balance - amount));

        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(to_balance + amount));
    }
}