use crate::errors::EscrowError;
use crate::types::EscrowStatus;

/// #714 — single source of truth for allowed escrow status transitions.
///
/// Every status-changing function should call this before writing a new
/// status, so adding a new transition only requires updating the map below.
pub fn assert_transition_allowed(
    from: &EscrowStatus,
    to: &EscrowStatus,
) -> Result<(), EscrowError> {
    let allowed: &[EscrowStatus] = match from {
        EscrowStatus::Created => &[EscrowStatus::Funded, EscrowStatus::Cancelled],
        EscrowStatus::Funded => &[
            EscrowStatus::Completed,
            EscrowStatus::Cancelled,
            EscrowStatus::Disputed,
        ],
        EscrowStatus::Disputed => &[EscrowStatus::Completed, EscrowStatus::Cancelled],
        // Completed / Cancelled are terminal.
        _ => &[],
    };

    if allowed.contains(to) {
        Ok(())
    } else {
        Err(EscrowError::InvalidEscrowState)
    }
}
