//! Event publishing helpers for the ChainVerse payment contract.
//!
//! Each function matches the frozen event schema defined in ADR-001.  Topics
//! and payloads must not be changed after the first deployment; only new
//! helpers can be added.
use soroban_sdk::{symbol_short, Address, Env, Symbol};

/// Emitted when the contract administrator address is updated.
///
/// Topic: `ADMIN_SET`
/// Payload: `(new_admin,)`
pub fn admin_set(env: &Env, new_admin: &Address) {
    env.events()
        .publish((symbol_short!("ADMIN_SET"),), (new_admin.clone(),));
}

/// Emitted when the platform treasury address is updated.
///
/// Topic: `TRES_SET`
/// Payload: `(new_treasury,)`
pub fn treasury_set(env: &Env, new_treasury: &Address) {
    env.events()
        .publish((symbol_short!("TRES_SET"),), (new_treasury.clone(),));
}

/// Emitted when the platform fee (in basis points) is updated.
///
/// Topic: `FEE_SET`
/// Payload: `(fee_bps,)`
pub fn fee_set(env: &Env, fee_bps: u32) {
    env.events()
        .publish((symbol_short!("FEE_SET"),), (fee_bps,));
}

/// Emitted when a supported-asset entry is written (add, update, enable, disable).
///
/// Topic: `ASSET_CFG`
/// Payload: `(asset, enabled)`
pub fn asset_configured(env: &Env, asset: &Address, enabled: bool) {
    env.events()
        .publish((symbol_short!("ASSET_CFG"),), (asset.clone(), enabled));
}

/// Emitted when a course payment configuration is written (add or update).
///
/// Topic: `CRSE_CFG`
/// Payload: `(course_id, price, asset, instructor, fee_bps, active)`
pub fn course_configured(
    env: &Env,
    course_id: &Symbol,
    price: i128,
    asset: &Address,
    instructor: &Address,
    fee_bps: u32,
    active: bool,
) {
    env.events().publish(
        (symbol_short!("CRSE_CFG"),),
        (
            course_id.clone(),
            price,
            asset.clone(),
            instructor.clone(),
            fee_bps,
            active,
        ),
    );
}

/// Emitted after a successful course purchase.
///
/// Topic: `PYMT_RCD`
/// Payload: `(student, course_id, amount, asset, instructor, payment_id)`
pub fn payment_recorded(
    env: &Env,
    student: &Address,
    course_id: &Symbol,
    amount: i128,
    asset: &Address,
    instructor: &Address,
    payment_id: &Symbol,
) {
    env.events().publish(
        (symbol_short!("PYMT_RCD"),),
        (
            student.clone(),
            course_id.clone(),
            amount,
            asset.clone(),
            instructor.clone(),
            payment_id.clone(),
        ),
    );
}
