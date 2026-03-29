use soroban_sdk::contracttype;

/// Lifecycle states of an escrow deposit.
///
/// Transitions:
///   Pending → Completed  (depositor calls release)
///   Pending → Refunded   (depositor cancels, funds returned)
///   Pending → Cancelled  (admin or timeout cancellation)
///   Pending → Expired    (escrow passed its deadline without action)
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum EscrowStatus {
    /// Funds are held, awaiting action.
    Pending,
    /// Funds have been released to the recipient.
    Completed,
    /// Funds have been returned to the depositor.
    Refunded,
    /// Escrow was cancelled before completion.
    Cancelled,
    /// Escrow expired without being acted upon.
    Expired,
}
