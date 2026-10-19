use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ConflictDetected = 4,
    NotFound = 5,
    Unauthorized = 6,
    InvalidRelationshipType = 7,
}
