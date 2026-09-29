use soroban_sdk::{contracttype, Address, Env};

use crate::errors::ContractError;
use crate::types::RuleVersion;

// ── TTL constants ─────────────────────────────────────────────────────────────
// ~1 year and ~2 years at 5 s/ledger — matches certificates contract policy.
pub const MIN_TTL: u32 = 3_110_400;
pub const MAX_TTL: u32 = 6_220_800;

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Sole admin address (instance storage).
    Admin,
    /// Pause flag (instance storage).
    Paused,
    /// Pending admin proposal during two-step handoff (instance storage).
    PendingAdmin,
    /// Expiry ledger-timestamp for pending admin (instance storage).
    PendingAdminExpiry,
    /// A specific rule version record (persistent, keyed by version number).
    RuleVersion(u64),
    /// Monotonically increasing version counter (persistent).
    NextVersion,
    /// Currently active version number; 0 = none (persistent).
    ActiveVersion,
    /// Reviewer address allowed to record outcomes (instance storage).
    Reviewer,
    /// Second-approver address for high-risk changes (instance storage).
    HighRiskApprover,
}

// ── TTL helpers ───────────────────────────────────────────────────────────────

/// Bump instance-storage TTL.  Call at the start of every entry-point.
pub fn bump_instance(env: &Env) {
    env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
}

/// Bump a persistent key's TTL after every read or write.
fn bump_persistent<K: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: &K) {
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_TTL, MAX_TTL);
}

// ── Admin ─────────────────────────────────────────────────────────────────────

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Verify `caller` is the admin.  Panics via `require_auth` if the Soroban
/// auth context is not satisfied, then returns `Unauthorized` if the
/// address doesn't match the stored admin.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
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

// ── Pending admin handoff ─────────────────────────────────────────────────────

pub fn set_pending_admin(env: &Env, addr: &Address, expiry: u64) {
    env.storage().instance().set(&DataKey::PendingAdmin, addr);
    env.storage()
        .instance()
        .set(&DataKey::PendingAdminExpiry, &expiry);
}

pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

pub fn get_pending_admin_expiry(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get::<_, u64>(&DataKey::PendingAdminExpiry)
        .unwrap_or(0)
}

pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
    env.storage()
        .instance()
        .remove(&DataKey::PendingAdminExpiry);
}

// ── Reviewer / high-risk approver ─────────────────────────────────────────────

pub fn set_reviewer(env: &Env, addr: &Address) {
    env.storage().instance().set(&DataKey::Reviewer, addr);
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

pub fn set_high_risk_approver(env: &Env, addr: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::HighRiskApprover, addr);
}

pub fn get_high_risk_approver(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::HighRiskApprover)
}

// ── Version counter ───────────────────────────────────────────────────────────

pub fn next_version(env: &Env) -> u64 {
    let key = DataKey::NextVersion;
    let v: u64 = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(0u64)
        .checked_add(1)
        .expect("version overflow"); // infeasible in practice
    env.storage().persistent().set(&key, &v);
    bump_persistent(env, &key);
    v
}

pub fn peek_next_version(env: &Env) -> u64 {
    let key = DataKey::NextVersion;
    env.storage()
        .persistent()
        .get::<_, u64>(&key)
        .unwrap_or(0)
        .saturating_add(1)
}

// ── Active version ────────────────────────────────────────────────────────────

pub fn get_active_version(env: &Env) -> u64 {
    let key = DataKey::ActiveVersion;
    let v = env
        .storage()
        .persistent()
        .get::<_, u64>(&key)
        .unwrap_or(0);
    if v > 0 {
        bump_persistent(env, &key);
    }
    v
}

pub fn set_active_version(env: &Env, version: u64) {
    let key = DataKey::ActiveVersion;
    env.storage().persistent().set(&key, &version);
    bump_persistent(env, &key);
}

// ── Rule version records ──────────────────────────────────────────────────────

pub fn save_rule_version(env: &Env, rv: &RuleVersion) {
    let key = DataKey::RuleVersion(rv.version);
    env.storage().persistent().set(&key, rv);
    bump_persistent(env, &key);
}

pub fn get_rule_version(env: &Env, version: u64) -> Option<RuleVersion> {
    let key = DataKey::RuleVersion(version);
    let rv: Option<RuleVersion> = env.storage().persistent().get(&key);
    if rv.is_some() {
        bump_persistent(env, &key);
    }
    rv
}

pub fn rule_version_exists(env: &Env, version: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::RuleVersion(version))
}
