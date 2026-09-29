use soroban_sdk::{contracttype, Address, Bytes, String};

// ── Risk classification ───────────────────────────────────────────────────────

/// Risk level of a rule-set change.
///
/// `High` changes (e.g. alterations to scoring weights, eligibility
/// thresholds, or protected-attribute proxies) require a second approver
/// before the version can be activated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RiskLevel {
    Standard,
    High,
}

// ── Review outcome ────────────────────────────────────────────────────────────

/// The result of a privacy / fairness review for one rule version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewOutcome {
    /// Review not yet completed.
    Pending,
    /// Reviewer approved — version is eligible for activation.
    Approved,
    /// Reviewer rejected — version must not be activated.
    Rejected,
    /// Approved with conditions recorded in `notes`.
    ApprovedWithConditions,
}

// ── Core structs ──────────────────────────────────────────────────────────────

/// A versioned snapshot of the scholarship eligibility / scoring rule-set.
///
/// `rule_payload` is an opaque byte blob whose interpretation lives off-chain
/// (e.g. a JSON document, a protobuf, or a content-addressed IPFS CID).
/// Storing the hash / CID rather than the full document is encouraged for
/// large payloads; the contract enforces a 4 096-byte ceiling.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RuleVersion {
    /// Monotonically increasing version identifier (starts at 1).
    pub version: u64,
    /// Address that proposed this version.
    pub owner: Address,
    /// Risk classification — determines approval workflow.
    pub risk_level: RiskLevel,
    /// Opaque rule payload (≤ 4 096 bytes).
    pub rule_payload: Bytes,
    /// Human-readable description of what changed and why.
    pub change_summary: String,
    /// Tag identifying the test cohort used to validate this version
    /// before promotion (e.g. "cohort-2026-q1").  Max 64 bytes.
    pub test_cohort_tag: String,
    /// Ledger timestamp at which the proposal was recorded.
    pub proposed_at: u64,
    /// Current review outcome.
    pub review_outcome: ReviewOutcome,
    /// Optional reviewer notes / conditions.
    pub review_notes: String,
    /// Ledger timestamp of the last review action (0 = not reviewed).
    pub reviewed_at: u64,
    /// Address of the reviewer (None = not reviewed).
    pub reviewer: Option<Address>,
    /// For `High` risk: optional second approver address.
    pub second_approver: Option<Address>,
    /// Whether this version is the currently active rule-set.
    pub is_active: bool,
    /// Version this change replaces (0 = no predecessor).
    pub replaces_version: u64,
}

/// Summary view returned by `get_rule_version` — safe for public queries.
/// Does not expose reviewer notes that might leak sensitive deliberation.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RuleVersionSummary {
    pub version: u64,
    pub owner: Address,
    pub risk_level: RiskLevel,
    pub change_summary: String,
    pub test_cohort_tag: String,
    pub proposed_at: u64,
    pub review_outcome: ReviewOutcome,
    pub reviewed_at: u64,
    pub is_active: bool,
    pub replaces_version: u64,
}
