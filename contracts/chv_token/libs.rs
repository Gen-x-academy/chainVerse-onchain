use soroban_sdk::{contract, contractimpl, Env, Address};

mod admin;
mod error;
mod storage;

use error::TokenError;

#[contract]
pub struct TokenContract;

#[contractimpl]
impl TokenContract {

    pub fn initialize(env: Env, admin: Address) -> Result<(), TokenError> {
        if storage::has_admin(&env) {
            return Err(TokenError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        Ok(())
    }

    pub fn mint(env: Env, to: Address, amount: i128) -> Result<(), TokenError> {
        admin::require_admin(&env)?;

        let balance = storage::balance_of(&env, &to);
        let new_balance = balance.checked_add(amount)
            .expect("Overflow");

        storage::set_balance(&env, &to, &new_balance);
        Ok(())
    }
}
