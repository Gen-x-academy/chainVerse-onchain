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
    /// No policy record exists for the given policy id.
    PolicyNotFound = 6,
    /// Patron has exceeded their maximum concurrent loans for this policy.
    PatronLoanLimitExceeded = 7,
    /// Total policy-wide concurrent loan limit has been reached.
    PolicyLoanLimitExceeded = 8,
    /// Cannot check out a work that is already loaned out.
    WorkAlreadyLoaned = 9,
    /// Cannot return a loan that doesn't exist or is already inactive.
    LoanNotFoundOrInactive = 10,
}