use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    Unauthorized = 1,
    InvalidSignature = 2,
    NoPenaltiesToWithdraw = 3,
    Unauthorized       = 1,
    InvalidSignature   = 2,
    AlreadyInitialized = 3,
    AlreadyRewarded    = 4,
    NotInitialized     = 5,
    InsufficientTreasuryAllowance = 6,
    ContractPaused     = 7,
}
