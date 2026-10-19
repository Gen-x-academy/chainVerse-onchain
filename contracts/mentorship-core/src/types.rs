use soroban_sdk::{contracttype, Address, BytesN, String, Symbol};

/// The explicit mentorship lifecycle. Transitions are one-directional:
/// Proposed → ProposedMenteeAccepted → ProposedMentorAccepted → Active → Completed
/// or at any point before Active: Rejected
/// or from Active: Terminated
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MentorshipStatus {
    /// Newly proposed, awaiting mentee acceptance.
    Proposed,
    /// Mentee has signed, awaiting mentor acceptance.
    ProposedMenteeAccepted,
    /// Both have signed, awaiting admin activation.
    ProposedMentorAccepted,
    /// Active mentorship, payments may settle.
    Active,
    /// Completed successfully.
    Completed,
    /// Rejected before activation (either party or admin).
    Rejected,
    /// Terminated after activation (admin only).
    Terminated,
}

/// A mentorship match record. Immutable identity (id, mentor, mentee, program_id);
/// mutable state (status, timestamps). Follows the privacy-minimized pattern: full
/// terms and declarations are off-chain; only hashes stored on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipMatch {
    /// Stable, immutable identifier set at creation.
    pub id: BytesN<32>,
    /// Program to which this mentorship belongs.
    pub program_id: BytesN<32>,
    /// The mentor (service provider).
    pub mentor: Address,
    /// The mentee (service recipient).
    pub mentee: Address,
    /// Award amount in currency's smallest unit.
    pub amount: i128,
    pub currency: Symbol,
    /// Version of terms accepted.
    pub terms_version: u32,
    /// Integrity commitment over off-chain terms.
    pub terms_hash: BytesN<32>,
    /// Human-readable title (bounded length).
    pub title: String,
    /// Start date (unix timestamp).
    pub start_date: u64,
    /// End date (unix timestamp).
    pub end_date: u64,
    /// Current state of the match.
    pub status: MentorshipStatus,
    /// When the proposal was created.
    pub proposed_at: u64,
    /// When mentee accepted (0 if not yet accepted).
    pub mentee_accepted_at: u64,
    /// When mentor accepted (0 if not yet accepted).
    pub mentor_accepted_at: u64,
    /// When admin activated (0 if not yet active).
    pub activated_at: u64,
    /// When marked completed (0 if not terminal).
    pub completed_at: u64,
    /// When marked rejected or terminated (0 if still pending/active).
    pub terminated_at: u64,
    /// Reason code for rejection or termination.
    pub termination_reason_code: u32,
    /// True while this match holds a reservation in the capacity contract.
    pub reserved: bool,
}

/// Signed acceptance record for the mentee. Captures the exact terms version
/// and hash at the moment of acceptance, binding the agreement to what was offered.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenteeAcceptance {
    pub match_id: BytesN<32>,
    pub terms_version: u32,
    pub terms_hash: BytesN<32>,
    /// The address that signed (always the match's mentee).
    pub signer: Address,
    pub accepted_at: u64,
    /// Integrity commitment over off-chain declarations/pledges.
    pub declarations_hash: BytesN<32>,
}

/// Signed acceptance record for the mentor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorAcceptance {
    pub match_id: BytesN<32>,
    pub terms_version: u32,
    pub terms_hash: BytesN<32>,
    /// The address that signed (always the match's mentor).
    pub signer: Address,
    pub accepted_at: u64,
    /// Integrity commitment over off-chain declarations/pledges.
    pub declarations_hash: BytesN<32>,
}
