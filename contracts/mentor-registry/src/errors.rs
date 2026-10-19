use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum MentorError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidInput = 4,
    MentorNotFound = 5,
    ProgramNotFound = 6,
    RateLimitExceeded = 7,
    AbuseSignalDetected = 8,
    InvalidDeliveryMode = 9,
    InvalidPrice = 10,
    InvalidAvailability = 11,
    ProgramFull = 12,
    InvalidProgram = 13,
}
