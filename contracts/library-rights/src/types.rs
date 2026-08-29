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
//! The registry types below (#928, #929, #932, #933) extend the same
//! boundary: [`CatalogEntry`] stores only coarse kind/parent/version
//! facts plus [`MetadataCommitment`] (a bounded URI + manifest hash) and
//! a [`ContentState`] wrapping [`ContentCommitment`] (algorithm +
//! digest) for renditions. Raw content, access URLs, and full metadata
//! documents stay off-chain; only their content-addressed pointers land
//! on-chain.

use soroban_sdk::{contracttype, Address, BytesN, String, Vec};
use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// A registered work's on-chain record (#927).
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
    /// Policy that applies to this work, which defines concurrent loan limits
    pub policy_id: Symbol,
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
/// Policy configuration that defines loan limits for patrons
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// Maximum number of concurrent active loans a single patron can have under this policy
    pub max_concurrent_loans_per_patron: u32,
    /// Total number of active loans currently active across all patrons under this policy
    pub total_active_loans: u32,
    /// Maximum total concurrent loans allowed across all patrons
    pub max_total_concurrent_loans: u32,
}

/// Reason codes for renewal denials
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RenewalDenialReason {
    /// Loan is not active
    LoanNotActive,
    /// Loan has already reached maximum renewals
    MaxRenewalsReached,
    /// Work is on hold by another patron
    WorkOnHold,
    /// Policy loan limit would be exceeded
    PolicyLimitExceeded,
    /// Patron's loan limit would be exceeded
    PatronLimitExceeded,
    /// New expiry would exceed license maximum duration
    ExceedsLicenseExpiry,
}

/// Active loan record
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LoanRecord {
    /// Work ID being loaned
    pub work_id: BytesN<32>,
    /// Patron who borrowed the work
    pub holder: Address,
    /// Timestamp when the loan was created
    pub created_at: u64,
    /// Timestamp when the loan expires
    pub expires_at: u64,
    /// Whether the loan is still active
    pub is_active: bool,
    /// Policy ID that applies to this loan
    pub policy_id: Symbol,
    /// Number of times this loan has been renewed
    pub renewal_count: u32,
    /// Whether auto-renewal is enabled for this loan
    pub auto_renew: bool,
    /// Maximum timestamp this loan can be extended to (license expiry)
    pub max_license_expiry: u64,
    /// Maximum number of renewals allowed for this loan
    pub max_renewals: u32,
}

/// Active hold record for a work that's currently loaned out
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HoldRecord {
    /// Work ID being held
    pub work_id: BytesN<32>,
    /// Patron who placed the hold
    pub holder: Address,
    /// Timestamp when the hold was created
    pub created_at: u64,
    /// Timestamp when the hold expires if not fulfilled
    pub expires_at: u64,
    /// Whether the hold is still active
    pub is_active: bool,
    /// Policy ID that applies to this hold
    pub policy_id: Symbol,
    /// Position in the queue for this work
    pub queue_position: u32,
    /// Unique nonce for idempotent operations
    pub request_nonce: BytesN<32>,
}

/// Reason codes for hold cancellation
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum HoldCancellationReason {
    /// Patron voluntarily cancelled their hold
    PatronInitiated,
    /// Librarian administratively cancelled the hold
    LibrarianInitiated,
    /// Hold expired before being fulfilled
    HoldExpired,
}

/// Tracks the number of active holds per patron per policy
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PatronPolicyActiveHolds {
    /// Count of active holds
    pub count: u32,
}

/// The three levels of the catalog hierarchy (#929).
///
/// Parent edges are strictly `Work -> Edition -> Rendition`:
/// editions attach only to works and renditions only to editions, and
/// works are always parent-less. Because each entry's kind is fixed at
/// registration and no re-parenting exists, the resulting graph is a
/// forest -- relationships can never cycle and can never cross an
/// invalid parent kind.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// An abstract work (a book, an album). Parent-less.
    Work,
    /// A published edition of a work (e.g. 2nd edition, paperback).
    Edition,
    /// A concrete digital format of an edition (e.g. EPUB, PDF, audio).
    Rendition,
}

/// Allowlisted content-hash algorithms (#932).
///
/// Only algorithms in this enum can be committed on-chain; anything else
/// is rejected at the ABI boundary, so the supported-algorithm list is
/// enforced by the type itself.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    /// SHA-256, per FIPS 180-4.
    Sha256,
    /// SHA-512, per FIPS 180-4.
    Sha512,
}

/// A bounded, content-addressed metadata commitment (#933).
///
/// Associates a catalog entry version with a metadata document that
/// lives off-chain. Only the URI (scheme- and length-validated) and the
/// hash of the manifest are stored; the manifest contents themselves are
/// never on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MetadataCommitment {
    /// Bounded content-addressed URI of the metadata manifest
    /// (e.g. `ipfs://<cid>`, `https://...`). Scheme allowlist and length
    /// bound are enforced at write time so untrusted strings can never
    /// blow past Soroban per-call budgets.
    pub uri: String,
    /// Hash of the off-chain metadata manifest the URI points at. Never
    /// the manifest itself.
    pub manifest_hash: BytesN<32>,
}

/// An algorithm-tagged content hash for a rendition (#932).
///
/// Files and access URLs stay off-chain; only the digest of the digital
/// artifact (EPUB, PDF, audio, ...) is anchored, so a delivered file can
/// later be verified against its registered artifact.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ContentCommitment {
    /// Hash algorithm used to compute `digest`.
    pub algorithm: HashAlgorithm,
    /// The content digest itself.
    pub digest: BytesN<32>,
}

/// Whether a catalog entry carries a content commitment (#932).
///
/// Works and editions have no digital artifact, so their `content` is
/// [`ContentState::None`]; renditions carry a [`ContentState::Committed`]
/// hash. Modeled as an enum (rather than `Option<ContentCommitment>`) so
/// the nested contracttype serializes cleanly with the `#[contracttype]`
/// derive.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ContentState {
    /// No content commitment (works and editions).
    None,
    /// A committed algorithm-tagged digest (renditions).
    Committed(ContentCommitment),
}

/// A canonical catalog entry (#928, #929).
///
/// The current (latest) state of a registered work, edition, or
/// rendition. Every change creates a new version; the entry always
/// mirrors the newest version, and immutable per-version snapshots live
/// under [`VersionSnapshot`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    /// Which level of the hierarchy this entry occupies.
    pub kind: EntryKind,
    /// Parent id (`None` for works, the parent work's id for editions,
    /// the parent edition's id for renditions).
    pub parent: Option<BytesN<32>>,
    /// Current version; increments on every metadata/content update.
    pub version: u32,
    /// Bounded metadata commitment for this entry version (#933).
    pub metadata: MetadataCommitment,
    /// Content commitment for renditions; `None` for works/editions
    /// (#932).
    pub content: ContentState,
    /// Pseudonymous on-chain address of the entry's custodian (#927).
    pub custodian: Address,
}

/// An immutable per-version snapshot of a catalog entry (#932, #933).
///
/// Written once when a version is created and never mutated, so a
/// specific version's metadata URI and content hash can always be
/// verified exactly as registered.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct VersionSnapshot {
    /// The 1-based version number this snapshot describes.
    pub version: u32,
    /// Metadata commitment as of this version (#933).
    pub metadata: MetadataCommitment,
    /// Content commitment as of this version, for renditions (#932).
    pub content: ContentState,
    /// Pseudonymous custodian as of this version.
    pub custodian: Address,
    /// The authorized registrant that created this version (#928).
    pub registered_by: Address,
    /// Ledger timestamp the version was registered at.
    pub registered_at: u64,
}

/// A bounded page of children ids for a parent entry (#929).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ChildrenPage {
    /// Up to `limit` child ids, starting at `cursor`.
    pub ids: Vec<BytesN<32>>,
    /// Cursor for the next page (== parent's total child count when
    /// `done` is true).
    pub next_cursor: u32,
    /// Whether this page reached the end of the child list.
    pub done: bool,
}
