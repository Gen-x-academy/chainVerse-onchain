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
    InvalidParties = 9,
    InvalidExpiration = 10,
    AlreadyDisputed = 11,
    DisputeResolutionNotImplemented = 12,
    NoFeesAvailable = 13,
}
