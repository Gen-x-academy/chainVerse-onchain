//! Data-minimization boundary for on-chain library records (#927).
//!
//! ## Classification
//! - **Public/allowed on-chain:** content hashes (`BytesN<32>`),
//!   pseudonymous `Address` values, coarse status enums/timestamps,
//!   schema version numbers, and the provenance *type* of an acquisition.
//! - **Prohibited on-chain:** names, emails, raw book/content bytes,
//!   reading position/progress, staff notes, donor or invoice details,
//!   and any other field that identifies a person or exposes
//!   content/behavioral detail. That data belongs in the off-chain
//!   backend (chainVerse-backend #979/#983/#986) and is referenced here
//!   only by its hash where verification is needed.
//!
//! [`WorkRecord`] is intentionally minimal: it holds nothing beyond a
//! content hash and a pseudonymous custodian address. Any future field
//! added to a type in this module must be checked against the
//! prohibited list above before being merged.
//!
//! [`ClassificationCommit`] (#931) and [`ProvenanceRecord`] (#934)
//! extend the same boundary: they commit only hashes of off-chain
//! manifests/documents plus an issuer address and a schema version --
//! never the manifest contents, donor names, invoice numbers, or any
//! other private document detail.

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

/// The classification manifest families committed by the registry (#931).
///
/// - `Taxonomy`: subjects, genres, and languages of the catalog.
/// - `Audience`: audience ratings and age-band classifications.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassificationKind {
    Taxonomy,
    Audience,
}

/// An immutable classification commitment (#931).
///
/// Stores the hash of an off-chain classification manifest, the schema
/// version of that manifest, and the issuing role-holder. Corrections
/// append a new commitment (linked through [`ClassificationCommit::previous_hash`])
/// instead of rewriting the current one, so provenance of every version
/// is preserved for indexers.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ClassificationCommit {
    /// Hash of the off-chain classification manifest (e.g. subjects,
    /// genres, languages, audience ratings). Never the manifest itself.
    pub manifest_hash: BytesN<32>,
    /// Schema version of the manifest format the hash was computed over.
    pub schema_version: u32,
    /// The `PolicyManager` address that committed this manifest hash.
    pub issuer: Address,
    /// Hash of the immediately preceding commitment, or `None` for the
    /// first commit of a kind.
    pub previous_hash: Option<BytesN<32>>,
    /// Ledger timestamp the commitment was recorded at.
    pub committed_at: u64,
}

/// The acquisition channels a work's provenance can attest (#934).
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceType {
    /// Acquired through a commercial purchase.
    Purchase,
    /// Acquired through a donation.
    Donation,
    /// Acquired by an institution (library, university, consortium).
    InstitutionalAcquisition,
}

/// An append-only provenance attestation for a work (#934).
///
/// Only the hash of the off-chain provenance document (receipt, donation
/// letter, institutional agreement) is stored -- never donor or invoice
/// details. Corrections append a new record linked through
/// [`ProvenanceRecord::previous_hash`] instead of overwriting history.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProvenanceRecord {
    /// The work this provenance attestation refers to.
    pub work_id: BytesN<32>,
    /// How the work was acquired.
    pub provenance_type: ProvenanceType,
    /// Hash of the off-chain provenance document. Private document
    /// contents never land on-chain.
    pub provenance_hash: BytesN<32>,
    /// The `PolicyManager` address that attested this record.
    pub attested_by: Address,
    /// Ledger timestamp the attestation was recorded at.
    pub attested_at: u64,
    /// Hash of the immediately preceding provenance record for the same
    /// work, or `None` for the first record.
    pub previous_hash: Option<BytesN<32>>,
}
