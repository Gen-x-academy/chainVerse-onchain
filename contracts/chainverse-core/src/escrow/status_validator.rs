use super::EscrowStatus;
use crate::errors::ContractError;

/// Validates that transitioning from `current` to `next` is a legal escrow state change.
///
/// Allowed transitions:
///   `Pending` → `Released`   (depositor releases funds to recipient)
///   `Pending` → `Cancelled`  (depositor cancels and reclaims funds)
///
/// All other transitions (e.g. Released → Cancelled, Cancelled → Released) are rejected
/// with `ContractError::InvalidEscrowState`.
pub fn validate_transition(
    current: &EscrowStatus,
    next: &EscrowStatus,
) -> Result<(), ContractError> {
    match (current, next) {
        (EscrowStatus::Pending, EscrowStatus::Released) => Ok(()),
        (EscrowStatus::Pending, EscrowStatus::Cancelled) => Ok(()),
        _ => Err(ContractError::InvalidEscrowState),
    }
}
