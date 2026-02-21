use soroban_sdk::{contract, contractimpl, Env, BytesN};
use crate::admin::require_admin;
use crate::storage::DataKey;
use crate::errors::Error;
mod storage;
use storage::DataKey;
mod signature;
use signature::build_message;
mod errors;
use errors::RewardError;

#[contract]
pub struct RewardContract;

#[contractimpl]
impl RewardContract {

    pub fn set_backend_pubkey(env: Env, pubkey: BytesN<32>) -> Result<(), Error> {
        require_admin(&env)?;

        env.storage()
            .instance()
            .set(&DataKey::BackendPubKey, &pubkey);

        Ok(())
    }

    pub fn rotate_backend_pubkey(env: Env, new_pubkey: BytesN<32>) -> Result<(), Error> {
        require_admin(&env)?;

        env.storage()
            .instance()
            .set(&DataKey::BackendPubKey, &new_pubkey);

        Ok(())
    }

    pub fn get_backend_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage()
            .instance()
            .get(&DataKey::BackendPubKey)
    }
}

use soroban_sdk::{contracttype, BytesN};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    BackendSigner,
    UsedNonce(BytesN<32>),
}