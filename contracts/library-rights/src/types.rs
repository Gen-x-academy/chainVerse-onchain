//! Data-minimization boundary for on-chain library records (#927).
//!
//! ## Classification
//! - **Public/allowed on-chain:** content hashes (`BytesN<32>`),
//!   pseudonymous `Address` values, coarse status enums/timestamps.
//! - **Prohibited on-chain:** names, emails, raw book/content bytes,
//!   reading position/progress, or staff notes -- or any other field
//!   that identifies a person or exposes content/behavioral detail.
//!   That data belongs in the off-chain backend (chainVerse-backend
//!   #979) and is referenced here only by its hash where verification
//!   is needed.
//!
//! [`WorkRecord`] is intentionally minimal: it holds nothing beyond a
//! content hash and a pseudonymous custodian address. Any future field
//! added to a type in this module must be checked against the
//! prohibited list above before being merged.

use soroban_sdk::{contracttype, Address, BytesN};

/// A registered work's on-chain record.
///
/// Contains only a content hash and a pseudonymous custodian address --
/// never a name, email, raw content, reading position, or staff note.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    /// Hash of the off-chain work content. Never the content itself.
    pub work_hash: BytesN<32>,
    /// Pseudonymous on-chain address of the work's current custodian.
    pub custodian: Address,
}

/// Access state for a content commitment. Quarantine is intentionally
/// separate from legal takedown and ordinary deactivation.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentStatus {
    Active,
    Deactivated,
    LegalTakedown,
    Quarantined,
}

/// Forensic evidence for an emergency quarantine. Only a hash of the reason
/// or incident record is stored on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct QuarantineRecord {
    pub reason_hash: BytesN<32>,
    pub quarantined_at: u64,
    pub quarantined_by: Address,
    pub restored_at: Option<u64>,
    pub restoration_review_hash: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MembershipStatus {
    Active,
    Revoked,
}

/// A membership attestation contains only opaque commitments and timestamps;
/// it never stores a name, student number, or plaintext institutional claim.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MembershipAttestation {
    pub wallet: Address,
    pub claim_commitment: BytesN<32>,
    pub institution_domain_hash: BytesN<32>,
    pub network_id: BytesN<32>,
    pub nonce: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub status: MembershipStatus,
}
