use soroban_sdk::{Address, Env};

use crate::errors::ContractError;
use crate::storage::DataKey;

// ---------------------------------------------------------------------------
// Amount validation
// ---------------------------------------------------------------------------

/// Returns `InvalidAmount` when `amount` is not strictly positive.
pub fn validate_amount(amount: i128) -> Result<(), ContractError> {
    if amount <= 0 {
        Err(ContractError::InvalidAmount)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Token validation
// ---------------------------------------------------------------------------

/// Returns `true` when `token` appears in the contract's supported-token list.
/// Returns `false` (rather than an error) when the contract is uninitialized,
/// so callers can decide how to handle that case.
pub fn is_token_supported(env: &Env, token: &Address) -> bool {
    let Some(config) = env
        .storage()
        .persistent()
        .get::<DataKey, crate::storage::Config>(&DataKey::Config)
    else {
        return false;
    };

    config.supported_tokens.contains(token)
}

/// Returns `UnsupportedToken` when `token` is not in the supported-token list.
pub fn require_supported_token(env: &Env, token: &Address) -> Result<(), ContractError> {
    if is_token_supported(env, token) {
        Ok(())
    } else {
        Err(ContractError::UnsupportedToken)
    }
}
