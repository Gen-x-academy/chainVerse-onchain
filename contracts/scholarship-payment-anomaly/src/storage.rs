use soroban_sdk::{contracttype, Address, Env, String};

use crate::errors::ContractError;
use crate::types::PaymentAlert;

pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    /// Trusted detector service account.
    Detector,
    /// Trusted resolver (human reviewer who may clear / confirm alerts).
    Resolver,
    /// Per-payment alert record (persistent, keyed by payment_ref).
    Alert(String),
    /// Total alerts raised (persistent counter for audit).
    TotalAlerts,
    /// Total amount currently paused (persistent i128 accumulator).
    TotalPausedAmount,
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

// ── Detector / Resolver ───────────────────────────────────────────────────────

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

pub fn set_resolver(env: &Env, a: &Address) {
    env.storage().instance().set(&DataKey::Resolver, a);
}

pub fn get_resolver(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Resolver)
}

pub fn require_resolver(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_resolver(env) {
        Some(r) if r == *caller => Ok(()),
        Some(_) => Err(ContractError::Unauthorized),
        None => Err(ContractError::NotInitialized),
    }
}

// ── Alert records ─────────────────────────────────────────────────────────────

pub fn save_alert(env: &Env, alert: &PaymentAlert) {
    let key = DataKey::Alert(alert.payment_ref.clone());
    env.storage().persistent().set(&key, alert);
    bump_persistent(env, &key);
}

pub fn get_alert(env: &Env, payment_ref: &String) -> Option<PaymentAlert> {
    let key = DataKey::Alert(payment_ref.clone());
    let v: Option<PaymentAlert> = env.storage().persistent().get(&key);
    if v.is_some() {
        bump_persistent(env, &key);
    }
    v
}

pub fn alert_exists(env: &Env, payment_ref: &String) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Alert(payment_ref.clone()))
}

// ── Global counters ───────────────────────────────────────────────────────────

pub fn increment_total_alerts(env: &Env) {
    let key = DataKey::TotalAlerts;
    let v: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    let new_v = v.saturating_add(1);
    env.storage().persistent().set(&key, &new_v);
    bump_persistent(env, &key);
}

pub fn get_total_alerts(env: &Env) -> u64 {
    let key = DataKey::TotalAlerts;
    let v = env.storage().persistent().get::<_, u64>(&key).unwrap_or(0);
    if v > 0 {
        bump_persistent(env, &key);
    }
    v
}

pub fn add_paused_amount(env: &Env, amount: i128) -> Result<(), ContractError> {
    let key = DataKey::TotalPausedAmount;
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let new_total = current
        .checked_add(amount)
        .ok_or(ContractError::ArithmeticOverflow)?;
    env.storage().persistent().set(&key, &new_total);
    bump_persistent(env, &key);
    Ok(())
}

pub fn sub_paused_amount(env: &Env, amount: i128) -> Result<(), ContractError> {
    let key = DataKey::TotalPausedAmount;
    let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    let new_total = current.checked_sub(amount).unwrap_or(0); // floor at 0
    env.storage().persistent().set(&key, &new_total);
    bump_persistent(env, &key);
    Ok(())
}

pub fn get_total_paused_amount(env: &Env) -> i128 {
    let key = DataKey::TotalPausedAmount;
    env.storage().persistent().get::<_, i128>(&key).unwrap_or(0)
}
