use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    /// The signal record for this application does not exist.
    SignalNotFound = 5,
    /// A signal record already exists for this application ID.
    SignalAlreadyExists = 6,
    /// The appeal already exists for this application.
    AppealAlreadyFiled = 7,
    /// No appeal on record for this application.
    AppealNotFound = 8,
    /// Reviewer resolution is not allowed on a non-Flagged signal.
    SignalNotFlagged = 9,
    /// Application ID exceeds the 64-byte limit.
    AppIdTooLong = 10,
    /// Signal description exceeds the 512-byte limit.
    DescriptionTooLong = 11,
    /// Evidence reference exceeds the 256-byte limit.
    EvidenceRefTooLong = 12,
    /// Appeals note exceeds the 1 024-byte limit.
    NotesTooLong = 13,
    /// Score must be in 0-100 range.
    InvalidScore = 14,
}
