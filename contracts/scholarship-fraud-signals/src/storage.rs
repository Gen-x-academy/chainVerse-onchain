use soroban_sdk::{contracttype, Address, Env, String};

use crate::errors::ContractError;
use crate::types::FraudSignal;

pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    /// The trusted detector service account.
    Detector,
    /// The trusted human reviewer account.
    Reviewer,
    /// Per-application fraud signal record (persistent).
    Signal(String),
}

pub fn bump_instance(env: &Env) {
    env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
}

fn bump_persistent<K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &K) {
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_TTL, MAX_TTL);
}

// ── Admin ─────────────────────────────────────────────────────────────────────

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, a: &Address) {
    env.storage().instance().set(&DataKey::Admin, a);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_admin(env) {
        Some(a) if a == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

// ── Pause ─────────────────────────────────────────────────────────────────────

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get::<_, bool>(&DataKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(env: &Env, v: bool) {
    env.storage().instance().set(&DataKey::Paused, &v);
}

pub fn assert_not_paused(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        Err(ContractError::ContractPaused)
    } else {
        Ok(())
    }
}

// ── Detector / Reviewer ───────────────────────────────────────────────────────

pub fn set_detector(env: &Env, a: &Address) {
    env.storage().instance().set(&DataKey::Detector, a);
}

pub fn get_detector(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Detector)
}

pub fn require_detector(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_detector(env) {
        Some(d) if d == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

pub fn set_reviewer(env: &Env, a: &Address) {
    env.storage().instance().set(&DataKey::Reviewer, a);
}

pub fn get_reviewer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Reviewer)
}

pub fn require_reviewer(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_reviewer(env) {
        Some(r) if r == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

// ── Signal records ────────────────────────────────────────────────────────────

pub fn save_signal(env: &Env, sig: &FraudSignal) {
    let key = DataKey::Signal(sig.app_id.clone());
    env.storage().persistent().set(&key, sig);
    bump_persistent(env, &key);
}

pub fn get_signal(env: &Env, app_id: &String) -> Option<FraudSignal> {
    let key = DataKey::Signal(app_id.clone());
    let v: Option<FraudSignal> = env.storage().persistent().get(&key);
    if v.is_some() {
        bump_persistent(env, &key);
    }
    v
}

pub fn signal_exists(env: &Env, app_id: &String) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Signal(app_id.clone()))
}
