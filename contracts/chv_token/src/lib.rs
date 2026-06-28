#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env,
};
mod error;
use error::TokenError;

const DECIMALS: u32 = 7;
/// Fix #630: Hard cap — 1 billion CHV tokens. Enforced in mint(); cannot be changed post-deploy.
const MAX_SUPPLY: i128 = 1_000_000_000 * 10_i128.pow(DECIMALS);
const BALANCE_MIN_TTL: u32 = 100_000;
const BALANCE_MAX_TTL: u32 = 200_000;

#[contracttype]
pub enum DataKey {
    Admin,
    Balance(Address),
    Initialized,
    TotalMinted,
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
        let initial_supply: i128 = 100_000_000 * 10_i128.pow(DECIMALS);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&(DataKey::Balance(treasury.clone())), &initial_supply);
        env.storage().instance().set(&DataKey::TotalMinted, &initial_supply);
        env.events().publish((symbol_short!("INIT"),), (admin, treasury, initial_supply));
        Ok(())
    }

    /// Fix #630: Mints new CHV tokens to `to`, enforcing the MAX_SUPPLY hard cap.
    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let admin: Address = env.storage().instance().get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;
        admin.require_auth();
        let total_minted: i128 = env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0);
        if total_minted + amount > MAX_SUPPLY {
            return Err(TokenError::SupplyCapExceeded);
        }
        let balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(balance + amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.storage().instance().set(&DataKey::TotalMinted, &(total_minted + amount));
        env.events().publish((symbol_short!("MINT"),), (to, amount));
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
        env.storage().persistent().extend_ttl(&DataKey::Balance(from.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.storage().persistent().extend_ttl(&DataKey::Balance(to.clone()), BALANCE_MIN_TTL, BALANCE_MAX_TTL);
        env.events().publish((symbol_short!("TRANSFER"),), (from, to, amount));
        Ok(())
    }

    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), TokenError> {
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        from.require_auth();
        let bal: i128 = env.storage().persistent()
            .get(&DataKey::Balance(from.clone())).unwrap_or(0);
        if bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(bal - amount));
        env.events().publish((symbol_short!("BURN"),), (from, amount));
        Ok(())
    }

    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage().persistent().get(&DataKey::Balance(account)).unwrap_or(0)
    }

    pub fn total_minted(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalMinted).unwrap_or(0)
    }
}
