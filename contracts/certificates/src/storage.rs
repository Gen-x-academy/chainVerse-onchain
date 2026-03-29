use soroban_sdk::{contracttype, Address, Env};

use crate::{Certificate, ContractError};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    Certificate(Address, u64),
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();

    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

pub fn require_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        Err(ContractError::ContractPaused)
    } else {
        Ok(())
    }
}

pub fn has_certificate(env: &Env, wallet: &Address, course_id: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Certificate(wallet.clone(), course_id))
}

pub fn load_certificate(env: &Env, wallet: &Address, course_id: u64) -> Option<Certificate> {
    env.storage()
        .persistent()
        .get(&DataKey::Certificate(wallet.clone(), course_id))
}

pub fn save_certificate(env: &Env, wallet: &Address, course_id: u64, certificate: &Certificate) {
    env.storage().persistent().set(
        &DataKey::Certificate(wallet.clone(), course_id),
        certificate,
    );
}
