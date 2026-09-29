use soroban_sdk::contracterror;

/// All error codes for the scholarship-fairness-review contract.
/// Codes are stable integers — never reorder or reuse a retired value.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// `init` called more than once.
    AlreadyInitialized = 1,
    /// Operation requires prior `init`.
    NotInitialized = 2,
    /// Caller is not the stored admin.
    Unauthorized = 3,
    /// Contract is administratively paused.
    ContractPaused = 4,
    /// Proposed rule version already exists.
    RuleVersionExists = 5,
    /// Referenced rule version does not exist.
    RuleVersionNotFound = 6,
    /// Review outcome has already been recorded for this version.
    ReviewAlreadyRecorded = 7,
    /// `rollback_to` target version has no passing review.
    RollbackTargetNotApproved = 8,
    /// A high-risk change requires a second approver before activation.
    HighRiskApprovalRequired = 9,
    /// The second approver must differ from the proposer.
    ApproverMustDifferFromProposer = 10,
    /// `activate` called on a version that is not approved.
    VersionNotApproved = 11,
    /// Proposed rule payload exceeds the 4 096-byte on-chain limit.
    RulePayloadTooLarge = 12,
    /// Test cohort tag exceeds 64-byte limit.
    CohortTagTooLong = 13,
    /// Attempted to write an empty rule payload.
    EmptyRulePayload = 14,
    /// Caller supplied an invalid (zero) version number.
    InvalidVersionNumber = 15,
}
