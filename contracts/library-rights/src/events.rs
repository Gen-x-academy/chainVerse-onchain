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