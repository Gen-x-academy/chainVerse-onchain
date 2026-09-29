use soroban_sdk::{contracttype, Address, String};

/// Category of payment anomaly detected.
///
/// Each variant corresponds to a detection rule in the acceptance criteria.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnomalyCategory {
    /// Destination wallet is newly registered, high-risk jurisdiction, or
    /// matches a known suspicious address.
    UnusualDestination,
    /// The recipient's destination wallet address changed within a short
    /// window before disbursement.
    RapidWalletChange,
    /// Multiple disbursements reference the same ledger sequence number or
    /// transaction hash.
    DuplicateLedgerReference,
    /// The disbursement amount is split into many sub-transactions in a
    /// pattern consistent with structuring / smurfing.
    SuspiciousSplit,
    /// The disbursement failed repeatedly (e.g. reverts, timeouts) before
    /// succeeding, which may indicate front-running or manipulation.
    RepeatedFailure,
    /// Catch-all for anomalies that do not fit the specific categories.
    Other,
}

/// Lifecycle state of a payment alert.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AlertStatus {
    /// Alert raised; payment is paused pending review.
    PausedPendingReview,
    /// Authorised resolver confirmed the anomaly — payment remains blocked.
    Confirmed,
    /// Resolver investigated and cleared the alert; payment may proceed.
    Cleared,
    /// Alert was superseded by a more recent alert for the same reference.
    Superseded,
}

/// A payment anomaly alert record.
///
/// # Safe pause guarantee
/// When an alert is raised with `pause_payment = true`, the downstream
/// disbursement system must check `is_payment_paused()` before executing.
/// This contract does not hold funds; it is a coordination layer.
///
/// # Evidence standards
/// `evidence_ref` must be a content-addressed off-chain pointer (IPFS CID,
/// SHA-256 hex of evidence bundle, etc.).  Raw wallet addresses of
/// uninvolved parties, biometrics, or PII must not appear in any string
/// field.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PaymentAlert {
    /// Opaque payment reference (max 64 bytes).  Should map to a ledger
    /// transaction ID, an application disbursement ID, etc.
    pub payment_ref: String,
    /// Anomaly category.
    pub category: AnomalyCategory,
    /// Current alert status.
    pub status: AlertStatus,
    /// Human-readable description (max 512 bytes, no PII).
    pub description: String,
    /// Content-addressed evidence pointer (max 256 bytes).
    pub evidence_ref: String,
    /// Payment amount in smallest token units (informational, i128 ≥ 0).
    pub amount: i128,
    /// Destination address of the payment.  Stored for audit purposes;
    /// must not be used for any purpose other than the current alert.
    pub destination: Address,
    /// Whether the alert has triggered a payment pause.
    pub payment_paused: bool,
    /// Ledger timestamp after which the pause automatically expires (0 = no
    /// expiry; resolver must explicitly clear).
    pub pause_expires_at: u64,
    /// The detector service account that raised this alert.
    pub raised_by: Address,
    /// Ledger timestamp when the alert was raised.
    pub raised_at: u64,
    /// Resolver who last acted on this alert (None = not resolved).
    pub resolved_by: Option<Address>,
    /// Resolver's notes (max 1 024 bytes).
    pub resolution_notes: String,
    /// Ledger timestamp of the last resolution action (0 = not resolved).
    pub resolved_at: u64,
    /// Rapid wallet change velocity counter (number of wallet changes in the
    /// look-back window, 0 = N/A).
    pub wallet_change_velocity: u32,
    /// Number of times this payment has failed and been retried.
    pub retry_count: u32,
}

/// Compact summary for external queries.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AlertSummary {
    pub payment_ref: String,
    pub category: AnomalyCategory,
    pub status: AlertStatus,
    pub description: String,
    pub amount: i128,
    pub payment_paused: bool,
    pub raised_at: u64,
    pub resolved_at: u64,
}
