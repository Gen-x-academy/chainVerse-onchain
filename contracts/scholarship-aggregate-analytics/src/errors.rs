use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ContractPaused = 4,
    /// The cohort tag does not exist; no data has been recorded for it.
    CohortNotFound = 5,
    /// The cohort is suppressed — size is below the k-anonymity threshold.
    CohortSuppressed = 6,
    /// A metric key exceeds the 32-byte limit.
    MetricKeyTooLong = 7,
    /// Attempted arithmetic overflow in counter update.
    ArithmeticOverflow = 8,
    /// The cohort tag exceeds the 64-byte limit.
    CohortTagTooLong = 9,
    /// Caller tried to decrement a counter below zero.
    CounterUnderflow = 10,
    /// Metric definition string exceeds 512-byte limit.
    MetricDefinitionTooLong = 11,
}
