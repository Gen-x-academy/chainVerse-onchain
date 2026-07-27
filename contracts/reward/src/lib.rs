#![no_std]

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env};

mod admin;
mod errors;
mod events;
mod reward;
mod signature;
mod storage;

#[cfg(test)]
mod test;

use admin::require_admin;
use errors::Error;
use storage::{
    get_penalty_pool, get_token, get_treasury, set_penalty_pool,
    set_reward_amount, set_token, set_treasury, DataKey, MIN_TTL, MAX_TTL,
};

#[contract]
pub struct RewardContract;

#[contractimpl]
impl RewardContract {
    /// One-time initialisation. Sets admin, treasury, token, and reward amount.
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

    pub fn claim_reward(env: Env, user: Address) -> Result<(), Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if storage::is_paused(&env) {
            return Err(Error::ContractPaused);
        }
        reward::claim_reward(env, user)
    }

    /// Accumulate a penalty when a user emergency-unstakes.
    pub fn record_penalty(env: Env, amount: i128) -> Result<(), Error> {
        // #737 — replace panic! with typed error
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
        // #737 — replace panic! with typed error
        if amount == 0 {
            return Err(Error::NoPenaltiesToWithdraw);
        }

        let token_addr = get_token(&env)?;
        let treasury = get_treasury(&env)?;
        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&treasury, &recipient, &amount);

        set_penalty_pool(&env, 0i128);

        env.events().publish(
            (soroban_sdk::symbol_short!("penalties"), soroban_sdk::symbol_short!("withdrawn")),
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
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, true);
        env.events().publish((soroban_sdk::symbol_short!("PAUSED"),), (caller,));
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }
        caller.require_auth();
        storage::set_paused(&env, false);
        env.events().publish((soroban_sdk::symbol_short!("UNPAUSED"),), (caller,));
        Ok(())
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::NotInitialized);
        }
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;
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
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::Unauthorized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }
    Ok(())
}
