use soroban_sdk::{contracttype, Address, BytesN};

/// Eligibility status for a participant in a program.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EligibilityStatus {
    /// Participant is eligible to take part.
    Eligible = 0,
    /// Participant is explicitly ineligible.
    Ineligible = 1,
    /// Participant is currently suspended.
    Suspended = 2,
    /// Participant's eligibility is pending review.
    Pending = 3,
}

/// Reason code for ineligibility or suspension.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EligibilityReason {
    /// Participant lacks required credentials or qualifications.
    MissingCredentials = 0,
    /// Participant failed vetting.
    FailedVetting = 1,
    /// Participant was suspended due to violation.
    SuspendedForViolation = 2,
    /// Participant did not meet program-specific requirements.
    FailedRequirements = 3,
}

/// Complete eligibility record for a participant in a program.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityRecord {
    pub participant: Address,
    pub program_id: BytesN<32>,
    pub status: EligibilityStatus,
    /// Hash of detailed reason/documentation (off-chain).
    pub reason_hash: BytesN<32>,
    /// When the eligibility determination expires.
    pub expires_at: u64,
    /// When the record was last updated.
    pub updated_at: u64,
}
