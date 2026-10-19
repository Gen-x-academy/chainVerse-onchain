use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// Mentor has reached their maximum concurrent engagements.
    MaxConcurrentExceeded = 4,
    /// Mentor has reached their maximum weekly hours.
    MaxWeeklyHoursExceeded = 5,
    /// Program or cohort has reached their maximum mentorships.
    ProgramCapacityExceeded = 6,
    /// Configuration not found for the given mentor or program.
    NotFound = 7,
    /// The caller is not authorized for this operation.
    Unauthorized = 8,
    /// Invalid configuration parameter (e.g., negative limits).
    InvalidConfig = 9,
}
