use soroban_sdk::{contracttype, Address, String};

/// The category of the fraud signal.
///
/// Categories are intentionally coarse so that no single bit of information
/// about the applicant is inadvertently exposed through the signal type.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalCategory {
    /// Same or very similar application content submitted by multiple actors.
    LikelyDuplicate,
    /// A submitted credential or document appears to have been reused from
    /// another application or sourced from an external data broker.
    DocumentReuse,
    /// Evidence values (grades, scores, dates) are inconsistent with each
    /// other or with known reference data.
    ManipulatedEvidence,
    /// Multiple applications exhibit coordinated patterns (timing, structure,
    /// IP cluster) suggesting orchestrated submission.
    CoordinatedSubmission,
    /// Catch-all for signals that do not fit the above categories.
    Other,
}

/// Current lifecycle state of a fraud signal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalStatus {
    /// Signal raised but not yet reviewed by a human.
    Flagged,
    /// Reviewer investigated and confirmed the signal is actionable.
    Confirmed,
    /// Reviewer investigated and found the signal to be a false positive.
    Dismissed,
    /// Applicant has filed an appeal; awaiting reviewer response.
    UnderAppeal,
    /// The appeal was upheld — the signal has been cleared.
    AppealUpheld,
    /// The appeal was rejected — the original signal stands.
    AppealRejected,
}

/// A fraud-signal record attached to one application.
///
/// # Key privacy constraints
/// 1. No biometric data, government IDs, or sensitive personal data may
///    appear in any field of this struct.
/// 2. `evidence_ref` must be a content-addressed identifier (IPFS CID,
///    SHA-256 hex, etc.) pointing to off-chain evidence, never inline PII.
/// 3. Signals **do not auto-reject** applications.  The downstream decision
///    system must consult `status` and `reviewer_resolution` and route the
///    application to human review.
/// 4. `risk_score` is advisory only — it must not gate any decision without
///    human sign-off.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FraudSignal {
    /// Opaque application identifier (max 64 bytes).  Must not be a wallet
    /// address or any identifier linkable to PII without additional context.
    pub app_id: String,
    /// Category of the signal.
    pub category: SignalCategory,
    /// Current lifecycle status.
    pub status: SignalStatus,
    /// Human-readable description of why this signal was raised.
    /// Max 512 bytes.  Must not contain PII.
    pub description: String,
    /// Content-addressed reference to supporting off-chain evidence.
    /// Max 256 bytes.
    pub evidence_ref: String,
    /// Advisory integer risk score in 0–100.  Higher = more suspicious.
    /// This value is informational; automated rejection based solely on
    /// this score is prohibited.
    pub risk_score: u32,
    /// Address of the service account that raised the signal.
    pub raised_by: Address,
    /// Ledger timestamp when the signal was first recorded.
    pub raised_at: u64,
    /// Address of the reviewer who last acted on this signal (None = not reviewed).
    pub reviewer: Option<Address>,
    /// Reviewer's notes on the resolution.  Max 1 024 bytes.
    pub reviewer_notes: String,
    /// Ledger timestamp of the last reviewer action (0 = not reviewed).
    pub reviewed_at: u64,
    /// Whether an appeal is currently on record.
    pub has_appeal: bool,
    /// Applicant-supplied appeal statement.  Max 1 024 bytes.
    pub appeal_statement: String,
    /// Ledger timestamp of the appeal filing (0 = no appeal).
    pub appealed_at: u64,
}

/// Summary view — excludes raw evidence_ref and internal reviewer notes to
/// limit information exposure on public queries.
#[contracttype]
#[derive(Clone, Debug)]
pub struct FraudSignalSummary {
    pub app_id: String,
    pub category: SignalCategory,
    pub status: SignalStatus,
    pub description: String,
    pub risk_score: u32,
    pub raised_at: u64,
    pub reviewed_at: u64,
    pub has_appeal: bool,
}
