//! Reward eligibility checks — issue #727.
//!
//! Separates eligibility rules from token-transfer logic so each concern
//! can be audited and tested independently.

use soroban_sdk::{Env, Address};
use crate::errors::Error;
use crate::storage::{has_been_rewarded, get_reward_amount, get_token, get_treasury};

/// Verify that `user` is eligible to claim a reward.
///
/// Checks performed (in order):
///   1. The user has not already been rewarded.
///   2. The configured reward amount is positive.
///   3. The treasury holds sufficient allowance to cover the reward.
///
/// Returns `Ok(())` when all checks pass, or the first failing [`Error`].
pub fn assert_eligible(env: &Env, user: &Address) -> Result<(), Error> {
    // Rule 1 — no double-claiming.
    if has_been_rewarded(env, user) {
        return Err(Error::AlreadyRewarded);
    }

    // Rule 2 — reward amount must be positive (guards against mis-initialisation).
    let reward_amount = get_reward_amount(env)?;
    if reward_amount <= 0 {
        return Err(Error::NotInitialized);
    }

    // Rule 3 — treasury allowance must cover at least the reward amount.
    let token_address = get_token(env)?;
    let treasury = get_treasury(env)?;
    let token_client = soroban_sdk::token::Client::new(env, &token_address);
    let allowance = token_client.allowance(&treasury, &env.current_contract_address());
    if allowance < reward_amount {
        return Err(Error::InsufficientTreasuryAllowance);
    }

    Ok(())
}

/// Returns `true` when `user` has already been rewarded.
///
/// Thin wrapper kept public so call-sites can do a cheap read-only check
/// without paying for the full `assert_eligible` path.
pub fn is_already_rewarded(env: &Env, user: &Address) -> bool {
    has_been_rewarded(env, user)
}
