use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    NotInitialized = 3,
    InvalidFee = 4,
    CourseNotFound = 5,
    CourseInactive = 6,
    AlreadyEnrolled = 7,
    NotEnrolled = 8,
    PaymentFailed = 9,
    RefundWindowExpired = 10,
    InsufficientBalance = 11,
    TransferFailed = 12,
    InvalidAmount = 13,
    InvalidToken = 14,
    UnauthorizedCaller = 15,
}
