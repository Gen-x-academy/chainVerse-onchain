#![no_std]

use soroban_sdk::{Address, BytesN, Env, String, Vec};

/// Delivery mode for mentorship sessions
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DeliveryMode {
    Synchronous = 0,
    Asynchronous = 1,
    Hybrid = 2,
}

/// On-chain mentor profile (privacy-minimized)
#[derive(Clone)]
pub struct MentorProfile {
    pub mentor_id: Address,
    pub expertise_tags_hash: BytesN<32>, // Hash of expertise list (off-chain)
    pub availability_mask: u32,           // Bitmask of 24 UTC hours
    pub delivery_modes: u8,               // Bitmask: bit 0=sync, 1=async, 2=hybrid
    pub base_price: u64,                  // Stroops per hour
    pub active: bool,
    pub profile_uri: String, // IPFS/Arweave reference
    pub created_at: u64,
    pub updated_at: u64,
}

/// Mentor completion/trust signal
#[derive(Clone)]
pub struct CompletionRecord {
    pub mentee: Address,
    pub program_id: u32,
    pub rating: u8, // 1-5 stars
    pub completed_at: u64,
}

/// Program summary (privacy-minimized)
#[derive(Clone)]
pub struct ProgramSummary {
    pub program_id: u32,
    pub organization: Address,
    pub topic_hash: BytesN<32>, // Hash of topic (off-chain)
    pub cohort_start: u64,
    pub duration_hours: u32,
    pub max_seats: u32,
    pub available_seats: u32,
    pub mode: DeliveryMode,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Mentor comparison result
#[derive(Clone)]
pub struct MentorComparison {
    pub mentor_id: Address,
    pub price: u64,
    pub avg_rating: u8,      // 0-5 (0 if no ratings)
    pub completion_count: u32,
    pub program_fit_score: u8, // 0-100
    pub availability_match: bool,
    pub mode_match: bool,
}

/// Mentee profile for matching
#[derive(Clone)]
pub struct MenteeProfile {
    pub goal_hash: BytesN<32>,        // Hash of goal (off-chain)
    pub expertise_level: u8,          // 0-100
    pub language_preferences: Vec<u8>, // ISO 639-1 codes
    pub available_hours: u32,         // Bitmask of 24 UTC hours
    pub preferred_mode: DeliveryMode,
}

/// Match reason enum
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MatchReason {
    ExpertiseMatch = 0,
    AvailabilityMatch = 1,
    PriceMatch = 2,
    HighRating = 3,
    LanguageMatch = 4,
    ModeMatch = 5,
}

/// Recommendation result
#[derive(Clone)]
pub struct Recommendation {
    pub mentor_id: Address,
    pub score: u32, // 0-10000 (0-100.00%)
    pub reasons: Vec<MatchReason>,
}

/// Mentor public projection (anti-enumeration)
#[derive(Clone)]
pub struct MentorPublicProjection {
    pub mentor_id: Address,
    pub availability_match: bool,
    pub price_bucket: PriceBucket, // Anonymized price
    pub has_verified_badge: bool,
    pub completion_count_range: u8, // 0: none, 1: 1-10, 2: 11-50, 3: 50+
}

/// Anonymized price range
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PriceBucket {
    Free = 0,
    Low = 1,      // < 50 stroops
    Medium = 2,   // 50-150 stroops
    High = 3,     // > 150 stroops
}

impl PriceBucket {
    pub fn from_price(price: u64) -> Self {
        match price {
            0 => PriceBucket::Free,
            1..=50 => PriceBucket::Low,
            51..=150 => PriceBucket::Medium,
            _ => PriceBucket::High,
        }
    }
}

/// Abuse signals for rate limiting
#[derive(Clone)]
pub struct AbuseSignals {
    pub rapid_queries: bool,
    pub enumeration_pattern: bool,
    pub cache_bypass_attempts: bool,
    pub risk_score: u8, // 0-100
}

/// Rate limit configuration
#[derive(Clone)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window_seconds: u32,
}

/// Contract error types
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MentorError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidInput = 4,
    MentorNotFound = 5,
    ProgramNotFound = 6,
    RateLimitExceeded = 7,
    AbuseSignalDetected = 8,
    InvalidDeliveryMode = 9,
    InvalidPrice = 10,
    InvalidAvailability = 11,
    ProgramFull = 12,
    InvalidProgram = 13,
}

/// Query filters for program discovery
#[derive(Clone)]
pub struct QueryFilters {
    pub topic_hash: Option<BytesN<32>>,
    pub min_seats: u32,
    pub mode: Option<DeliveryMode>,
    pub organization: Option<Address>,
    pub cohort_start_min: Option<u64>,
    pub cohort_start_max: Option<u64>,
}
