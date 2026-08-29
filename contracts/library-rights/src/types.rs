//! Data-minimization boundary for on-chain library records (#927).
//!
//! ## Classification
//! - **Public/allowed on-chain:** content hashes (`BytesN<32>`),
//!   pseudonymous `Address` values, coarse status enums/timestamps.
//! - **Prohibited on-chain:** names, emails, raw book/content bytes,
//!   reading position/progress, or staff notes -- or any other field
//!   that identifies a person or exposes content/behavioral detail.
//!   That data belongs in the off-chain backend (chainVerse-backend
//!   #979) and is referenced here only by its hash where verification
//!   is needed.
//!
//! [`WorkRecord`] is intentionally minimal: it holds nothing beyond a
//! content hash and a pseudonymous custodian address. Any future field
//! added to a type in this module must be checked against the
//! prohibited list above before being merged.

use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// A registered work's on-chain record.
///
/// Contains only a content hash and a pseudonymous custodian address --
/// never a name, email, raw content, reading position, or staff note.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    /// Hash of the off-chain work content. Never the content itself.
    pub work_hash: BytesN<32>,
    /// Pseudonymous on-chain address of the work's current custodian.
    pub custodian: Address,
    /// Policy that applies to this work, which defines concurrent loan limits
    pub policy_id: Symbol,
}

/// Policy configuration that defines loan limits for patrons
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Maximum number of concurrent active loans a single patron can have under this policy
    pub max_concurrent_loans_per_patron: u32,
    /// Total number of active loans currently active across all patrons under this policy
    pub total_active_loans: u32,
    /// Maximum total concurrent loans allowed across all patrons
    pub max_total_concurrent_loans: u32,
}

/// Reason codes for renewal denials
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RenewalDenialReason {
    /// Loan is not active
    LoanNotActive,
    /// Loan has already reached maximum renewals
    MaxRenewalsReached,
    /// Work is on hold by another patron
    WorkOnHold,
    /// Policy loan limit would be exceeded
    PolicyLimitExceeded,
    /// Patron's loan limit would be exceeded
    PatronLimitExceeded,
    /// New expiry would exceed license maximum duration
    ExceedsLicenseExpiry,
}

/// Active loan record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LoanRecord {
    /// Work ID being loaned
    pub work_id: BytesN<32>,
    /// Patron who borrowed the work
    pub holder: Address,
    /// Timestamp when the loan was created
    pub created_at: u64,
    /// Timestamp when the loan expires
    pub expires_at: u64,
    /// Whether the loan is still active
    pub is_active: bool,
    /// Policy ID that applies to this loan
    pub policy_id: Symbol,
    /// Number of times this loan has been renewed
    pub renewal_count: u32,
    /// Whether auto-renewal is enabled for this loan
    pub auto_renew: bool,
    /// Maximum timestamp this loan can be extended to (license expiry)
    pub max_license_expiry: u64,
    /// Maximum number of renewals allowed for this loan
    pub max_renewals: u32,
}

/// Active hold record for a work that's currently loaned out
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HoldRecord {
    /// Work ID being held
    pub work_id: BytesN<32>,
    /// Patron who placed the hold
    pub holder: Address,
    /// Timestamp when the hold was created
    pub created_at: u64,
    /// Timestamp when the hold expires if not fulfilled
    pub expires_at: u64,
    /// Whether the hold is still active
    pub is_active: bool,
    /// Policy ID that applies to this hold
    pub policy_id: Symbol,
    /// Position in the queue for this work
    pub queue_position: u32,
    /// Unique nonce for idempotent operations
    pub request_nonce: BytesN<32>,
}

/// Reason codes for hold cancellation
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum HoldCancellationReason {
    /// Patron voluntarily cancelled their hold
    PatronInitiated,
    /// Librarian administratively cancelled the hold
    LibrarianInitiated,
    /// Hold expired before being fulfilled
    HoldExpired,
}

/// Tracks the number of active holds per patron per policy
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PatronPolicyActiveHolds {
    /// Count of active holds
    pub count: u32,
}