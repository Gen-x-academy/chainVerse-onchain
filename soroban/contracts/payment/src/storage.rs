//! Storage helpers for the ChainVerse payment contract.
//!
//! Follows the TTL policy from ADR-001:
//! - Instance storage: `Admin`, `Treasury`, `FeePercent`, `RefundWindowSeconds`.
//! - Persistent storage: `AssetConfig`, `CourseConfig`, `Enrollment`,
//!   `PaymentRecord`, `InstructorBalance`.
//!
//! Every persistent write calls `bump_ttl` to ensure the entry survives at
//! least `MIN_TTL` ledgers and at most `MAX_TTL` ledgers.
use soroban_sdk::{Address, Env, Symbol};

use chainverse_types::{AssetConfig, CourseConfig, DataKey, MAX_TTL, MIN_TTL};

use crate::errors::ContractError;

// ─── TTL helper ──────────────────────────────────────────────────────────────

/// Extend the lifetime of a persistent storage entry to at most `MAX_TTL`
/// ledgers, ensuring at least `MIN_TTL` ledgers remain.
fn bump_persistent(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(key, MIN_TTL, MAX_TTL);
}

// ─── Initialisation guard ────────────────────────────────────────────────────

/// Returns `true` if the contract has been initialised (i.e. `Admin` exists).
pub fn is_initialized(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Admin)
}

// ─── Admin ───────────────────────────────────────────────────────────────────

/// Persist the administrator address in instance storage.
pub fn write_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Read the administrator address.
///
/// # Errors
/// [`ContractError::NotInitialized`] if the contract has not been initialised.
pub fn read_admin(env: &Env) -> Result<Address, ContractError> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(ContractError::NotInitialized)
}

// ─── Treasury ────────────────────────────────────────────────────────────────

/// Persist the treasury address in instance storage.
pub fn write_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&DataKey::Treasury, treasury);
}

/// Read the treasury address.
///
/// # Errors
/// [`ContractError::NotInitialized`] if the contract has not been initialised.
pub fn read_treasury(env: &Env) -> Result<Address, ContractError> {
    env.storage()
        .instance()
        .get(&DataKey::Treasury)
        .ok_or(ContractError::NotInitialized)
}

// ─── Platform fee ────────────────────────────────────────────────────────────

/// Persist the platform fee (in basis points) in instance storage.
pub fn write_fee(env: &Env, fee_bps: u32) {
    env.storage().instance().set(&DataKey::FeePercent, &fee_bps);
}

/// Read the platform fee in basis points.
///
/// # Errors
/// [`ContractError::NotInitialized`] if the contract has not been initialised.
pub fn read_fee(env: &Env) -> Result<u32, ContractError> {
    env.storage()
        .instance()
        .get(&DataKey::FeePercent)
        .ok_or(ContractError::NotInitialized)
}

// ─── Refund window ───────────────────────────────────────────────────────────

/// Persist the refund window (in seconds) in instance storage.
pub fn write_refund_window(env: &Env, seconds: u64) {
    env.storage()
        .instance()
        .set(&DataKey::RefundWindowSeconds, &seconds);
}

/// Read the refund window in seconds.
///
/// Returns `None` when no explicit value has been set.
#[allow(dead_code)]
pub fn read_refund_window(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::RefundWindowSeconds)
}

// ─── Asset configuration ─────────────────────────────────────────────────────

/// Write (or overwrite) the configuration for a supported asset.
pub fn write_asset_config(env: &Env, asset: &Address, config: &AssetConfig) {
    let key = DataKey::AssetConfig(asset.clone());
    env.storage().persistent().set(&key, config);
    bump_persistent(env, &key);
}

/// Read the configuration for an asset, or `None` if it has never been added.
pub fn read_asset_config(env: &Env, asset: &Address) -> Option<AssetConfig> {
    let key = DataKey::AssetConfig(asset.clone());
    let result = env.storage().persistent().get(&key);
    if result.is_some() {
        bump_persistent(env, &key);
    }
    result
}

/// Return `true` if the asset exists and is currently enabled.
pub fn is_asset_enabled(env: &Env, asset: &Address) -> bool {
    read_asset_config(env, asset)
        .map(|c| c.enabled)
        .unwrap_or(false)
}

// ─── Course configuration ────────────────────────────────────────────────────

/// Write (or overwrite) the payment configuration for a course.
pub fn write_course_config(env: &Env, course_id: &Symbol, config: &CourseConfig) {
    let key = DataKey::CourseConfig(course_id.clone());
    env.storage().persistent().set(&key, config);
    bump_persistent(env, &key);
}

/// Read the payment configuration for a course, or `None` if absent.
pub fn read_course_config(env: &Env, course_id: &Symbol) -> Option<CourseConfig> {
    let key = DataKey::CourseConfig(course_id.clone());
    let result = env.storage().persistent().get(&key);
    if result.is_some() {
        bump_persistent(env, &key);
    }
    result
}
