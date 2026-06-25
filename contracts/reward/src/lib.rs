#![no_std]

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

    /// Returns the contract version for post-deploy verification.
    pub fn version(_env: Env) -> u32 {
        1
    }
}
