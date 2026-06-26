#![no_std]

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env};

use crate::admin::require_admin;
use crate::errors::Error;
use crate::storage::{
    get_penalty_pool, get_token, get_treasury, set_penalty_pool, DataKey,
};

mod admin;
mod errors;
mod events;
mod reward;
mod signature;
mod storage;

#[cfg(test)]
mod test;
use soroban_sdk::{contract, contractimpl, Env, BytesN, Address};

mod storage;
mod signature;
mod errors;
mod reward;
mod events;
mod admin;
mod crypto;

#[cfg(test)]
mod test;

use storage::{set_treasury, set_token, set_reward_amount, MIN_TTL, MAX_TTL};
use crate::storage::DataKey;
use admin::require_admin;
use errors::Error;

#[contract]
pub struct RewardContract;

#[contractimpl]
impl RewardContract {
    pub fn initialize(env: Env, admin: Address, treasury: Address, token: Address, reward_amount: i128) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        storage::set_treasury(&env, &treasury);
        storage::set_token(&env, &token);
        storage::set_reward_amount(&env, reward_amount);
        storage::set_penalty_pool(&env, 0i128);
    }

    pub fn set_backend_pubkey(env: Env, pubkey: BytesN<32>) -> Result<(), Error> {
        require_admin(&env)?;
        env.storage().instance().set(&DataKey::BackendPubKey, &pubkey);
    /// One-time initialisation. Sets admin, treasury, token, and reward amount.
    /// Reverts if already initialised.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        token: Address,
        reward_amount: i128,
    ) -> Result<(), Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        set_treasury(&env, &treasury);
        set_token(&env, &token);
        set_reward_amount(&env, reward_amount);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    pub fn rotate_backend_pubkey(env: Env, new_pubkey: BytesN<32>) -> Result<(), Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::NotInitialized);
        }
        require_admin(&env)?;
        env.storage().instance().set(&DataKey::BackendPubKey, &new_pubkey);
        Ok(())
    }

    pub fn get_backend_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::BackendPubKey)
    }

    pub fn claim_reward(env: Env, user: Address) -> Result<(), errors::Error> {
        reward::claim_reward(env, user)
    }

    /// Accumulate a penalty when a user emergency-unstakes.
    ///
    /// Called internally by the staking logic; the penalty amount is credited
    /// to the on-chain penalty pool so it can later be recovered by the admin.
    pub fn record_penalty(env: Env, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            panic!("amount must be positive");
        }
        let current = get_penalty_pool(&env);
        set_penalty_pool(&env, current + amount);
        Ok(())
    }

    /// Withdraw accumulated emergency-unstake penalties to `recipient` (admin only).
    ///
    /// Transfers the full penalty pool balance from the treasury to `recipient`
    /// and resets the pool to zero.  Panics if the pool is empty.
    pub fn withdraw_penalties(
        env: Env,
        admin: Address,
        recipient: Address,
    ) -> Result<(), Error> {
        admin.require_auth();
        require_admin_internal(&env, &admin)?;

        let amount = get_penalty_pool(&env);
        if amount == 0 {
            panic!("no penalties to withdraw");
        }

        let token_addr = get_token(&env);
        let treasury = get_treasury(&env);

        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&treasury, &recipient, &amount);

        set_penalty_pool(&env, 0i128);

        env.events().publish(
            (soroban_sdk::symbol_short!("penalties"), soroban_sdk::symbol_short!("withdrawn")),
            (recipient, amount),
        );

        Ok(())
    }

    /// Return the current accumulated penalty pool balance.
    pub fn get_penalty_pool(env: Env) -> i128 {
        get_penalty_pool(&env)
        Ok(())
    }

    pub fn get_backend_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        env.storage().instance().get(&DataKey::BackendPubKey)
    }

    pub fn claim_reward(env: Env, user: Address) -> Result<(), errors::Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::is_paused(&env) {
            return Err(errors::Error::ContractPaused);
        }
        reward::claim_reward(env, user)
    }

fn require_admin_internal(env: &Env, caller: &Address) -> Result<(), Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::Unauthorized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }
    Ok(())
    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        storage::is_paused(&env)
    }

    /// Admin-only: pause the contract.
    pub fn pause(env: Env, caller: Address) -> Result<(), errors::Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(errors::Error::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(errors::Error::Unauthorized)?;
        if caller != admin {
            return Err(errors::Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, true);
        env.events()
            .publish((soroban_sdk::symbol_short!("PAUSED"),), (caller,));
        Ok(())
    }

    /// Admin-only: unpause the contract.
    pub fn unpause(env: Env, caller: Address) -> Result<(), errors::Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(errors::Error::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(errors::Error::Unauthorized)?;
        if caller != admin {
            return Err(errors::Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, false);
        env.events()
            .publish((soroban_sdk::symbol_short!("UNPAUSED"),), (caller,));
        Ok(())
    }
}
