//! Shared PayoutRecord validation — issue #733.
//!
//! Centralises the rules that must hold for any payout entry, whether it
//! comes from `execute()` (immediate batch) or `schedule_payout()` (deferred).
//! Both call-sites import and call `validate_payout_record()`, so a fix in
//! one place applies everywhere.

use soroban_sdk::Address;
use crate::PayoutError;

/// Maximum individual payout amount (1 billion stroops / base units).
/// Prevents accidental overflow or treasury drain from a single entry.
pub const MAX_PAYOUT_AMOUNT: i128 = 1_000_000_000;

/// Validate a single payout entry `(recipient, amount)`.
///
/// Rules enforced:
///   1. `amount` must be strictly positive (> 0).
///   2. `amount` must not exceed `MAX_PAYOUT_AMOUNT`.
///   3. The `recipient` address is passed through as-is — zero-address checks
///      are handled by the Soroban runtime during the actual token transfer,
///      but we keep the parameter here so future checks can be added without
///      changing call-sites.
///
/// Returns `Ok(())` on success, or the first failing [`PayoutError`].
pub fn validate_payout_record(recipient: &Address, amount: i128) -> Result<(), PayoutError> {
    // Rule 1 — amount must be positive (covers both negative and zero).
    if amount <= 0 {
        return Err(PayoutError::NegativeAmount);
    }

    // Rule 2 — amount must not exceed the per-entry ceiling.
    if amount > MAX_PAYOUT_AMOUNT {
        return Err(PayoutError::NegativeAmount); // reuse closest error; extend enum if needed
    }

    // recipient is accepted as a lint-free parameter for future extensibility.
    let _ = recipient;

    Ok(())
}

/// Validate every entry in a batch before any transfer is attempted.
///
/// Iterates the slice and returns on the first invalid entry, guaranteeing
/// that the caller can safely proceed to execution knowing the whole batch
/// is valid (all-or-nothing semantics).
pub fn validate_batch(
    payouts: &soroban_sdk::Vec<(Address, i128)>,
) -> Result<(), PayoutError> {
    for (recipient, amount) in payouts.iter() {
        validate_payout_record(&recipient, amount)?;
    }
    Ok(())
}
