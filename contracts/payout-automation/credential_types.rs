//! Shared credential types for the zero-knowledge credential presentation system.
//!
//! These types are used across `zk_verifier` and `identity` contracts to
//! support selective disclosure, predicate proofs, and credential lifecycle
//! management.

use soroban_sdk::{contracterror, contracttype, Address, BytesN, Symbol, Vec};

// ── Credential schema ───────────────────────────────────────────────────────

/// Defines the type of a claim value within a credential.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ClaimType {
    /// UTF-8 encoded string value.
    String = 0,
    /// Unsigned 64-bit integer value (used for age, dates, quantities).
    U64 = 1,
    /// Boolean flag.
    Bool = 2,
    /// Raw 32-byte value (e.g. hash commitments).
    Bytes32 = 3,
}

/// A single claim within a credential schema definition.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimDefinition {
    /// Human-readable claim key (e.g. "age", "has_prescription").
    pub key: Symbol,
    /// The expected value type for this claim.
    pub claim_type: ClaimType,
    /// Whether this claim is required in every credential of this schema.
    pub required: bool,
}

/// A credential schema defines the structure of claims a credential can hold.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialSchema {
    /// Unique identifier for this schema (hash of schema definition).
    pub schema_id: BytesN<32>,
    /// The issuer who created this schema.
    pub issuer: Address,
    /// Ordered list of claim definitions.
    pub claims: Vec<ClaimDefinition>,
    /// Schema version for forward compatibility.
    pub version: u32,
}

// ── Credential status ───────────────────────────────────────────────────────

/// Lifecycle status of an issued credential.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CredentialStatus {
    /// Credential is active and can be presented.
    Active = 0,
    /// Credential has been revoked by the issuer.
    Revoked = 1,
    /// Credential has passed its expiration time.
    Expired = 2,
    /// Credential is temporarily suspended.
    Suspended = 3,
}

// ── Credential ──────────────────────────────────────────────────────────────

/// An issued verifiable credential bound to a holder.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credential {
    /// Unique credential identifier.
    pub credential_id: BytesN<32>,
    /// Hash of the schema this credential conforms to.
    pub schema_id: BytesN<32>,
    /// The address that issued this credential.
    pub issuer: Address,
    /// The address that holds this credential.
    pub holder: Address,
    /// Ledger timestamp when the credential was issued.
    pub issued_at: u64,
    /// Ledger timestamp after which the credential is no longer valid.
    pub expires_at: u64,
    /// Current lifecycle status.
    pub status: CredentialStatus,
    /// ZK commitment to the full set of claim values (Poseidon hash).
    pub claims_commitment: BytesN<32>,
    /// Optional: credential ID of a parent credential (for chaining).
    pub parent_credential_id: BytesN<32>,
    /// Revocation registry index assigned at issuance.
    pub revocation_index: u64,
}

// ── Predicate types ─────────────────────────────────────────────────────────

/// Type of predicate proof that can be performed on a claim value.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PredicateType {
    /// Prove value > threshold (e.g. age > 18).
    GreaterThan = 0,
    /// Prove value < threshold.
    LessThan = 1,
    /// Prove lower <= value <= upper.
    InRange = 2,
    /// Prove value is a member of a predefined set.
    SetMembership = 3,
    /// Prove value != some target.
    NotEqual = 4,
}

/// A predicate proof request: prove a property of a claim without revealing it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredicateRequest {
    /// The claim key to evaluate.
    pub claim_key: Symbol,
    /// The type of predicate to prove.
    pub predicate_type: PredicateType,
    /// Threshold or comparison values encoded as 32-byte values.
    /// - GreaterThan / LessThan: single threshold value
    /// - InRange: two values (lower, upper)
    /// - SetMembership: set of allowed values
    /// - NotEqual: single value
    pub reference_values: Vec<BytesN<32>>,
}

// ── Selective disclosure ────────────────────────────────────────────────────

/// A request specifying which attributes to disclose and which predicates to prove.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisclosureRequest {
    /// Credential being presented.
    pub credential_id: BytesN<32>,
    /// Claim keys the holder agrees to reveal in plaintext.
    pub disclosed_keys: Vec<Symbol>,
    /// Predicate proofs to include (prove property without revealing value).
    pub predicates: Vec<PredicateRequest>,
    /// If true, the presentation can be linked to other presentations
    /// by the same holder (for regulatory audit).
    pub auditable: bool,
}

/// A disclosed attribute: the key and its committed value alongside a proof.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisclosedAttribute {
    /// The claim key.
    pub key: Symbol,
    /// The revealed claim value (encoded as 32 bytes).
    pub value: BytesN<32>,
    /// ZK proof that this value matches the commitment in the credential.
    pub disclosure_proof: BytesN<32>,
}

/// Result of a predicate evaluation, with proof but without revealing the value.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredicateResult {
    /// The claim key evaluated.
    pub claim_key: Symbol,
    /// The predicate that was proven.
    pub predicate_type: PredicateType,
    /// Whether the predicate holds.
    pub satisfied: bool,
    /// ZK proof that the predicate was correctly evaluated.
    pub proof: BytesN<32>,
}

// ── Credential presentation ─────────────────────────────────────────────────

/// A verifiable credential presentation with selective disclosure.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialPresentation {
    /// The credential being presented.
    pub credential_id: BytesN<32>,
    /// The schema this credential conforms to.
    pub schema_id: BytesN<32>,
    /// The holder making the presentation.
    pub holder: Address,
    /// Attributes disclosed in plaintext.
    pub disclosed_attributes: Vec<DisclosedAttribute>,
    /// Predicate proof results.
    pub predicate_results: Vec<PredicateResult>,
    /// ZK proof binding the presentation to the credential commitment.
    pub presentation_proof: BytesN<32>,
    /// Non-revocation proof (witness against the accumulator).
    pub non_revocation_proof: BytesN<32>,
    /// Linkage tag for auditable presentations (zero if not auditable).
    pub linkage_tag: BytesN<32>,
    /// Ledger timestamp of the presentation.
    pub timestamp: u64,
}

// ── Revocation types ────────────────────────────────────────────────────────

/// An accumulator-based revocation registry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevocationRegistry {
    /// Unique identifier for this registry.
    pub registry_id: BytesN<32>,
    /// The issuer who manages this registry.
    pub issuer: Address,
    /// Current accumulator value (product of all non-revoked witnesses).
    pub accumulator: BytesN<32>,
    /// Total number of credentials registered.
    pub total_issued: u64,
    /// Number of credentials currently revoked.
    pub total_revoked: u64,
    /// Ledger timestamp of the last accumulator update.
    pub last_updated: u64,
}

/// A non-revocation witness for a specific credential.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevocationWitness {
    /// The credential this witness is for.
    pub credential_id: BytesN<32>,
    /// The witness value used to prove non-revocation against the accumulator.
    pub witness: BytesN<32>,
    /// The accumulator value this witness was computed against.
    pub accumulator_at_issuance: BytesN<32>,
    /// Index in the revocation registry.
    pub index: u64,
}

// ── Batch verification ──────────────────────────────────────────────────────

/// Result of verifying a batch of credential presentations.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchVerificationResult {
    /// Total presentations in the batch.
    pub total: u32,
    /// Number of presentations that passed verification.
    pub verified: u32,
    /// Number of presentations that failed verification.
    pub failed: u32,
    /// Per-presentation result: true if verified, false if failed.
    pub results: Vec<bool>,
}

// ── Credential chaining ─────────────────────────────────────────────────────

/// A request to issue a new credential based on proof of an existing one.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChainedIssuanceRequest {
    /// The presentation proving possession of the parent credential.
    pub parent_presentation: CredentialPresentation,
    /// Schema ID for the new credential to be issued.
    pub new_schema_id: BytesN<32>,
    /// Commitment to the new credential's claims.
    pub new_claims_commitment: BytesN<32>,
    /// Requested expiration for the new credential.
    pub requested_expiry: u64,
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors specific to the credential presentation system.
///
/// Code range 200–249 to avoid collisions with CommonError (1–49)
/// and existing contract-specific errors (100+).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CredentialContractError {
    /// The credential has been revoked.
    CredentialRevoked = 200,
    /// The credential has expired.
    CredentialExpired = 201,
    /// The credential is suspended.
    CredentialSuspended = 202,
    /// The credential was not found in storage.
    CredentialNotFound = 203,
    /// The schema was not found in storage.
    SchemaNotFound = 204,
    /// The caller is not the credential issuer.
    NotIssuer = 205,
    /// The caller is not the credential holder.
    NotHolder = 206,
    /// A disclosed attribute key is not present in the credential schema.
    InvalidClaimKey = 207,
    /// A predicate proof failed verification.
    PredicateFailed = 208,
    /// The selective disclosure proof is invalid.
    InvalidDisclosureProof = 209,
    /// The non-revocation proof is invalid against the current accumulator.
    InvalidNonRevocationProof = 210,
    /// The presentation proof binding is invalid.
    InvalidPresentationProof = 211,
    /// The revocation registry was not found.
    RegistryNotFound = 212,
    /// The credential is already revoked.
    AlreadyRevoked = 213,
    /// The parent credential for chaining is invalid.
    InvalidParentCredential = 214,
    /// Duplicate credential ID.
    DuplicateCredential = 215,
    /// Duplicate schema ID.
    DuplicateSchema = 216,
    /// The credential has an invalid schema binding.
    SchemaMismatch = 217,
    /// Predicate reference values are invalid or missing.
    InvalidPredicateParams = 218,
    /// Batch is empty.
    EmptyBatch = 219,
}
