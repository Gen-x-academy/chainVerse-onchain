use soroban_sdk::contracterror;

/// Typed errors for the library-rights contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAdmin = 3,
    DuplicateRole = 4,
    WorkNotFound = 5,
    InvalidPolicy = 6,
    PolicyNotFound = 7,
    PolicyVersionNotFound = 8,
    LicenseNotFound = 9,
    LicenseInactive = 10,
    RenditionNotFound = 11,
    RenditionInactive = 12,
    SeatNotFound = 13,
    SeatUnavailable = 14,
    BorrowingLimitReached = 15,
    NotEnrolled = 16,
    CourseRegistryCallFailed = 17,
    InvalidTimestamp = 18,
    LoanNotFound = 19,
    LoanIdOverflow = 20,
}
