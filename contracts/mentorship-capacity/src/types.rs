use soroban_sdk::{contracttype, Address, BytesN};

/// Capacity limits for a single mentor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorCapacityConfig {
    /// Maximum concurrent active mentorships.
    pub max_concurrent: u32,
    /// Maximum hours per week.
    pub max_weekly_hours: u32,
    /// Maximum mentorships per program.
    pub max_per_program: u32,
    /// Maximum mentorships per cohort.
    pub max_per_cohort: u32,
    /// Reserve slots for pending proposals.
    pub proposal_buffer: u32,
}

/// Current capacity utilization for a mentor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorCapacityUtilization {
    pub mentor: Address,
    /// Number of currently active mentorships.
    pub current_concurrent: u32,
    /// Current week (unix timestamp / 604800).
    pub current_week: u32,
    /// Hours used this week.
    pub hours_this_week: u32,
    /// Last updated timestamp.
    pub updated_at: u64,
}

/// Per-program capacity limits.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCapacityConfig {
    pub program_id: BytesN<32>,
    /// Maximum mentorships in this program.
    pub max_mentorships: u32,
    /// Maximum mentors in this program.
    pub max_mentors: u32,
    /// Current count of active mentorships.
    pub active_count: u32,
}
