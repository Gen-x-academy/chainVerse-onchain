use soroban_sdk::{contractevent, Address, BytesN, Symbol};

/// Emitted when a patron successfully checks out a work, creating an active loan.
#[contractevent]
pub struct LoanCreated {
    /// Unique identifier of the loan
    pub loan_id: BytesN<32>,
    /// Work ID that was checked out
    pub work_id: BytesN<32>,
    /// Patron who borrowed the work
    pub holder: Address,
    /// Timestamp when the loan was created
    pub created_at: u64,
    /// Timestamp when the loan expires
    pub expires_at: u64,
    /// Policy ID that applies to this loan
    pub policy_id: Symbol,
}

/// Emitted when a patron successfully returns a work, closing the active loan.
#[contractevent]
pub struct LoanReturned {
    /// Unique identifier of the loan that was closed
    pub loan_id: BytesN<32>,
    /// Work ID that was returned
    pub work_id: BytesN<32>,
    /// Patron who returned the work
    pub holder: Address,
    /// Timestamp when the loan was returned
    pub returned_at: u64,
    /// Policy ID that applied to this loan
    pub policy_id: Symbol,
}

/// Emitted when a new policy is created or updated by the PolicyManager.
#[contractevent]
pub struct PolicyUpdated {
    /// ID of the policy that was updated
    pub policy_id: Symbol,
    /// New maximum concurrent loans per patron
    pub max_concurrent_loans_per_patron: u32,
    /// New maximum total concurrent loans across all patrons
    pub max_total_concurrent_loans: u32,
}

/// Emitted when a keeper is added to the allowlist.
#[contractevent]
pub struct KeeperAdded {
    /// Address of the keeper that was added
    pub keeper: Address,
}

/// Emitted when a keeper is removed from the allowlist.
#[contractevent]
pub struct KeeperRemoved {
    /// Address of the keeper that was removed
    pub keeper: Address,
}

/// Emitted when an auto-renew evaluation completes.
#[contractevent]
pub struct RenewalEvaluated {
    /// Number of loans that were processed during evaluation
    pub processed_loans: u32,
    /// Number of loans that expired and were closed
    pub expired_loans: u32,
    /// Address of the caller that triggered the evaluation
    pub caller: Address,
}

/// Emitted when a loan is successfully renewed.
#[contractevent]
pub struct LoanRenewed {
    /// Unique identifier of the loan that was renewed
    pub loan_id: BytesN<32>,
    /// Work ID that was renewed
    pub work_id: BytesN<32>,
    /// Patron who renewed the loan
    pub holder: Address,
    /// Previous expiration timestamp
    pub previous_expires_at: u64,
    /// New expiration timestamp
    pub new_expires_at: u64,
    /// Number of times this loan has been renewed
    pub renewal_count: u32,
    /// Policy ID that applies to this loan
    pub policy_id: Symbol,
}

/// Emitted when a loan renewal is denied.
#[contractevent]
pub struct LoanRenewalDenied {
    /// Unique identifier of the loan that was denied renewal
    pub loan_id: BytesN<32>,
    /// Work ID of the loan
    pub work_id: BytesN<32>,
    /// Patron of the loan
    pub holder: Address,
    /// Reason why renewal was denied
    pub reason: crate::types::RenewalDenialReason,
}

/// Emitted when a hold is successfully cancelled.
#[contractevent]
pub struct HoldCancelled {
    /// Unique identifier of the hold that was cancelled
    pub hold_id: BytesN<32>,
    /// Work ID of the hold
    pub work_id: BytesN<32>,
    /// Patron who placed the hold
    pub holder: Address,
    /// Timestamp when the hold was cancelled
    pub cancelled_at: u64,
    /// Reason for cancellation
    pub reason: crate::types::HoldCancellationReason,
    /// Policy ID that applied to this hold
    pub policy_id: Symbol,
    /// Whether the next hold in queue was advanced to readiness
    pub next_hold_advanced: bool,
}