use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    InvalidAsset = 1,
    IncorrectAmount = 2,
    AlreadyPurchased = 3,
    CourseNotFound = 4,
}