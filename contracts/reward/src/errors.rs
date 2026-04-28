use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Unauthorized       = 1,
    InvalidSignature   = 2,
    AlreadyInitialized = 3,
    AlreadyRewarded    = 4,
    NotInitialized     = 5,
}
