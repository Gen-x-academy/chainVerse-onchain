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

use chainverse_types::{AssetConfig, CourseConfig, DataKey, PaymentRecord, MAX_TTL, MIN_TTL};

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

// ─── Enrollment ──────────────────────────────────────────────────────────────

/// Record a student's enrollment in a course (value = enrollment timestamp).
pub fn write_enrollment(env: &Env, student: &Address, course_id: &Symbol, paid_at: u64) {
    let key = DataKey::Enrollment(student.clone(), course_id.clone());
    env.storage().persistent().set(&key, &paid_at);
    bump_persistent(env, &key);
}

/// Return the enrollment timestamp for (student, course_id), or `None`.
#[allow(dead_code)]
pub fn read_enrollment(env: &Env, student: &Address, course_id: &Symbol) -> Option<u64> {
    let key = DataKey::Enrollment(student.clone(), course_id.clone());
    let result = env.storage().persistent().get(&key);
    if result.is_some() {
        bump_persistent(env, &key);
    }
    result
}

/// Return `true` if the student is enrolled in the course.
pub fn has_enrollment(env: &Env, student: &Address, course_id: &Symbol) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Enrollment(student.clone(), course_id.clone()))
}

// ─── Payment records ─────────────────────────────────────────────────────────

/// Persist the full payment receipt keyed by (student, course_id).
pub fn write_payment_record(env: &Env, record: &PaymentRecord) {
    let key = DataKey::PaymentRecord(record.student.clone(), record.course_id.clone());
    env.storage().persistent().set(&key, record);
    bump_persistent(env, &key);
}

/// Read the payment receipt for (student, course_id), or `None` if absent.
///
/// Also bumps the entry TTL so recently inspected receipts stay live.
pub fn read_payment_record(
    env: &Env,
    student: &Address,
    course_id: &Symbol,
) -> Option<PaymentRecord> {
    let key = DataKey::PaymentRecord(student.clone(), course_id.clone());
    let result = env.storage().persistent().get(&key);
    if result.is_some() {
        bump_persistent(env, &key);
    }
    result
}

/// Look up a payment receipt by its globally unique business payment ID.
pub fn read_payment_record_by_id(env: &Env, payment_id: &Symbol) -> Option<PaymentRecord> {
    let (student, course_id) = read_payment_id_owner(env, payment_id)?;
    read_payment_record(env, &student, &course_id)
}

// ─── Payment-ID reservations ────────────────────────────────────────────────

/// Reserve `payment_id` for the (student, course_id) pair.
pub fn write_payment_id_owner(
    env: &Env,
    payment_id: &Symbol,
    student: &Address,
    course_id: &Symbol,
) {
    let key = DataKey::PaymentIdOwner(payment_id.clone());
    let owner = (student.clone(), course_id.clone());
    env.storage().persistent().set(&key, &owner);
    bump_persistent(env, &key);
}

/// Return the (student, course_id) pair that owns `payment_id`, or `None`.
fn read_payment_id_owner(env: &Env, payment_id: &Symbol) -> Option<(Address, Symbol)> {
    let key = DataKey::PaymentIdOwner(payment_id.clone());
    let result = env.storage().persistent().get(&key);
    if result.is_some() {
        bump_persistent(env, &key);
    }
    result
}

// ─── Instructor balances ────────────────────────────────────────────────────

/// Credit `amount` to the instructor's claimable balance for a specific asset.
pub fn add_to_instructor_balance(env: &Env, instructor: &Address, asset: &Address, amount: i128) {
    let current = read_instructor_balance_asset(env, instructor, asset);
    write_instructor_balance_asset(env, instructor, asset, current + amount);
}

/// Overwrite the per-asset instructor claimable balance.
pub fn write_instructor_balance_asset(
    env: &Env,
    instructor: &Address,
    asset: &Address,
    amount: i128,
) {
    let key = DataKey::InstructorBalanceAsset(instructor.clone(), asset.clone());
    env.storage().persistent().set(&key, &amount);
    bump_persistent(env, &key);
}

/// Read the per-asset instructor claimable balance (zero when never credited).
pub fn read_instructor_balance_asset(env: &Env, instructor: &Address, asset: &Address) -> i128 {
    let key = DataKey::InstructorBalanceAsset(instructor.clone(), asset.clone());
    let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if env.storage().persistent().has(&key) {
        bump_persistent(env, &key);
    }
    amount
}

/// Credit `amount` to the platform claimable balance for a specific asset.
pub fn add_to_platform_balance(env: &Env, asset: &Address, amount: i128) {
    let current = read_platform_balance_asset(env, asset);
    write_platform_balance_asset(env, asset, current + amount);
}

/// Overwrite the per-asset platform claimable balance.
pub fn write_platform_balance_asset(env: &Env, asset: &Address, amount: i128) {
    let key = DataKey::PlatformBalanceAsset(asset.clone());
    env.storage().persistent().set(&key, &amount);
    bump_persistent(env, &key);
}

/// Read the per-asset platform claimable balance (zero when never credited).
pub fn read_platform_balance_asset(env: &Env, asset: &Address) -> i128 {
    let key = DataKey::PlatformBalanceAsset(asset.clone());
    let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if env.storage().persistent().has(&key) {
        bump_persistent(env, &key);
    }
    amount
}

// ─── Legacy instructor balance (kept for backwards-compatibility) ────────────

/// Overwrite the legacy aggregate instructor claimable balance.
/// Kept so that previously-deployed data keys remain readable.
#[allow(dead_code)]
pub fn write_instructor_balance(env: &Env, instructor: &Address, amount: i128) {
    let key = DataKey::InstructorBalance(instructor.clone());
    env.storage().persistent().set(&key, &amount);
    bump_persistent(env, &key);
}

/// Read the legacy aggregate instructor claimable balance (zero when absent).
#[allow(dead_code)]
pub fn read_instructor_balance(env: &Env, instructor: &Address) -> i128 {
    let key = DataKey::InstructorBalance(instructor.clone());
    let amount: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if env.storage().persistent().has(&key) {
        bump_persistent(env, &key);
    }
    amount
}
