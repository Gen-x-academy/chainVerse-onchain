use soroban_sdk::{Env, Address};
use crate::storage::DataKey;
use crate::errors::ContractError;

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn require_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        return Err(ContractError::ContractPaused);
    }
    Ok(())
}

pub fn set_pause(env: &Env, admin: Address, paused: bool) -> Result<(), ContractError> {
    let stored_admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap();

    if admin != stored_admin {
        return Err(ContractError::Unauthorized);
    }

    admin.require_auth();

    env.storage()
        .instance()
        .set(&DataKey::Paused, &paused);

    Ok(())
}