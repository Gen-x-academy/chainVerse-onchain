use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TokenError {
    AlreadyInitialized = 0,
    NotInitialized = 1,
    InvalidAmount = 2,
    InsufficientBalance = 3,
    Unauthorized = 4,
    SelfTransfer = 5,
    SupplyCapExceeded = 6,
    NoPendingAdmin = 7,
}
