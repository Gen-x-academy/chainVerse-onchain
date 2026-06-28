#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String, Symbol, Vec,
};
mod error;
use error::TokenError;

const CONTRACT_VERSION: &str = "1.0.0";
const DECIMALS: u32 = 7;
const TOTAL_SUPPLY: i128 = 100_000_000 * 10_i128.pow(DECIMALS);
const BALANCE_MIN_TTL: u32 = 100_000;
const BALANCE_MAX_TTL: u32 = 200_000;

#[contracttype]
pub enum DataKey {
    Admin,
    Balance(Address),
    Initialized,
}

#[contract]
pub struct CHVToken;

#[contractimpl]
impl CHVToken {
    pub fn initialize(env: Env, admin: Address, treasury: Address) -> Result<(), TokenError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(TokenError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&(DataKey::Balance(treasury.clone())), &TOTAL_SUPPLY);
        env.events().publish((symbol_short!("INIT"),), (admin, treasury, TOTAL_SUPPLY));
        Ok(())
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        if from == to {
            return Err(TokenError::SelfTransfer);
        }
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        from.require_auth();
        let from_bal: i128 = env.storage().persistent()
            .get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if from_bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        let to_bal: i128 = env.storage().persistent()
            .get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.events().publish((symbol_short!("TRANSFER"),), (from, to, amount));
        Ok(())
    }

    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage().persistent().get(&DataKey::Balance(account)).unwrap_or(0)
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != stored_admin {
            panic!("unauthorized");
        }
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }
}
