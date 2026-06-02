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
        env.storage().persistent().extend_ttl(
            &DataKey::Balance(admin.clone()),
            BALANCE_MIN_TTL,
            BALANCE_MAX_TTL,
        );

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
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), TokenError> {
        from.require_auth();

        if from == to {
            return Err(TokenError::SelfTransfer);
        }

        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(TokenError::InsufficientBalance);
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage().persistent().extend_ttl(
            &DataKey::Balance(from),
            BALANCE_MIN_TTL,
            BALANCE_MAX_TTL,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));
        env.storage().persistent().extend_ttl(
            &DataKey::Balance(to),
            BALANCE_MIN_TTL,
            BALANCE_MAX_TTL,
        );

        Ok(())
    }

    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), TokenError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(TokenError::NotInitialized)?;

        if stored_admin != admin {
            return Err(TokenError::Unauthorized);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_transfer_rejects_invalid_amount() {
        let env = Env::default();
        env.mock_all_auths();

        let from = Address::generate(&env);
        let to = Address::generate(&env);

        let result = CHVToken::transfer(env, from, to, 0);
        assert_eq!(result, Err(TokenError::InvalidAmount));
    }
}
