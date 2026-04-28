#![no_std]

use soroban_sdk::{contract, contractimpl, Env, BytesN, Address};

mod storage;
mod signature;
mod errors;
mod reward;
mod events;
mod admin;
mod crypto;

use storage::{set_treasury, set_token, set_reward_amount, DataKey};
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
}
