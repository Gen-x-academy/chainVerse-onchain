use soroban_sdk::{contracttype, Address, Bytes, BytesN, String};

/// Represents a mentorship agreement
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Agreement {
    /// Unique agreement identifier
    pub id: u64,
    /// Mentor address
    pub mentor: Address,
    /// Mentee address
    pub mentee: Address,
    /// Hash of the program details (stored off-chain)
    pub program_hash: BytesN<32>,
    /// Hash of goals and scope (stored off-chain)
    pub scope_hash: BytesN<32>,
    /// Start timestamp
    pub start_time: u64,
    /// End timestamp
    pub end_time: u64,
    /// Session cadence in seconds (e.g., weekly = 604800)
    pub cadence_seconds: u64,
    /// Price per session in stroops
    pub price_per_session: i128,
    /// Payment token address
    pub token: Address,
    /// Hash of cancellation policy (stored off-chain)
    pub cancellation_policy_hash: BytesN<32>,
    /// Hash of code of conduct (stored off-chain)
    pub conduct_hash: BytesN<32>,
    /// Hash of evidence requirements (stored off-chain)
    pub evidence_hash: BytesN<32>,
    /// Version of terms accepted
    pub terms_version: u32,
    /// Current agreement status
    pub status: AgreementStatus,
    /// Creation timestamp
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Agreement lifecycle status
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgreementStatus {
    /// Created, awaiting acceptance
    Pending = 0,
    /// Both parties accepted
    Active = 1,
    /// Agreement completed successfully
    Completed = 2,
    /// Agreement cancelled
    Cancelled = 3,
    /// Agreement suspended (can be resumed)
    Suspended = 4,
}

/// Participant acceptance record
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acceptance {
    /// Address of accepting party
    pub actor: Address,
    /// Agreement ID
    pub agreement_id: u64,
    /// Hash of agreement at time of acceptance
    pub agreement_hash: BytesN<32>,
    /// Terms version accepted
    pub terms_version: u32,
    /// Acceptance timestamp
    pub timestamp: u64,
    /// Nonce for replay protection
    pub nonce: u64,
}

/// Amendment proposal
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Amendment {
    /// Amendment identifier
    pub id: u64,
    /// Agreement being amended
    pub agreement_id: u64,
    /// Proposer address
    pub proposer: Address,
    /// New cadence (None = no change)
    pub new_cadence: Option<u64>,
    /// New scope hash (None = no change)
    pub new_scope_hash: Option<BytesN<32>>,
    /// New start time (None = no change)
    pub new_start_time: Option<u64>,
    /// New end time (None = no change)
    pub new_end_time: Option<u64>,
    /// New price per session (None = no change)
    pub new_price: Option<i128>,
    /// New cancellation policy hash (None = no change)
    pub new_cancellation_hash: Option<BytesN<32>>,
    /// Impact description hash (stored off-chain)
    pub impact_hash: BytesN<32>,
    /// Proposed at timestamp
    pub proposed_at: u64,
    /// Amendment status
    pub status: AmendmentStatus,
    /// Mentor approved
    pub mentor_approved: bool,
    /// Mentee approved
    pub mentee_approved: bool,
    /// Activated at timestamp (if activated)
    pub activated_at: Option<u64>,
}

/// Amendment lifecycle status
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmendmentStatus {
    /// Proposed, awaiting approval
    Proposed = 0,
    /// All parties approved, ready to activate
    Approved = 1,
    /// Amendment activated, changes applied
    Activated = 2,
    /// Amendment rejected by at least one party
    Rejected = 3,
    /// Amendment expired before activation
    Expired = 4,
}

/// Block relationship between users (MENTOR-044)
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    /// User who initiated the block
    pub blocker: Address,
    /// User being blocked
    pub blocked: Address,
    /// Timestamp when block was created
    pub blocked_at: u64,
    /// Optional reason hash (stored off-chain for privacy)
    pub reason_hash: Option<BytesN<32>>,
}

/// Represents whether a match is allowed between two users
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchingAllowed {
    Allowed = 0,
    BlockedByFirst = 1,
    BlockedBySecond = 2,
    MutualBlock = 3,
}
