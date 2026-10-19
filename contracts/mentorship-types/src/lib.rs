#![no_std]

//! Canonical, privacy-minimized vocabulary shared by mentorship contracts.
//!
//! These types define identifiers and lifecycle states, not workflow storage
//! or authorization. Content and evidence remain off-chain; hashes and public
//! attestations are the only content-related values intended for the ledger.

use soroban_sdk::{contracttype, Address, BytesN};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorId(pub Address);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenteeId(pub Address);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipProgramId(pub BytesN<32>);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngagementId(pub BytesN<32>);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionId(pub BytesN<32>);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliverableId(pub BytesN<32>);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentId(pub BytesN<32>);

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeId(pub BytesN<32>);

/// A deployed Soroban token contract, rather than a ticker or display name.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrencyId(pub Address);

/// Token units in the currency contract's smallest denomination.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenAmount(pub i128);

impl TokenAmount {
    pub fn is_non_negative(self) -> bool {
        self.0 >= 0
    }
}

/// Opaque commitment to private content held outside the ledger.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentCommitment(pub BytesN<32>);

/// Public proof that an actor attested to a committed off-chain artifact.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicAttestation {
    pub actor: Address,
    pub commitment: ContentCommitment,
    pub attested_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantRole {
    Mentee = 1,
    Mentor = 2,
    Tutor = 3,
    Guardian = 4,
    OrganizationStaff = 5,
    Moderator = 6,
    FinanceStaff = 7,
    Administrator = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatus {
    Draft = 1,
    Published = 2,
    Paused = 3,
    Closed = 4,
}

impl ProgramStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Published)
                | (Self::Published, Self::Paused)
                | (Self::Published, Self::Closed)
                | (Self::Paused, Self::Published)
                | (Self::Paused, Self::Closed)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngagementStatus {
    Requested = 1,
    Accepted = 2,
    Active = 3,
    Completed = 4,
    Cancelled = 5,
    Disputed = 6,
}

impl EngagementStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Requested, Self::Accepted)
                | (Self::Requested, Self::Cancelled)
                | (Self::Accepted, Self::Active)
                | (Self::Accepted, Self::Cancelled)
                | (Self::Active, Self::Completed)
                | (Self::Active, Self::Cancelled)
                | (Self::Active, Self::Disputed)
                | (Self::Disputed, Self::Completed)
                | (Self::Disputed, Self::Cancelled)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Scheduled = 1,
    InProgress = 2,
    EvidenceSubmitted = 3,
    Confirmed = 4,
    Cancelled = 5,
    Disputed = 6,
}

impl SessionStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Scheduled, Self::InProgress)
                | (Self::Scheduled, Self::Cancelled)
                | (Self::InProgress, Self::EvidenceSubmitted)
                | (Self::InProgress, Self::Cancelled)
                | (Self::InProgress, Self::Disputed)
                | (Self::EvidenceSubmitted, Self::Confirmed)
                | (Self::EvidenceSubmitted, Self::Disputed)
                | (Self::Disputed, Self::Confirmed)
                | (Self::Disputed, Self::Cancelled)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliverableStatus {
    Draft = 1,
    Submitted = 2,
    RevisionRequested = 3,
    Accepted = 4,
    Rejected = 5,
    Disputed = 6,
    Cancelled = 7,
}

impl DeliverableStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Submitted)
                | (Self::Submitted, Self::Accepted)
                | (Self::Submitted, Self::RevisionRequested)
                | (Self::Submitted, Self::Rejected)
                | (Self::Submitted, Self::Disputed)
                | (Self::RevisionRequested, Self::Submitted)
                | (Self::RevisionRequested, Self::Rejected)
                | (Self::RevisionRequested, Self::Cancelled)
                | (Self::Disputed, Self::Accepted)
                | (Self::Disputed, Self::Rejected)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaymentStatus {
    PendingFunding = 1,
    Funded = 2,
    PartiallyReleased = 3,
    Released = 4,
    Refunded = 5,
    Disputed = 6,
    Cancelled = 7,
}

impl PaymentStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::PendingFunding, Self::Funded)
                | (Self::PendingFunding, Self::Cancelled)
                | (Self::Funded, Self::PartiallyReleased)
                | (Self::Funded, Self::Released)
                | (Self::Funded, Self::Refunded)
                | (Self::Funded, Self::Disputed)
                | (Self::PartiallyReleased, Self::PartiallyReleased)
                | (Self::PartiallyReleased, Self::Released)
                | (Self::PartiallyReleased, Self::Refunded)
                | (Self::PartiallyReleased, Self::Disputed)
                | (Self::Disputed, Self::PartiallyReleased)
                | (Self::Disputed, Self::Released)
                | (Self::Disputed, Self::Refunded)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Open = 1,
    UnderReview = 2,
    Resolved = 3,
    Withdrawn = 4,
}

impl DisputeStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Open, Self::UnderReview)
                | (Self::Open, Self::Withdrawn)
                | (Self::UnderReview, Self::Resolved)
                | (Self::UnderReview, Self::Withdrawn)
        )
    }
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeOutcome {
    MentorFavored = 1,
    MenteeFavored = 2,
    Split = 3,
    NoFault = 4,
}

#[cfg(test)]
mod tests {
    use super::{
        DeliverableStatus, DisputeStatus, EngagementStatus, PaymentStatus, ProgramStatus,
        SessionStatus, TokenAmount,
    };

    #[test]
    fn accepted_lifecycle_edges_and_terminal_states_are_enforced() {
        assert!(ProgramStatus::Draft.can_transition_to(ProgramStatus::Published));
        assert!(ProgramStatus::Paused.can_transition_to(ProgramStatus::Closed));
        assert!(!ProgramStatus::Closed.can_transition_to(ProgramStatus::Published));

        assert!(EngagementStatus::Requested.can_transition_to(EngagementStatus::Accepted));
        assert!(EngagementStatus::Active.can_transition_to(EngagementStatus::Disputed));
        assert!(!EngagementStatus::Completed.can_transition_to(EngagementStatus::Active));

        assert!(SessionStatus::EvidenceSubmitted.can_transition_to(SessionStatus::Confirmed));
        assert!(!SessionStatus::Confirmed.can_transition_to(SessionStatus::Disputed));

        assert!(DeliverableStatus::Submitted.can_transition_to(DeliverableStatus::Accepted));
        assert!(DeliverableStatus::RevisionRequested.can_transition_to(DeliverableStatus::Submitted));
        assert!(!DeliverableStatus::Accepted.can_transition_to(DeliverableStatus::Submitted));

        assert!(PaymentStatus::Funded.can_transition_to(PaymentStatus::Disputed));
        assert!(PaymentStatus::Disputed.can_transition_to(PaymentStatus::Refunded));
        assert!(!PaymentStatus::Released.can_transition_to(PaymentStatus::Refunded));

        assert!(DisputeStatus::Open.can_transition_to(DisputeStatus::UnderReview));
        assert!(!DisputeStatus::Resolved.can_transition_to(DisputeStatus::Open));

        assert!(TokenAmount(0).is_non_negative());
        assert!(!TokenAmount(-1).is_non_negative());
    }
}