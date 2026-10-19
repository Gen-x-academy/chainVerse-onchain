use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    MentorIneligible = 4,
    MenteeIneligible = 5,
    NotFound = 6,
    Unauthorized = 7,
    InvalidExpiry = 8,
    InvalidStatus = 9,
}
