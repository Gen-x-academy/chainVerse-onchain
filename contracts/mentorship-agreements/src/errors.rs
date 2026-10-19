use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MentorshipError {
    /// Unauthorized access attempt
    Unauthorized = 1,
    /// Contract already initialized
    AlreadyInitialized = 2,
    /// Contract not initialized
    NotInitialized = 3,
    /// Agreement not found
    AgreementNotFound = 4,
    /// Invalid agreement status
    InvalidStatus = 5,
    /// Invalid time range
    InvalidTimeRange = 6,
    /// Invalid price
    InvalidPrice = 7,
    /// Invalid cadence
    InvalidCadence = 8,
    /// Participant addresses must differ
    SameParticipant = 9,
    /// Agreement already accepted by this party
    AlreadyAccepted = 10,
    /// Agreement not yet accepted by both parties
    NotFullyAccepted = 11,
    /// Amendment not found
    AmendmentNotFound = 12,
    /// Amendment already processed
    AmendmentAlreadyProcessed = 13,
    /// Invalid amendment proposer
    InvalidProposer = 14,
    /// No changes in amendment
    NoChanges = 15,
    /// Amendment requires approval from both parties
    RequiresApproval = 16,
    /// Agreement must be active to amend
    MustBeActive = 17,
    /// Invalid nonce (replay attack detected)
    InvalidNonce = 18,
    /// Hash mismatch
    HashMismatch = 19,
    /// Terms version mismatch
    TermsVersionMismatch = 20,
    /// Users are blocked from matching
    UsersBlocked = 21,
    /// Block already exists
    BlockAlreadyExists = 22,
    /// Block not found
    BlockNotFound = 23,
    /// Cannot block self
    CannotBlockSelf = 24,
    /// Invalid hash length
    InvalidHash = 25,
    /// Arithmetic overflow
    Overflow = 26,
    /// Invalid token address
    InvalidToken = 27,
}
