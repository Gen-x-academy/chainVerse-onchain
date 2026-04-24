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
    NoFeesAvailable = 8,
    AlreadyDisputed = 9,
    InvalidExpiration = 8,
    InvalidParties = 9,
}
