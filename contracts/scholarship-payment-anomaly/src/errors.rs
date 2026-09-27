use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    /// Payment alert not found for this payment reference.
    AlertNotFound = 5,
    /// An alert already exists for this payment reference.
    AlertAlreadyExists = 6,
    /// Payment reference exceeds the 64-byte limit.
    PaymentRefTooLong = 7,
    /// Alert description exceeds the 512-byte limit.
    DescriptionTooLong = 8,
    /// Evidence reference exceeds the 256-byte limit.
    EvidenceRefTooLong = 9,
    /// Resolution notes exceed the 1 024-byte limit.
    NotesTooLong = 10,
    /// The alert is in a terminal state and cannot be modified.
    AlertAlreadyResolved = 11,
    /// Requested pause window has already expired.
    PauseWindowExpired = 12,
    /// Arithmetic overflow detected in amount calculation.
    ArithmeticOverflow = 13,
    /// The payment amount must be positive.
    InvalidAmount = 14,
    /// Invalid wallet change velocity — must be ≥ 0.
    InvalidVelocity = 15,
}
