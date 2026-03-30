use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    AlreadyInitialized = 2,
    NotInitialized = 3,
    ContractPaused = 4,
    InvalidAmount = 5,
    UnsupportedToken = 6,
    EscrowNotFound = 7,
    InvalidEscrowState = 8,
    EscrowNotExpired = 9,
}
