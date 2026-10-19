use soroban_sdk::{contracttype, String, Vec};

/// Aggregate counters for one cohort bucket.
///
/// # Privacy invariant
/// Results are returned only when the cohort size and every positive
/// outcome subgroup meet `k_threshold`. Individual identifiers are never stored.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CohortMetrics {
    /// Human-readable tag, e.g. "2026-q1-stem".
    pub cohort_tag: String,
    /// Total applicants ingested into this cohort.
    pub total_applicants: u64,
    /// Applicants who met eligibility criteria.
    pub eligible_count: u64,
    /// Applicants who completed the review stage.
    pub review_completed_count: u64,
    /// Applicants who received a positive decision.
    pub approved_count: u64,
    /// Applicants who received a negative decision.
    pub rejected_count: u64,
    /// Applicants who withdrew or were marked incomplete.
    pub withdrawn_count: u64,
    /// Awards actually disbursed.
    pub disbursed_count: u64,
    /// Total monetary value disbursed (in smallest token units, i128).
    pub total_disbursed_amount: i128,
    /// Ledger timestamp of the first record written to this cohort.
    pub created_at: u64,
    /// Ledger timestamp of the last update.
    pub updated_at: u64,
}

/// Per-metric definition record.  Stored separately so definitions can be
/// updated without touching the numeric counters.
#[contracttype]
#[derive(Clone, Debug)]
pub struct MetricDefinition {
    /// Short key used as the storage identifier, e.g. "eligible_count".
    pub key: String,
    /// Human-readable name of the metric.
    pub name: String,
    /// Definition and any applicable caveats, e.g. "Counts applications that
    /// passed all static eligibility checks as of the snapshot date.
    /// Does not account for manual override decisions."
    pub definition: String,
    /// Ledger timestamp of the last definition update.
    pub updated_at: u64,
}

/// Suppressed placeholder returned instead of metrics when the cohort or an
/// outcome subgroup is below the k-anonymity threshold.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SuppressedResult {
    pub cohort_tag: String,
    /// The configured suppression threshold.
    pub k_threshold: u64,
    pub reason: String,
}

/// Return type of `get_cohort_metrics` — either real data or a suppression notice.
#[contracttype]
#[derive(Clone, Debug)]
pub enum CohortResult {
    Metrics(CohortMetrics),
    Suppressed(SuppressedResult),
}

/// Supported counter fields — used as the discriminant for `increment` /
/// `decrement` calls to avoid free-form key attacks.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricField {
    TotalApplicants,
    EligibleCount,
    ReviewCompletedCount,
    ApprovedCount,
    RejectedCount,
    WithdrawnCount,
    DisbursedCount,
}

/// A batch of disbursement amounts for one cohort, submitted atomically.
/// Each element is an i128 (smallest token units).
#[contracttype]
#[derive(Clone, Debug)]
pub struct DisbursementBatch {
    pub cohort_tag: String,
    pub amounts: Vec<i128>,
}

/// Aggregate-only input for a sponsor report. The recorder must apply
/// suppression off-chain before submitting; contract calldata is public.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SponsorImpactSubmission {
    pub cohort_tag: String,
        /// Distinct recipients, deduplicated by the trusted off-chain recorder.
    pub recipient_count: u64,
    /// Count of learning enrollments or funded learning opportunities.
    pub funded_learning_count: u64,
    /// Count of learning opportunities with completion attestations.
    pub completed_count: u64,
    /// Count of credentials issued to recipients in this cohort.
    pub credentials_issued_count: u64,
    /// Distinct awards included in the total funding amount.
    pub funded_award_count: u64,
    /// Aggregate amount in the smallest token unit.
    pub total_funded_amount: i128,
}

/// How consumers must interpret the reported outcomes.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImpactInterpretation {
    DescriptiveAssociationOnly,
}

/// Immutable, public sponsor impact snapshot for one cohort.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SponsorImpactReport {
    pub cohort_tag: String,
    pub recipient_count: u64,
    pub funded_learning_count: u64,
    pub completed_count: u64,
    pub credentials_issued_count: u64,
    pub funded_award_count: u64,
    pub total_funded_amount: i128,
    pub interpretation: ImpactInterpretation,
    pub published_at: u64,
}
