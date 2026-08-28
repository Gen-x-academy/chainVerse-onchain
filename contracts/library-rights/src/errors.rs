use soroban_sdk::contracterror;

/// Typed errors for the library-rights contract.
///
/// Kept local to this crate rather than re-using `shared::ContractError`:
/// every existing workspace contract (`course_registry`, `staking`,
/// `token`, `payout-automation`, `escrow-vault`, ...) defines its own
/// local error enum despite `docs/contracts.md` describing a shared-enum
/// convention, so this follows the convention actually in force across
/// the codebase.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// `bootstrap` was called after the contract was already bootstrapped.
    AlreadyInitialized = 1,
    /// A role-gated call was made before `bootstrap` ever succeeded.
    NotInitialized = 2,
    /// The caller does not hold the role required for this call.
    NotAdmin = 3,
    /// Two or more of the four roles were given the same address.
    DuplicateRole = 4,
    /// No work record exists for the given work id.
    WorkNotFound = 5,
    /// The requested loan was not found.
    LoanNotFound = 7,
    /// The requested hold was not found.
    HoldNotFound = 8,
    /// The requested hold has expired.
    HoldExpired = 9,
    /// A reserve for the given course already exists.
    ReserveExists = 10,
    /// The requested reserve was not found.
    ReserveNotFound = 11,
    /// The user is not enrolled in the course required for this loan.
    NotEnrolled = 12,
    /// There are no seats available for the requested reserve.
    NoSeatsAvailable = 13,
    /// The course registry contract address has not been set.
    CourseRegistryNotSet = 14,
}