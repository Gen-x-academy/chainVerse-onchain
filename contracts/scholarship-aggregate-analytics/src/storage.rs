use soroban_sdk::{contracttype, Address, Env, String};

use crate::errors::ContractError;
use crate::types::{CohortMetrics, MetricDefinition};

pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    /// Recorder is the only address permitted to write metrics.
    Recorder,
    /// K-anonymity suppression threshold (instance).
    KThreshold,
    /// Per-cohort aggregate metrics (persistent).
    CohortMetrics(String),
    /// Per-metric definition record (persistent).
    MetricDefinition(String),
}

// ── TTL ───────────────────────────────────────────────────────────────────────

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

// ── Recorder ─────────────────────────────────────────────────────────────────

pub fn get_recorder(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Recorder)
}

pub fn set_recorder(env: &Env, a: &Address) {
    env.storage().instance().set(&DataKey::Recorder, a);
}

pub fn require_recorder(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_recorder(env) {
        Some(r) if r == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

// ── K-threshold ───────────────────────────────────────────────────────────────

pub fn get_k_threshold(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::KThreshold)
        .unwrap_or(10)
}

pub fn set_k_threshold(env: &Env, k: u64) {
    env.storage().instance().set(&DataKey::KThreshold, &k);
}

// ── CohortMetrics ─────────────────────────────────────────────────────────────

pub fn get_cohort(env: &Env, tag: &String) -> Option<CohortMetrics> {
    let key = DataKey::CohortMetrics(tag.clone());
    let v: Option<CohortMetrics> = env.storage().persistent().get(&key);
    if v.is_some() {
        bump_persistent(env, &key);
    }
    v
}

pub fn save_cohort(env: &Env, cm: &CohortMetrics) {
    let key = DataKey::CohortMetrics(cm.cohort_tag.clone());
    env.storage().persistent().set(&key, cm);
    bump_persistent(env, &key);
}

pub fn cohort_exists(env: &Env, tag: &String) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::CohortMetrics(tag.clone()))
}

// ── MetricDefinition ─────────────────────────────────────────────────────────

pub fn get_metric_definition(env: &Env, key_str: &String) -> Option<MetricDefinition> {
    let key = DataKey::MetricDefinition(key_str.clone());
    let v: Option<MetricDefinition> = env.storage().persistent().get(&key);
    if v.is_some() {
        bump_persistent(env, &key);
    }
    v
}

pub fn save_metric_definition(env: &Env, md: &MetricDefinition) {
    let key = DataKey::MetricDefinition(md.key.clone());
    env.storage().persistent().set(&key, md);
    bump_persistent(env, &key);
}
