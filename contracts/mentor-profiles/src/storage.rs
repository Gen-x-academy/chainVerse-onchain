use soroban_sdk::{Address, Bytes, Env};
use crate::types::MentorProfile;
use crate::errors::ContractError;

/// Storage keys for contract state
pub enum DataKey {
    Admin = 0,
    PendingAdmin = 1,
    BackendSigner = 2,
    Profile(Address) = 3,
    RatingCounter = 4,
    ReputationMultiplier = 5,
}

impl DataKey {
    fn discriminant(&self) -> u32 {
        match self {
            DataKey::Admin => 0,
            DataKey::PendingAdmin => 1,
            DataKey::BackendSigner => 2,
            DataKey::Profile(_) => 3,
            DataKey::RatingCounter => 4,
            DataKey::ReputationMultiplier => 5,
        }
    }
}

/// Get the admin address
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

/// Set the admin address
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Require that the caller is the admin
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    match get_admin(env) {
        Some(admin) => {
            if &admin == caller {
                Ok(())
            } else {
                Err(ContractError::UnauthorizedAdmin)
            }
        }
        None => Err(ContractError::UnauthorizedAdmin),
    }
}

/// Get the pending admin (for two-step admin transfer)
pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

/// Set the pending admin
pub fn set_pending_admin(env: &Env, pending_admin: &Address) {
    env.storage().instance().set(&DataKey::PendingAdmin, pending_admin);
}

/// Clear the pending admin
pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
}

/// Get the backend signer address (authorized to submit ratings)
pub fn get_backend_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::BackendSigner)
}

/// Set the backend signer address
pub fn set_backend_signer(env: &Env, signer: &Address) {
    env.storage().instance().set(&DataKey::BackendSigner, signer);
}

/// Get a mentor's profile
pub fn get_profile(env: &Env, mentor_id: &Address) -> Option<MentorProfile> {
    env.storage()
        .instance()
        .get(&DataKey::Profile(mentor_id.clone()))
}

/// Store a mentor's profile
pub fn set_profile(env: &Env, mentor_id: &Address, profile: &MentorProfile) {
    env.storage()
        .instance()
        .set(&DataKey::Profile(mentor_id.clone()), profile);
}

/// Check if a profile exists
pub fn profile_exists(env: &Env, mentor_id: &Address) -> bool {
    env.storage()
        .instance()
        .has(&DataKey::Profile(mentor_id.clone()))
}

/// Get rating counter (for aggregate rating calculations)
pub fn get_rating_counter(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::RatingCounter)
        .unwrap_or(0)
}

/// Increment rating counter
pub fn increment_rating_counter(env: &Env) {
    let counter = get_rating_counter(env);
    env.storage()
        .instance()
        .set(&DataKey::RatingCounter, counter.saturating_add(1));
}

/// Get reputation multiplier (configurable boost factor)
pub fn get_reputation_multiplier(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::ReputationMultiplier)
        .unwrap_or(10) // Default: 10x weight on ratings
}

/// Set reputation multiplier
pub fn set_reputation_multiplier(env: &Env, multiplier: u32) {
    if multiplier == 0 {
        env.storage().instance().remove(&DataKey::ReputationMultiplier);
    } else {
        env.storage()
            .instance()
            .set(&DataKey::ReputationMultiplier, multiplier);
    }
}
