use soroban_sdk::{Address, Env};

use crate::errors::ContractError;
use crate::storage::DataKey;

pub mod token_validation;
pub use token_validation::validate_token_amount;

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

// ---------------------------------------------------------------------------
// Fee calculation
// ---------------------------------------------------------------------------

/// Calculates the protocol fee based on the config.
pub fn calculate_fee(env: &Env, amount: i128) -> Result<i128, ContractError> {
    let config: crate::storage::Config = env
        .storage()
        .persistent()
        .get(&DataKey::Config)
        .ok_or(ContractError::NotInitialized)?;

    // fee in basis points, e.g. 100 = 1%
    // amount * fee / 10000
    Ok((amount * (config.protocol_fee as i128)) / 10000)
}
