use soroban_sdk::{Address, Bytes};

/// Represents the availability status of a mentor
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AvailabilityStatus {
    Available = 0,
    Booked = 1,
    Archived = 2,
}

/// Represents different delivery modes for mentorship
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeliveryMode {
    LiveOneOnOne = 0,
    Async = 1,
    Hybrid = 2,
}

/// Time window for mentor availability (in UTC)
#[derive(Clone)]
pub struct TimeWindow {
    /// Start hour (0-23)
    pub start_hour: u32,
    /// End hour (0-23)
    pub end_hour: u32,
    /// Days of week (bitmap: bit 0 = Monday, bit 6 = Sunday)
    pub days_of_week: u32,
}

/// Core mentor profile stored on-chain
#[derive(Clone)]
pub struct MentorProfile {
    /// Mentor's Soroban address
    pub mentor_id: Address,
    /// Hash of expertise metadata (off-chain storage)
    pub expertise_hash: Bytes,
    /// Current availability status
    pub availability_status: AvailabilityStatus,
    /// Primary timezone offset from UTC (in hours, e.g., -5 for EST)
    pub timezone_offset: i32,
    /// Bitmask of supported delivery modes
    pub delivery_modes: u32,
    /// Hourly rate in stroops (Stellar microunits)
    pub rate_per_hour: u128,
    /// Total number of ratings received
    pub rating_count: u32,
    /// Sum of all rating scores (divide by rating_count for average)
    pub rating_sum: u32,
    /// Derived reputation score (0-100)
    pub reputation_score: u32,
    /// Hash of public attestations/endorsements (off-chain)
    pub public_attestation_hash: Bytes,
    /// Hash of engagement policies (off-chain)
    pub policy_hash: Bytes,
    /// Ledger sequence when profile was created
    pub created_at: u32,
    /// Ledger sequence when profile was last updated
    pub updated_at: u32,
}

/// Lightweight directory entry for search indexing
#[derive(Clone)]
pub struct DirectoryEntry {
    /// Mentor's address
    pub mentor_id: Address,
    /// Expertise tag identifiers (e.g., "solidity", "rust", "dapp-architecture")
    pub expertise_tags: u32, // Bitmask for common tags, can be extended
    /// Languages supported (bitmask: bit 0 = English, bit 1 = Spanish, etc.)
    pub languages: u32,
    /// Timezone offset for availability overlap calculation
    pub timezone_offset: i32,
    /// Bitmask of delivery modes
    pub delivery_modes: u32,
    /// Price band category (0 = low, 1 = mid, 2 = high)
    pub price_band: u32,
    /// Rating band category (0 = new, 1 = good, 2 = excellent)
    pub rating_band: u32,
    /// Whether mentor is currently accepting new mentees
    pub availability_flag: bool,
    /// Ledger sequence of last update
    pub last_updated: u32,
}

/// Rating submission from backend
#[derive(Clone)]
pub struct RatingSubmission {
    /// Mentor being rated
    pub mentor_id: Address,
    /// Rating score (1-5)
    pub score: u32,
    /// Hash of review content (off-chain)
    pub review_hash: Bytes,
    /// Session or engagement reference hash
    pub engagement_hash: Bytes,
}
