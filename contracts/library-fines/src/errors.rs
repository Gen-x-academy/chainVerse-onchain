use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    DuplicateReference = 4,
    EntryNotFound = 5,
    SettlementNotFound = 6,
    InvalidAmount = 7,
    WaiverExceedsBalance = 8,
    UnsupportedAsset = 9,
    DuplicateSettlement = 10,
    InvalidStateTransition = 11,
    PaymentExceedsBalance = 12,
}
