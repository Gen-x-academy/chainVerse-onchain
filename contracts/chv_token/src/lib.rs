#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Env, Symbol, Vec, symbol_short
};

const DECIMALS: u32 = 7;
const TOTAL_SUPPLY: i128 = 100_000_000 * 10_i128.pow(DECIMALS);

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
    pub fn initialize(env: Env, admin: Address) {
        // Prevent re-initialization
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("Already initialized");
        }

        admin.require_auth();

        // Set admin
        env.storage().instance().set(&DataKey::Admin, &admin);

        // Mint entire supply to admin (treasury)
        env.storage()
            .instance()
            .set(&DataKey::Balance(admin.clone()), &TOTAL_SUPPLY);

        // Mark as initialized
        env.storage().instance().set(&DataKey::Initialized, &true);
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
            .instance()
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
            .instance()
            .set(&DataKey::Balance(from), &(from_balance - amount));

        env.storage()
            .instance()
            .set(&DataKey::Balance(to), &(to_balance + amount));
    }
}