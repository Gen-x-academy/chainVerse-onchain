use crate::errors::ContractError;

/// Maximum token transfer amount allowed in a single escrow (i128::MAX / 2 to avoid overflow in fee math).
const MAX_TRANSFER_AMOUNT: i128 = i128::MAX / 2;

/// Validates a token transfer amount for use in escrow creation.
///
/// Rules:
/// - Must be strictly greater than zero (rejects zero-value transfers).
/// - Must not exceed `MAX_TRANSFER_AMOUNT` (guards against overflow in fee calculations).
///
/// Returns `ContractError::InvalidAmount` when either check fails.
pub fn validate_token_amount(amount: i128) -> Result<(), ContractError> {
    if amount <= 0 {
        return Err(ContractError::InvalidAmount);
    }
    if amount > MAX_TRANSFER_AMOUNT {
        return Err(ContractError::InvalidAmount);
    }
    Ok(())
}
