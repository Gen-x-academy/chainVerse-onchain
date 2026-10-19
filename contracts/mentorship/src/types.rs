use soroban_sdk::{contracttype, Address, Bytes, BytesN, String};

/// Status of a mentorship engagement
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EngagementStatus {
    /// Pending guardian consent (if required)
    PendingConsent,
    /// Active engagement
    Active,
    /// Temporarily paused
    Paused,
    /// Successfully completed
    Completed,
    /// Terminated early (mutual or policy-based)
    Terminated,
    /// Terminated due to safeguarding concerns
    SafeguardingTermination,
}

/// Reason for pausing an engagement
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PauseReason {
    /// Scheduled break (e.g., vacation, exams)
    ScheduledBreak,
    /// Personal circumstances
    PersonalCircumstances,
    /// Health-related
    Health,
    /// Schedule conflict
    ScheduleConflict,
    /// Policy compliance review
    PolicyReview,
    /// Other reason (hash of off-chain details)
    Other,
}

/// Reason for terminating an engagement
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminationReason {
    /// Mutual agreement to end early
    Mutual,
    /// Completion of all objectives
    Completed,
    /// Policy violation
    PolicyViolation,
    /// Inactivity exceeding threshold
    Inactivity,
    /// Safeguarding concerns
    Safeguarding,
    /// Guardian revoked consent
    ConsentRevoked,
}

/// Guardian consent record
#[contracttype]
#[derive(Clone, Debug)]
pub struct GuardianConsent {
    /// Guardian's address
    pub guardian: Address,
    /// Mentee (protected party) address
    pub mentee: Address,
    /// Engagement ID this consent covers
    pub engagement_id: u64,
    /// Hash of consent terms (off-chain)
    pub terms_hash: BytesN<32>,
    /// Timestamp when consent was granted
    pub granted_at: u64,
    /// Optional expiry timestamp
    pub expires_at: Option<u64>,
    /// Whether consent is currently active
    pub is_active: bool,
    /// Timestamp of last material change requiring re-consent
    pub last_material_change: Option<u64>,
}

/// Pause request for an engagement
#[contracttype]
#[derive(Clone, Debug)]
pub struct PauseRequest {
    /// Who requested the pause
    pub requester: Address,
    /// Reason for pause
    pub reason: PauseReason,
    /// Hash of detailed reason (off-chain)
    pub reason_hash: BytesN<32>,
    /// When pause should take effect
    pub effective_date: u64,
    /// Optional resume date
    pub resume_date: Option<u64>,
    /// Timestamp of request
    pub requested_at: u64,
    /// Whether mutual approval is required
    pub requires_approval: bool,
    /// Whether approved (if required)
    pub is_approved: bool,
}

/// Mentorship engagement record
#[contracttype]
#[derive(Clone, Debug)]
pub struct Engagement {
    /// Unique engagement ID
    pub id: u64,
    /// Mentor address
    pub mentor: Address,
    /// Mentee address
    pub mentee: Address,
    /// Current status
    pub status: EngagementStatus,
    /// Hash of engagement agreement (off-chain)
    pub agreement_hash: BytesN<32>,
    /// Payment amount per session (in stroops)
    pub payment_per_session: i128,
    /// Token used for payment
    pub payment_token: Address,
    /// Created timestamp
    pub created_at: u64,
    /// Last activity timestamp
    pub last_activity_at: u64,
    /// Requires guardian consent
    pub requires_guardian_consent: bool,
    /// Guardian consent record (if required)
    pub guardian_consent: Option<GuardianConsent>,
    /// Current pause request (if any)
    pub active_pause: Option<PauseRequest>,
    /// Completed sessions count
    pub completed_sessions: u32,
    /// Total sessions planned
    pub planned_sessions: u32,
}

/// Feedback on matching/recommendations
#[contracttype]
#[derive(Clone, Debug)]
pub struct MatchingFeedback {
    /// Feedback ID
    pub id: u64,
    /// User providing feedback
    pub user: Address,
    /// Hash of feedback content (off-chain for privacy)
    pub feedback_hash: BytesN<32>,
    /// Type of feedback
    pub feedback_type: FeedbackType,
    /// Timestamp
    pub submitted_at: u64,
    /// Optional reference to engagement/proposal
    pub reference_id: Option<u64>,
}

/// Type of matching feedback
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedbackType {
    /// Feedback on recommendation relevance
    RecommendationRelevance,
    /// Feedback on declined proposal
    DeclinedProposal,
    /// Feedback on match quality after engagement
    MatchQuality,
    /// General platform feedback
    General,
}

/// Termination outcome record
#[contracttype]
#[derive(Clone, Debug)]
pub struct TerminationOutcome {
    /// Engagement ID
    pub engagement_id: u64,
    /// Reason for termination
    pub reason: TerminationReason,
    /// Timestamp of termination
    pub terminated_at: u64,
    /// Who initiated termination
    pub initiated_by: Address,
    /// Hash of outcome details (off-chain)
    pub outcome_hash: BytesN<32>,
    /// Final session count
    pub final_session_count: u32,
    /// Whether payment settlement is complete
    pub settlement_complete: bool,
}
