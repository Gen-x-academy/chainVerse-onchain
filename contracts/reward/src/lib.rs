#![no_std]

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env};

mod admin;
mod eligibility;
mod errors;
mod events;
mod reward;
mod signature;
mod storage;

#[cfg(test)]
mod test;

#[cfg(test)]
mod reward_test;
pub use errors::Error;

use admin::require_admin;
use storage::{
    get_admin, get_backend_pubkey, get_penalty_pool, get_token, get_treasury, is_initialized,
    set_admin, set_backend_pubkey, set_initialized, set_penalty_pool, set_reward_amount, set_token,
    set_treasury, MAX_TTL, MIN_TTL,
};

#[contract]
pub struct RewardContract;

#[contractimpl]
impl RewardContract {
    /// One-time initialisation. Sets admin, treasury, token, and reward amount.
    /// Configuration is written to persistent storage so it survives WASM upgrades.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        token: Address,
        reward_amount: i128,
    ) -> Result<(), Error> {
        if is_initialized(&env) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        set_treasury(&env, &treasury);
        set_token(&env, &token);
        set_reward_amount(&env, reward_amount);
        set_initialized(&env);
        Ok(())
    }

    pub fn rotate_backend_pubkey(env: Env, new_pubkey: BytesN<32>) -> Result<(), Error> {
        if !is_initialized(&env) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env)?;
        set_backend_pubkey(&env, &new_pubkey);
        Ok(())
    }

    pub fn get_backend_pubkey(env: Env) -> Option<BytesN<32>> {
        get_backend_pubkey(&env)
    }

    /// Returns the configured treasury address (persistent — survives upgrades).
    pub fn get_treasury(env: Env) -> Result<Address, Error> {
        get_treasury(&env)
    }

    /// Returns the configured reward token address.
    pub fn get_token(env: Env) -> Result<Address, Error> {
        get_token(&env)
    }

    pub fn claim_reward(env: Env, user: Address) -> Result<(), Error> {
        // Keep instance TTL warm for any residual instance data; config lives in persistent.
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        reward::claim_reward(env, user)
    }

    /// Admin-only: update the per-student reward amount without redeploying.
    pub fn set_reward_amount(env: Env, new_amount: i128) -> Result<(), errors::Error> {
        reward::update_reward_amount(env, new_amount)
    }

    /// Returns the current per-student reward amount.
    pub fn get_reward_amount(env: Env) -> Result<i128, errors::Error> {
        reward::current_reward_amount(env)
    }

    /// Admin-only: distribute rewards to many students in a single transaction.
    pub fn batch_claim_reward(
        env: Env,
        recipients: soroban_sdk::Vec<Address>,
    ) -> Result<(), errors::Error> {
        reward::batch_claim_reward(env, recipients)
    }

    /// Accumulate a penalty when a user emergency-unstakes.
    pub fn record_penalty(env: Env, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let current = get_penalty_pool(&env);
        set_penalty_pool(&env, current + amount);
        Ok(())
    }

    /// Withdraw accumulated emergency-unstake penalties to `recipient` (admin only).
    pub fn withdraw_penalties(
        env: Env,
        admin: Address,
        recipient: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        require_admin_by_caller(&env, &admin)?;

        let amount = get_penalty_pool(&env);
        if amount == 0 {
            return Err(Error::NoPenaltiesToWithdraw);
        }

        let token_addr = get_token(&env)?;
        let treasury = get_treasury(&env)?;
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&treasury, &recipient, &amount);

        set_penalty_pool(&env, 0i128);

        env.events().publish(
            (
                soroban_sdk::symbol_short!("penalties"),
                soroban_sdk::symbol_short!("withdrawn"),
            ),
            (recipient, amount),
        );
        Ok(())
    }

    pub fn get_penalty_pool(env: Env) -> i128 {
        get_penalty_pool(&env)
    }

    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    pub fn pause(env: Env, caller: Address) -> Result<(), Error> {
        if !is_initialized(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = get_admin(&env).ok_or(Error::Unauthorized)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, true);
        env.events()
            .publish((soroban_sdk::symbol_short!("PAUSED"),), (caller,));
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        if !is_initialized(&env) {
            return Err(Error::NotInitialized);
        }
        let admin = get_admin(&env).ok_or(Error::Unauthorized)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, false);
        env.events()
            .publish((soroban_sdk::symbol_short!("UNPAUSED"),), (caller,));
        Ok(())
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        if !is_initialized(&env) {
            return Err(Error::NotInitialized);
        }
        let stored_admin = get_admin(&env).ok_or(Error::Unauthorized)?;
        if stored_admin != admin {
            return Err(Error::Unauthorized);
        }
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        1
    }
}

fn require_admin_by_caller(env: &Env, caller: &Address) -> Result<(), Error> {
    let admin = get_admin(env).ok_or(Error::Unauthorized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
