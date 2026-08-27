use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    NotFound = 1,
    NotPending = 2,
    Expired = 3,
    Unauthorized = 4,
    TokenNotAllowed = 5,
    NotExpired = 6,
    AlreadyReleased = 7,
    InvalidAmount = 8,
    InvalidRecipient = 9,
    InvalidExpiration = 10,
    AlreadyDisputed = 11,
    DisputeResolutionNotImplemented = 12,
    NoFeesAvailable = 13,
    ContractPaused = 14,
    /// Escrow is not in the expected lifecycle state for this action.
    InvalidEscrowState = 15,
    /// No arbiter is configured to resolve disputes (#864).
    NoArbiterConfigured = 16,
    /// Attempted to resolve an escrow that is not in the Disputed state (#864).
    NotDisputed = 17,
    /// The dispute resolution allocation is invalid (negative, or exceeds the
    /// remaining escrow balance once fees are accounted for) (#864).
    InvalidAllocation = 18,
}
