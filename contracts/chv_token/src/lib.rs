#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol, Vec, symbol_short
};

mod error;
use error::TokenError;

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

    /// Initialize contract and mint fixed supply to treasury
    pub fn initialize(env: Env, admin: Address) -> Result<(), TokenError> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(TokenError::AlreadyInitialized);
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);

        // Mint entire supply to admin (treasury) using persistent storage
        env.storage()
            .persistent()
            .set(&DataKey::Balance(admin.clone()), &TOTAL_SUPPLY);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Balance(admin.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);

        env.storage().instance().set(&DataKey::Initialized, &true);

        Ok(())
    }

    /// Get total supply
    pub fn total_supply(_env: Env) -> i128 {
        TOTAL_SUPPLY
    }

    /// Get token decimals
    pub fn decimals(_env: Env) -> u32 {
        DECIMALS
    }

    /// Get balance of address
    pub fn balance(env: Env, addr: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(addr))
            .unwrap_or(0)
    }

    /// Transfer tokens
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        if amount <= 0 {
            panic!("Invalid amount");
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Balance(from), BALANCE_MIN_TTL, BALANCE_MAX_TTL);

        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Balance(to), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
    }
}
