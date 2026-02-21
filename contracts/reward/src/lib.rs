use soroban_sdk::{contract, contractimpl, Env, BytesN};
use crate::admin::require_admin;
use crate::storage::DataKey;
use crate::errors::Error;

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