use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TokenError {
    AlreadyInitialized = 0,
    NotInitialized = 1,
    InvalidAmount = 2,
    InsufficientBalance = 3,
    SelfTransfer = 4,
}
