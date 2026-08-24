//! Stable error discriminants for the ChainVerse payment contract.
//!
//! Discriminant values are frozen.  Never remove or renumber an entry once
//! deployed; only append new variants at the end.
use soroban_sdk::contracterror;

/// All errors that can be returned by the payment contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    /// `initialize` was called on a contract that is already initialised.
    AlreadyInitialized = 1,
    /// The caller is not the contract administrator.
    NotAdmin = 2,
    /// A method that requires initialization was called before `initialize`.
    NotInitialized = 3,
    /// The supplied fee exceeds `MAX_FEE_BASIS_POINTS` (2 000 bps / 20 %).
    InvalidFee = 4,
    /// The referenced course has no configuration entry.
    CourseNotFound = 5,
    /// The course exists but its `active` flag is `false`.
    CourseInactive = 6,
    /// The student is already enrolled in this course.
    AlreadyEnrolled = 7,
    /// The student is not enrolled in this course.
    NotEnrolled = 8,
    /// The SAC `transfer` call failed or the student balance was insufficient.
    PaymentFailed = 9,
    /// A refund was requested but the refund window has expired.
    RefundWindowExpired = 10,
    /// The instructor has no claimable balance.
    InsufficientBalance = 11,
    /// The SAC `transfer` back to the student/instructor failed.
    TransferFailed = 12,
    /// A monetary amount supplied by the caller is zero or negative.
    InvalidAmount = 13,
    /// The asset address is the zero address or otherwise invalid.
    InvalidAsset = 14,
    /// The caller supplied an address that does not match the expected role.
    UnauthorizedCaller = 15,
    /// The asset has been configured but is currently disabled.
    AssetNotEnabled = 16,
    /// The asset has never been added via `add_asset`.
    AssetNotFound = 17,
    /// The supplied address (admin, treasury, instructor) is invalid.
    InvalidAddress = 18,
}
