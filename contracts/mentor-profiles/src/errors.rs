use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum ContractError {
    /// Admin role is required for this operation
    UnauthorizedAdmin = 1,
    /// Only the mentor can perform this operation
    OnlyMentorAllowed = 2,
    /// Profile not found for given mentor ID
    ProfileNotFound = 3,
    /// Invalid availability status provided
    InvalidStatus = 4,
    /// Invalid timezone offset (must be -12 to +14)
    InvalidTimezone = 5,
    /// Invalid delivery mode bitmask
    InvalidDeliveryMode = 6,
    /// Rate per hour cannot be zero
    InvalidRate = 7,
    /// Rating score must be 1-5
    InvalidRatingScore = 8,
    /// Invalid time window provided
    InvalidTimeWindow = 9,
    /// Profile already exists for this mentor
    ProfileAlreadyExists = 10,
    /// Hash length is invalid
    InvalidHashLength = 11,
    /// Operation failed due to authentication
    AuthenticationFailed = 12,
    /// Invalid input data
    InvalidInput = 13,
    /// Backend signer not authorized
    UnauthorizedBackend = 14,
    /// Reputation score calculation overflow
    ReputationCalcError = 15,
    /// Storage operation failed
    StorageFailed = 16,
}
