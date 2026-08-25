//! Purchase execution for the ChainVerse payment contract.
//!
//! Implements the atomic course-purchase flow required by issue #915:
//! authorization → validation → idempotency checks → SAC token transfer →
//! payment record + enrollment persistence → event emission.
//!
//! Every step before persistence is a pure check, and persistence happens
//! only after a successful token transfer. If any step fails, the invocation
//! returns an error and the Soroban host rolls back all state mutations,
//! guaranteeing that a failed call leaves no payment, enrollment, or
//! accounting residue.

use soroban_sdk::{token, Address, Env, Symbol};

use chainverse_types::{CourseConfig, DataKey, PaymentRecord, FEE_DENOMINATOR};

use crate::errors::ContractError;
use crate::events;
use crate::storage;

/// Compute the platform fee for `amount` at `fee_bps` basis points using
/// truncating (floor) integer division, mirroring the Solidity prototype
/// formula `(amount * fee_bps) / 10000`.
///
/// The computation is split into quotient/remainder terms so it can never
/// overflow an `i128`, even for amounts close to [`i128::MAX`]:
/// - `amount / 10_000 * fee_bps <= amount * MAX_FEE_BASIS_POINTS / 10_000`
///   (bounded by `amount / 5`), and
/// - `(amount % 10_000) * fee_bps < 2 * 10^7`.
pub(crate) fn calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    let bps = i128::from(fee_bps);
    let denom = i128::from(FEE_DENOMINATOR);
    let (quotient, remainder) = (amount / denom, amount % denom);
    quotient * bps + (remainder * bps) / denom
}

/// Resolve the effective fee in basis points for a course.
///
/// A per-course override of `0` means "use the global platform fee".
/// Both values are bounded by `MAX_FEE_BASIS_POINTS` at configuration time.
fn effective_fee_bps(env: &Env, course: &CourseConfig) -> Result<u32, ContractError> {
    if course.fee_bps == 0 {
        storage::read_fee(env)
    } else {
        Ok(course.fee_bps)
    }
}

/// Execute an authorized, idempotent course purchase.
///
/// Steps (in order):
/// 1. Validate the caller-supplied `payment_id` (non-empty, ≤ 32 bytes).
/// 2. Require the student's authorization (`student.require_auth()`).
/// 3. Load the course configuration; reject missing/inactive courses.
/// 4. Verify the configured asset is registered and enabled.
/// 5. Reject duplicate enrollments and duplicate payment IDs **before**
///    moving any funds, so replays never transfer tokens.
/// 6. Transfer exactly the configured price from student to this contract
///    via the Soroban token client (SAC).
/// 7. Persist the payment record (gross + split), enrollment, reservation,
///    and instructor credit atomically.
/// 8. Emit the frozen `PYMT_RCD` event.
pub(crate) fn execute_purchase(
    env: &Env,
    student: &Address,
    course_id: &Symbol,
    payment_id: &Symbol,
) -> Result<(), ContractError> {
    // ── 1. Payment-ID validation ────────────────────────────────────────
    // A reserved ID must be a non-empty symbol (≤ 32 bytes, enforced by the
    // Soroban host at construction).
    if *payment_id == Symbol::new(env, "") {
        return Err(ContractError::InvalidPaymentId);
    }

    // ── 2. Student authorization ────────────────────────────────────────
    student.require_auth();

    // ── 3. Course validation ────────────────────────────────────────────
    let course =
        storage::read_course_config(env, course_id).ok_or(ContractError::CourseNotFound)?;
    if !course.active {
        return Err(ContractError::CourseInactive);
    }

    // ── 4. Asset validation ─────────────────────────────────────────────
    if !storage::is_asset_enabled(env, &course.asset) {
        return Err(ContractError::AssetNotEnabled);
    }

    // ── 5. Idempotency (before any funds move) ─────────────────────────
    if storage::has_enrollment(env, student, course_id) {
        return Err(ContractError::AlreadyEnrolled);
    }
    if env
        .storage()
        .persistent()
        .has(&DataKey::PaymentIdOwner(payment_id.clone()))
    {
        return Err(ContractError::DuplicatePaymentId);
    }

    // ── 6. Split accounting (pure) ──────────────────────────────────────
    let fee_bps = effective_fee_bps(env, &course)?;
    let gross = course.price;
    let fee_amount = calculate_fee(gross, fee_bps);
    let instructor_amount = gross - fee_amount;

    // ── 7. Token movement via the Stellar Asset Contract ────────────────
    let token_client = token::Client::new(env, &course.asset);
    let escrow = env.current_contract_address();
    token_client
        .try_transfer(student, &escrow, &gross)
        .map_err(|_| ContractError::PaymentFailed)?
        .map_err(|_| ContractError::PaymentFailed)?;

    // ── 8. Atomic persistence ───────────────────────────────────────────
    let paid_at = env.ledger().timestamp();
    let record = PaymentRecord {
        student: student.clone(),
        course_id: course_id.clone(),
        amount: gross,
        asset: course.asset.clone(),
        paid_at,
        payment_id: payment_id.clone(),
        fee_amount,
        instructor_amount,
    };
    storage::write_payment_record(env, &record);
    storage::write_enrollment(env, student, course_id, paid_at);
    storage::write_payment_id_owner(env, payment_id, student, course_id);
    // Credit per-asset balances (isolated by Stellar Asset Contract address).
    storage::add_to_instructor_balance(env, &course.instructor, &course.asset, instructor_amount);
    storage::add_to_platform_balance(env, &course.asset, fee_amount);

    // ── 9. Event ────────────────────────────────────────────────────────
    events::payment_recorded(
        env,
        student,
        course_id,
        gross,
        &course.asset,
        &course.instructor,
        payment_id,
    );

    Ok(())
}
