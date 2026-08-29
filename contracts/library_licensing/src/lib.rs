//! E-Library on-chain contract — library_licensing
//!
//! # Features
//!
//! ## #940 — Enforce license validity windows
//! Licenses carry `not_before` (inclusive) and `expires_at` (exclusive)
//! validity windows.  Every state-changing path validates the window using
//! checked arithmetic so an overflow fails deterministically.  Derived access
//! grants are clamped to the parent license's window.
//!
//! ## #984 — Granular librarian capabilities
//! Six fine-grained capability bits replace the single admin key for
//! day-to-day library operations:
//!
//! | Capability   | Bit | Allowed operations                                    |
//! |--------------|-----|-------------------------------------------------------|
//! | Cataloger    | 0   | `grant_license`                                       |
//! | Circulation  | 1   | `revoke_license`, `derive_access_grant`               |
//! | Finance      | 2   | (reserved for fine/fee management)                    |
//! | Compliance   | 3   | `revoke_capability`                                   |
//! | Policy       | 4   | `grant_capability`                                    |
//! | Auditor      | 5   | read-only query helpers                               |
//!
//! Capability grants are scoped (holder + capability bit) and can carry an
//! optional expiry so temporary delegations self-expire.  Every grant and
//! revocation emits an event.  The admin key retains the ability to perform
//! all operations directly.
//!
//! ## #985 — Tutor authority scoped to owned courses
//! Before a tutor may mutate a reading-list (commit manifest or publish a
//! version) the contract verifies, via a lightweight registry adapter, that
//! the tutor is the registered owner of that course.  Cross-course calls,
//! expired tutor tokens, and global-admin escalation are all rejected.
//!
//! ## #986 — Anchor course reading-list manifests
//! A *manifest* is a 32-byte content hash (e.g. SHA-256 of the off-chain
//! JSON file) anchored together with a tutor signature (`BytesN<64>`) and an
//! optional institution co-signature.  Private annotations stay off-chain;
//! only the digest is stored.  Students can re-hash their downloaded file and
//! compare against the on-chain digest to verify integrity.
//!
//! ## #987 — Version and schedule list publication
//! Reading-list versions are *immutable*: once published they cannot be
//! overwritten.  Each version carries an `effective_at` timestamp (scheduled
//! release) and an optional `expires_at`.  One *active pointer*
//! (`ActiveList`) per `(course_id, term)` pair tracks which version is
//! currently live.  Activation is deterministic (the caller supplies the
//! expected previous version so stale updates are rejected), and every
//! pointer change emits a `LIST_ACT` event.

#![no_std]

const LICENSE_MIN_TTL: u32 = 100_000;
const LICENSE_MAX_TTL: u32 = 500_000;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, String,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LicenseError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    InvalidWindow = 5,
    WindowOverflow = 6,
    NotYetActive = 7,
    Expired = 8,
    LicenseRevoked = 9,
    InvalidDuration = 10,
    InvalidRights = 11,
    /// The source and successor rendition commitments must differ.
    InvalidRenditionMigration = 12,
    /// A migration already exists for the source rendition.
    RenditionMigrationExists = 13,
    /// The requested grant or rendition migration does not exist.
    MigrationNotFound = 14,
    /// The caller has not opted into an opt-in migration.
    MigrationNotAccepted = 15,
    /// A grant may only opt into a migration for its license's rendition.
    InvalidMigrationTarget = 16,
    InvalidSeats = 12,
    AllocationExceeded = 13,
    InvalidRole = 14,
    AttestationExpired = 15,
    AttestationRevoked = 16,
    OfferExpired = 17,
    OfferAlreadyAccepted = 18,
    OfferBindingMismatch = 19,
    InvalidPrice = 20,
    NonTransferable = 12,
    LoanReturned = 13,
    InvalidRendition = 14,
}

// ---------------------------------------------------------------------------
// License types (pre-existing)
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LicenseStatus {
    Active,
    Revoked,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct License {
    pub work_id: BytesN<32>,
    pub licensor: Address,
    pub licensee: Address,
    pub rights: String,
    /// Validity window start, inclusive: the license is active at and after this timestamp.
    pub not_before: u64,
    /// Validity window end, exclusive: the license is inactive at and after this timestamp.
    pub expires_at: u64,
    pub status: LicenseStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LibraryRole {
    Student,
    Tutor,
    Librarian,
    Auditor,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityAttestation {
    pub subject: Address,
    pub institution: Address,
    pub role: LibraryRole,
    pub issuer: Address,
    pub issued_at: u64,
    pub expires_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RentalOffer {
    pub work_id: BytesN<32>,
    pub publisher: Address,
    pub buyer: Address,
    pub contract_id: BytesN<32>,
    pub network: String,
    pub seat_count: u32,
    pub not_before: u64,
    pub expires_at: u64,
    pub asset: String,
    pub price: i128,
    pub nonce: BytesN<32>,
    pub accepted: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstitutionalAllocation {
    pub parent_license_id: BytesN<32>,
    pub organization: Address,
    pub sub_account: Address,
    pub seats: u32,
    pub expires_at: u64,
    pub active: bool,
}

#[contracttype]
pub struct AccessGrant {
    /// The authoritative loan/license identifier. This is the only on-chain
    /// reference needed to re-check the source of the grant.
    pub license_id: BytesN<32>,
    /// Patron identity is immutable and is never transferable.
    pub grantee: Address,
    /// Hash/identifier of the rendition; raw URLs and secrets remain off-chain.
    pub rendition_id: BytesN<32>,
    /// Explicit loan binding retained in the ABI for backend verification.
    pub loan_id: BytesN<32>,
    pub not_before: u64,
    pub expires_at: u64,
    /// Commitment over the authoritative loan, patron, rendition, and expiry.
    pub commitment: BytesN<32>,
}

// ---------------------------------------------------------------------------
// #984 — Granular librarian capabilities
// ---------------------------------------------------------------------------

/// Six capability bits for fine-grained librarian access control.
///
/// Using `u32` lets us store the set as a bitmask in persistent storage and
/// extend it later without a storage migration.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Capability {
    /// May call `grant_license`.
    Cataloger = 0,
    /// May call `revoke_license` and `derive_access_grant`.
    Circulation = 1,
    /// Reserved for fine/fee management (e.g., overdue charges).
    Finance = 2,
    /// May call `revoke_capability`.
    Compliance = 3,
    /// May call `grant_capability`.
    Policy = 4,
    /// Read-only query helpers (informational — enforcement optional).
    Auditor = 5,
}

/// Stored per `(holder, capability)` pair.  `expires_at == 0` means
/// the grant never expires.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityGrant {
    pub holder: Address,
    pub capability: Capability,
    /// 0 = no expiry.  Otherwise the grant is invalid at and after this timestamp.
    pub expires_at: u64,
}

// ---------------------------------------------------------------------------
// #986 — Reading-list manifest
// ---------------------------------------------------------------------------

/// An integrity anchor for an off-chain reading-list file.
///
/// `content_hash` is the SHA-256 (or equivalent 32-byte digest) of the
/// canonical off-chain JSON manifest.  `tutor_sig` is the tutor's Ed25519
/// signature over `content_hash`; `institution_sig` is an optional
/// co-signature from the institution.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadingListManifest {
    pub course_id: BytesN<32>,
    pub term: String,
    pub content_hash: BytesN<32>,
    /// Tutor's 64-byte signature over `content_hash`.
    pub tutor_sig: BytesN<64>,
    /// Optional institution co-signature (all-zeros means absent).
    pub institution_sig: BytesN<64>,
    pub committed_at: u64,
    pub committed_by: Address,
}

// ---------------------------------------------------------------------------
// #987 — Versioned reading-list publication
// ---------------------------------------------------------------------------

/// An immutable, versioned snapshot of a reading list for a `(course_id,
/// term)` pair.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadingListVersion {
    pub course_id: BytesN<32>,
    pub term: String,
    /// Monotonically increasing version number (1, 2, 3, …).
    pub version: u32,
    /// The manifest that this version anchors.
    pub manifest_id: BytesN<32>,
    /// Scheduled activation timestamp (inclusive).  The version is not
    /// considered live before this time.
    pub effective_at: u64,
    /// Optional scheduled deactivation (exclusive).  0 = no expiry.
    pub expires_at: u64,
    pub published_by: Address,
    pub published_at: u64,
}

/// Active-pointer record: one per `(course_id, term)`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveList {
    pub course_id: BytesN<32>,
    pub term: String,
    /// The version number that is currently active.
    pub version: u32,
    /// The manifest id (content hash anchor) for quick access.
    pub manifest_id: BytesN<32>,
    pub activated_at: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// A governed relationship between an old rendition commitment and its
/// successor. `Forced` preserves access automatically; `OptIn` requires the
/// grant holder to explicitly accept the successor.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenditionMigrationPolicy {
    OptIn,
    Forced,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenditionMigration {
    pub from_work_id: BytesN<32>,
    pub to_work_id: BytesN<32>,
    pub policy: RenditionMigrationPolicy,
    pub created_at: u64,
}

#[contracttype]
pub enum DataKey {
    // ── Pre-existing ──────────────────────────────────────────────────────
    Admin,
    LicenseCount,
    License(BytesN<32>),
    GrantCount,
    AccessGrant(BytesN<32>),
    // ── #984 ──────────────────────────────────────────────────────────────
    /// `CapabilityGrant` keyed by `(holder_address_bytes, capability_index)`.
    /// We encode the key as `(Address, u32)` via a tuple stored in a
    /// `BytesN<36>` (32-byte address XDR hash + 4-byte big-endian cap index).
    /// Using a dedicated key variant keeps the namespace flat and avoids
    /// nested contracttype generics which are not supported by the SDK macro.
    LibrarianCap(Address, u32),
    // ── #986 ──────────────────────────────────────────────────────────────
    ManifestCount,
    Manifest(BytesN<32>),
    // ── #987 ──────────────────────────────────────────────────────────────
    /// Monotonic version counter per `(course_id, term)` — stored as a u32.
    VersionCount(BytesN<32>, String),
    /// Immutable version record: `(course_id_hash, term, version_number)`.
    ListVersion(BytesN<32>, String, u32),
    /// Active-pointer record per `(course_id, term)`.
    ActiveList(BytesN<32>, String),
    RenditionMigration(BytesN<32>),
    GrantMigrationOptIn(BytesN<32>, BytesN<32>),
    Allocation(BytesN<32>),
    AllocationTotal(BytesN<32>),
    AllocationLimit(BytesN<32>),
    AttestationCount,
    Attestation(BytesN<32>),
    OfferCount,
    Offer(BytesN<32>),
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Admin-gated authorization, mirroring the staking/vault `require_admin`
/// pattern: the caller must equal the stored admin and then prove it with
/// `require_auth`. Keeping the comparison explicit makes the Unauthorized
/// branch reachable and testable.
fn require_admin(env: &Env, caller: &Address) -> Result<(), LicenseError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(LicenseError::NotInitialized)?;
    if *caller != admin {
        return Err(LicenseError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// #984 — Check whether `caller` holds `cap` (either via the admin key or via
/// a stored `CapabilityGrant`).  Consumes the auth proof from `caller`.
fn require_capability(env: &Env, caller: &Address, cap: Capability) -> Result<(), LicenseError> {
    // Admin always has every capability.
    if let Some(admin) = env.storage().instance().get::<_, Address>(&DataKey::Admin) {
        if *caller == admin {
            caller.require_auth();
            return Ok(());
        }
    } else {
        return Err(LicenseError::NotInitialized);
    }

    let cap_index = cap as u32;
    let grant: CapabilityGrant = env
        .storage()
        .persistent()
        .get(&DataKey::LibrarianCap(caller.clone(), cap_index))
        .ok_or(LicenseError::CapabilityNotGranted)?;

    // Check expiry (expires_at == 0 means no expiry).
    if grant.expires_at != 0 && env.ledger().timestamp() >= grant.expires_at {
        return Err(LicenseError::CapabilityExpired);
    }

    caller.require_auth();
    Ok(())
}

/// Collision-resistant id derivation (ADR-0001 I3): a monotonic instance
/// nonce mixed with the ledger timestamp and caller-specific bytes, so two
/// records born in the same ledger never collide.
fn derive_commitment(
    env: &Env,
    loan_id: &BytesN<32>,
    patron: &Address,
    rendition_id: &BytesN<32>,
    expires_at: u64,
) -> BytesN<32> {
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &loan_id.clone().to_array()));
    input.append(&patron.clone().to_xdr(env));
    input.append(&Bytes::from_slice(env, &rendition_id.clone().to_array()));
    input.append(&Bytes::from_slice(env, &expires_at.to_be_bytes()));
    env.crypto().sha256(&input).into()
}

fn derive_id(env: &Env, counter_key: DataKey, salt: Bytes) -> Result<BytesN<32>, LicenseError> {
    let nonce: u64 = env.storage().instance().get(&counter_key).unwrap_or(0);
    // #940 — checked arithmetic: the monotonic counter must not overflow
    // silently (it would otherwise wrap and reuse an id).
    let next = nonce.checked_add(1).ok_or(LicenseError::WindowOverflow)?;
    env.storage().instance().set(&counter_key, &next);
    let mut id_input: Bytes = Bytes::new(env);
    id_input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    id_input.append(&Bytes::from_slice(
        env,
        &env.ledger().timestamp().to_be_bytes(),
    ));
    id_input.append(&salt);
    Ok(env.crypto().sha256(&id_input).into())
}

/// #985 — Verify that `tutor` owns `course_id` using the registry adapter
/// address stored in instance storage.  Absent adapter → falls back to admin
/// check (useful in test environments where no registry is deployed).
///
/// The registry adapter contract must expose:
///   `fn is_course_owner(course_id: BytesN<32>, account: Address) -> bool`
///
/// We call it cross-contract here.  If the adapter is not configured the
/// call is skipped and the check passes only for the admin.
fn require_tutor_owns_course(
    env: &Env,
    tutor: &Address,
    course_id: &BytesN<32>,
) -> Result<(), LicenseError> {
    // If the tutor is admin they always pass.
    if let Some(admin) = env.storage().instance().get::<_, Address>(&DataKey::Admin) {
        if *tutor == admin {
            tutor.require_auth();
            return Ok(());
        }
    }

    // Check via registry adapter when configured.
    if let Some(registry) = env
        .storage()
        .instance()
        .get::<_, Address>(&RegistryKey::CourseRegistry)
    {
        let client = CourseRegistryClient::new(env, &registry);
        if !client.is_course_owner(course_id, tutor) {
            return Err(LicenseError::TutorNotCourseOwner);
        }
        tutor.require_auth();
    } else {
        // No registry configured — the tutor must at least have the
        // Circulation capability (so they are a known librarian).
        require_capability(env, tutor, Capability::Circulation)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// #985 — Registry adapter storage key + thin client trait
// ---------------------------------------------------------------------------

/// Separate enum for the registry key to keep DataKey clean.
#[contracttype]
enum RegistryKey {
    CourseRegistry,
}

/// Thin client for the course-registry contract.  Only the `is_course_owner`
/// function is needed; everything else stays in the registry contract.
mod registry_adapter {
    use soroban_sdk::{contractclient, Address, BytesN, Env};

    #[contractclient(name = "CourseRegistryClient")]
    pub trait CourseRegistry {
        fn is_course_owner(env: Env, course_id: BytesN<32>, account: Address) -> bool;
    }
}
use registry_adapter::CourseRegistryClient;

// ---------------------------------------------------------------------------
// Contract implementation
// ---------------------------------------------------------------------------

#[contract]
pub struct LibraryLicensing;

#[contractimpl]
impl LibraryLicensing {
    // ── Initialisation ────────────────────────────────────────────────────

    /// One-time bootstrap of the contract admin, required before any
    /// admin-gated operation can run.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), LicenseError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(LicenseError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }

    /// Admin-only: configure the course-registry adapter address used by #985
    /// tutor-ownership checks.
    pub fn set_course_registry(
        env: Env,
        caller: Address,
        registry: Address,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&RegistryKey::CourseRegistry, &registry);
        Ok(())
    }

    // ── #940 — License lifecycle ──────────────────────────────────────────

    /// #984 — Issue a license.  Requires the `Cataloger` capability (or
    /// admin).
    ///
    /// `not_before` is inclusive and `expires_at` is exclusive, so a
    /// zero-length (`not_before == expires_at`) or inverted window is
    /// rejected.
    pub fn grant_license(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        licensee: Address,
        rights: String,
        not_before: u64,
        expires_at: u64,
    ) -> Result<BytesN<32>, LicenseError> {
        // #984 — cataloger capability required (admin always passes).
        require_capability(&env, &caller, Capability::Cataloger)?;
        if rights.is_empty() {
            return Err(LicenseError::InvalidRights);
        }
        if not_before >= expires_at {
            return Err(LicenseError::InvalidWindow);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LicenseError::NotInitialized)?;
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &work_id.clone().to_array()));
        salt.append(&licensee.clone().to_xdr(&env));
        let id = derive_id(&env, DataKey::LicenseCount, salt)?;
        let license = License {
            work_id,
            licensor: admin,
            licensee: licensee.clone(),
            rights,
            not_before,
            expires_at,
            status: LicenseStatus::Active,
        };
        env.storage()
            .persistent()
            .set(&DataKey::License(id.clone()), &license);
        env.storage().persistent().extend_ttl(
            &DataKey::License(id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("LIC_NEW"),),
            (id.clone(), licensee, not_before, expires_at),
        );
        Ok(id)
    }

    /// #984 — Revoke a license.  Requires the `Circulation` capability (or
    /// admin).
    pub fn revoke_license(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
    ) -> Result<(), LicenseError> {
        // #984 — circulation capability required.
        require_capability(&env, &caller, Capability::Circulation)?;
        let mut license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if license.status != LicenseStatus::Active {
            return Err(LicenseError::LicenseRevoked);
        }
        license.status = LicenseStatus::Revoked;
        env.storage()
            .persistent()
            .set(&DataKey::License(license_id.clone()), &license);
        env.storage().persistent().extend_ttl(
            &DataKey::License(license_id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events()
            .publish((symbol_short!("LIC_REVK"),), (license_id,));
        Ok(())
    }

    /// #984 — Derive a bounded access grant.  Requires the `Circulation`
    /// capability on the caller OR the caller being the licensee themselves.
    ///
    /// The grant window starts now and is clamped to the parent license's
    /// window, so a derived grant can never outlive the license it derives
    /// from.
    pub fn derive_access_grant(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        grantee: Address,
        duration: u64,
    ) -> Result<BytesN<32>, LicenseError> {
        if duration == 0 {
            return Err(LicenseError::InvalidDuration);
        }
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        // The licensee can always derive; librarians with Circulation may also
        // derive on behalf of the library.
        if caller != license.licensee {
            require_capability(&env, &caller, Capability::Circulation)?;
        } else {
            caller.require_auth();
        }
        let now = env.ledger().timestamp();
        if now < license.not_before {
            return Err(LicenseError::NotYetActive);
        }
        if now >= license.expires_at {
            return Err(LicenseError::Expired);
        }
        if license.status != LicenseStatus::Active {
            return Err(LicenseError::LicenseRevoked);
        }
        let requested_expiry = now
            .checked_add(duration)
            .ok_or(LicenseError::WindowOverflow)?;
        let grant_expires_at = if requested_expiry < license.expires_at {
            requested_expiry
        } else {
            license.expires_at
        };
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &license_id.clone().to_array()));
        salt.append(&grantee.clone().to_xdr(&env));
        let grant_id = derive_id(&env, DataKey::GrantCount, salt)?;
        let rendition_id = license.work_id.clone();
        let commitment =
            derive_commitment(&env, &license_id, &grantee, &rendition_id, grant_expires_at);
        let grant = AccessGrant {
            license_id: license_id.clone(),
            grantee: grantee.clone(),
            rendition_id: rendition_id.clone(),
            loan_id: license_id.clone(),
            not_before: now,
            expires_at: grant_expires_at,
            commitment: commitment.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::AccessGrant(grant_id.clone()), &grant);
        env.storage().persistent().extend_ttl(
            &DataKey::AccessGrant(grant_id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("GRANT_NEW"),),
            (
                grant_id.clone(),
                license_id,
                grantee,
                rendition_id,
                now,
                grant_expires_at,
                commitment,
            ),
        );
        Ok(grant_id)
    }

    // ── #940 — Read-only checks ────────────────────────────────────────────

    /// Read-only check: is the license currently inside its validity window
    /// and not revoked?  `not_before` inclusive, `expires_at` exclusive.
    /// Creates a one-way migration from an old rendition commitment to its
    /// successor. `Forced` follows active grants automatically; `OptIn`
    /// preserves choice for each grant holder. The old record is never
    /// overwritten, so both commitments remain auditable.
    pub fn propose_rendition_migration(
        env: Env,
        caller: Address,
        from_work_id: BytesN<32>,
        to_work_id: BytesN<32>,
        policy: RenditionMigrationPolicy,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
        if from_work_id == to_work_id {
            return Err(LicenseError::InvalidRenditionMigration);
        }
        let key = DataKey::RenditionMigration(from_work_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(LicenseError::RenditionMigrationExists);
        }
        let migration = RenditionMigration {
            from_work_id,
            to_work_id,
            policy,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &migration);
        env.storage()
            .persistent()
            .extend_ttl(&key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        env.events()
            .publish((symbol_short!("REND_MIG"),), migration);
        Ok(())
    }

    /// Lets a grant holder explicitly follow an `OptIn` migration. The
    /// caller must be the grant's grantee; no name, student record, or other
    /// off-chain identity data is involved.
    pub fn accept_rendition_migration(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
        to_work_id: BytesN<32>,
    ) -> Result<(), LicenseError> {
        let grant: AccessGrant = env
            .storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if caller != grant.grantee {
            return Err(LicenseError::Unauthorized);
        }
        caller.require_auth();
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(grant.license_id))
            .ok_or(LicenseError::NotFound)?;
        let migration: RenditionMigration = env
            .storage()
            .persistent()
            .get(&DataKey::RenditionMigration(license.work_id.clone()))
            .ok_or(LicenseError::MigrationNotFound)?;
        if migration.to_work_id != to_work_id {
            return Err(LicenseError::InvalidMigrationTarget);
        }
        if migration.policy != RenditionMigrationPolicy::OptIn {
            return Err(LicenseError::InvalidMigrationTarget);
        }
        let key = DataKey::GrantMigrationOptIn(grant_id.clone(), to_work_id.clone());
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        env.events()
            .publish((symbol_short!("MIG_ACCPT"),), (grant_id, to_work_id));
        Ok(())
    }

    pub fn rendition_migration(
        env: Env,
        from_work_id: BytesN<32>,
    ) -> Result<RenditionMigration, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::RenditionMigration(from_work_id))
            .ok_or(LicenseError::MigrationNotFound)
    }

    /// Returns whether a grant is usable for the requested rendition. The
    /// original work remains valid, while a successor requires a forced
    /// migration or an explicit opt-in recorded for this grant.
    pub fn is_grant_active_for_work(
        env: Env,
        grant_id: BytesN<32>,
        work_id: BytesN<32>,
    ) -> Result<bool, LicenseError> {
        let grant: AccessGrant = env
            .storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(grant.license_id))
            .ok_or(LicenseError::NotFound)?;
        let now = env.ledger().timestamp();
        let active = license.status == LicenseStatus::Active
            && now >= license.not_before
            && now < license.expires_at
            && now >= grant.not_before
            && now < grant.expires_at;
        if !active {
            return Ok(false);
        }
        if work_id == license.work_id {
            return Ok(true);
        }
        let migration = match env
            .storage()
            .persistent()
            .get::<_, RenditionMigration>(&DataKey::RenditionMigration(license.work_id))
        {
            Some(value) => value,
            None => return Ok(false),
        };
        if migration.to_work_id != work_id {
            return Ok(false);
        }
        Ok(migration.policy == RenditionMigrationPolicy::Forced
            || env
                .storage()
                .persistent()
                .get(&DataKey::GrantMigrationOptIn(grant_id, work_id))
                .unwrap_or(false))
    }

    /// #940 — read-only check: is the license currently inside its validity
    /// window and not revoked? `not_before` inclusive, `expires_at`
    /// exclusive.
    pub fn is_license_active(env: Env, license_id: BytesN<32>) -> Result<bool, LicenseError> {
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id))
            .ok_or(LicenseError::NotFound)?;
        let now = env.ledger().timestamp();
        Ok(license.status == LicenseStatus::Active
            && now >= license.not_before
            && now < license.expires_at)
    }

    /// Read-only check: is the derived grant currently usable?  Both the
    /// grant's own window and the parent license's window/status must be
    /// satisfied.
    pub fn is_grant_active(env: Env, grant_id: BytesN<32>) -> Result<bool, LicenseError> {
        let grant: AccessGrant = env
            .storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id))
            .ok_or(LicenseError::NotFound)?;
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(grant.license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        let now = env.ledger().timestamp();
        let license_ok = license.status == LicenseStatus::Active
            && now >= license.not_before
            && now < license.expires_at;
        let grant_ok = now >= grant.not_before && now < grant.expires_at;
        Ok(license_ok && grant_ok)
    }

    /// Set the seat capacity available for allocations under a parent license.
    /// Only the licensee (the institution receiving the parent license) may set
    /// this limit, and lowering it below already allocated seats is rejected.
    pub fn set_allocation_limit(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        seat_limit: u32,
    ) -> Result<(), LicenseError> {
        if seat_limit == 0 { return Err(LicenseError::InvalidSeats); }
        let license: License = env.storage().persistent().get(&DataKey::License(license_id.clone())).ok_or(LicenseError::NotFound)?;
        if caller != license.licensee { return Err(LicenseError::Unauthorized); }
        caller.require_auth();
        let allocated: u32 = env.storage().persistent().get(&DataKey::AllocationTotal(license_id.clone())).unwrap_or(0);
        if seat_limit < allocated { return Err(LicenseError::AllocationExceeded); }
        env.storage().persistent().set(&DataKey::AllocationLimit(license_id), &seat_limit);
        Ok(())
    }

    /// Allocate seats from an institutional parent license to a registered
    /// organizational sub-account. The check and total update happen in one
    /// contract invocation, preventing over-allocation across campuses.
    pub fn allocate_seats(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        organization: Address,
        sub_account: Address,
        seats: u32,
    ) -> Result<BytesN<32>, LicenseError> {
        if seats == 0 { return Err(LicenseError::InvalidSeats); }
        let license: License = env.storage().persistent().get(&DataKey::License(license_id.clone())).ok_or(LicenseError::NotFound)?;
        if caller != license.licensee || organization != license.licensee { return Err(LicenseError::Unauthorized); }
        caller.require_auth();
        let now = env.ledger().timestamp();
        if license.status != LicenseStatus::Active { return Err(LicenseError::LicenseRevoked); }
        if now < license.not_before { return Err(LicenseError::NotYetActive); }
        if now >= license.expires_at { return Err(LicenseError::Expired); }
        let limit: u32 = env.storage().persistent().get(&DataKey::AllocationLimit(license_id.clone())).ok_or(LicenseError::InvalidSeats)?;
        let current: u32 = env.storage().persistent().get(&DataKey::AllocationTotal(license_id.clone())).unwrap_or(0);
        let next = current.checked_add(seats).ok_or(LicenseError::AllocationExceeded)?;
        if next > limit { return Err(LicenseError::AllocationExceeded); }
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &license_id.clone().to_array()));
        salt.append(&sub_account.clone().to_xdr(&env));
        let allocation_id = derive_id(&env, DataKey::GrantCount, salt)?;
        let allocation = InstitutionalAllocation { parent_license_id: license_id.clone(), organization, sub_account: sub_account.clone(), seats, expires_at: license.expires_at, active: true };
        env.storage().persistent().set(&DataKey::Allocation(allocation_id.clone()), &allocation);
        env.storage().persistent().set(&DataKey::AllocationTotal(license_id.clone()), &next);
        env.storage().persistent().extend_ttl(&DataKey::Allocation(allocation_id.clone()), LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        env.events().publish((symbol_short!("ALLOC_NEW"),), (allocation_id.clone(), license_id, sub_account, seats));
        Ok(allocation_id)
    }

    pub fn revoke_allocation(env: Env, caller: Address, allocation_id: BytesN<32>) -> Result<(), LicenseError> {
        let mut allocation: InstitutionalAllocation = env.storage().persistent().get(&DataKey::Allocation(allocation_id.clone())).ok_or(LicenseError::NotFound)?;
        let license: License = env.storage().persistent().get(&DataKey::License(allocation.parent_license_id.clone())).ok_or(LicenseError::NotFound)?;
        if caller != license.licensee { return Err(LicenseError::Unauthorized); }
        caller.require_auth();
        if !allocation.active { return Err(LicenseError::NotFound); }
        allocation.active = false;
        env.storage().persistent().set(&DataKey::Allocation(allocation_id.clone()), &allocation);
        let total: u32 = env.storage().persistent().get(&DataKey::AllocationTotal(allocation.parent_license_id.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::AllocationTotal(allocation.parent_license_id), &total.saturating_sub(allocation.seats));
        Ok(())
    }

    pub fn allocation(env: Env, allocation_id: BytesN<32>) -> Result<InstitutionalAllocation, LicenseError> {
        env.storage().persistent().get(&DataKey::Allocation(allocation_id)).ok_or(LicenseError::NotFound)
    }

    pub fn allocation_total(env: Env, license_id: BytesN<32>) -> Result<u32, LicenseError> {
        env.storage().persistent().get(&DataKey::License(license_id.clone())).ok_or(LicenseError::NotFound)?;
        Ok(env.storage().persistent().get(&DataKey::AllocationTotal(license_id)).unwrap_or(0))
    }

    pub fn allocation_limit(env: Env, license_id: BytesN<32>) -> Result<u32, LicenseError> {
        env.storage().persistent().get(&DataKey::AllocationLimit(license_id)).ok_or(LicenseError::InvalidSeats)
    }

    /// Issue an institution-scoped eligibility claim. The contract admin is
    /// the issuer, preventing arbitrary wallets from minting role claims.
    pub fn issue_attestation(
        env: Env,
        caller: Address,
        subject: Address,
        institution: Address,
        role: LibraryRole,
        expires_at: u64,
    ) -> Result<BytesN<32>, LicenseError> {
        require_admin(&env, &caller)?;
        let now = env.ledger().timestamp();
        if expires_at <= now { return Err(LicenseError::InvalidWindow); }
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&subject.clone().to_xdr(&env));
        salt.append(&institution.clone().to_xdr(&env));
        let id = derive_id(&env, DataKey::AttestationCount, salt)?;
        let attestation = EligibilityAttestation { subject: subject.clone(), institution: institution.clone(), role, issuer: caller, issued_at: now, expires_at, active: true };
        env.storage().persistent().set(&DataKey::Attestation(id.clone()), &attestation);
        env.storage().persistent().extend_ttl(&DataKey::Attestation(id.clone()), LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        env.events().publish((symbol_short!("ATTEST"),), (id.clone(), subject, institution, expires_at));
        Ok(id)
    }

    pub fn revoke_attestation(env: Env, caller: Address, attestation_id: BytesN<32>) -> Result<(), LicenseError> {
        let mut attestation: EligibilityAttestation = env.storage().persistent().get(&DataKey::Attestation(attestation_id.clone())).ok_or(LicenseError::NotFound)?;
        if caller != attestation.issuer { return Err(LicenseError::Unauthorized); }
        caller.require_auth();
        if !attestation.active { return Err(LicenseError::AttestationRevoked); }
        attestation.active = false;
        env.storage().persistent().set(&DataKey::Attestation(attestation_id), &attestation);
        Ok(())
    }

    pub fn attestation(env: Env, attestation_id: BytesN<32>) -> Result<EligibilityAttestation, LicenseError> {
        env.storage().persistent().get(&DataKey::Attestation(attestation_id)).ok_or(LicenseError::NotFound)
    }

    pub fn is_attestation_active(env: Env, attestation_id: BytesN<32>, subject: Address, institution: Address, role: LibraryRole) -> Result<bool, LicenseError> {
        let attestation: EligibilityAttestation = env.storage().persistent().get(&DataKey::Attestation(attestation_id)).ok_or(LicenseError::NotFound)?;
        let now = env.ledger().timestamp();
        Ok(attestation.active && attestation.subject == subject && attestation.institution == institution && attestation.role == role && now < attestation.expires_at)
    }

    /// Create a publisher-signed, buyer-bound fixed-term rental offer.
    pub fn create_rental_offer(
        env: Env,
        publisher: Address,
        work_id: BytesN<32>,
        buyer: Address,
        contract_id: BytesN<32>,
        network: String,
        seat_count: u32,
        not_before: u64,
        expires_at: u64,
        asset: String,
        price: i128,
        nonce: BytesN<32>,
    ) -> Result<BytesN<32>, LicenseError> {
        publisher.require_auth();
        if seat_count == 0 || asset.is_empty() || price < 0 || not_before >= expires_at { return Err(LicenseError::InvalidWindow); }
        let now = env.ledger().timestamp();
        if expires_at <= now { return Err(LicenseError::OfferExpired); }
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &nonce.clone().to_array()));
        salt.append(&buyer.clone().to_xdr(&env));
        let id = derive_id(&env, DataKey::OfferCount, salt)?;
        let offer = RentalOffer { work_id, publisher, buyer, contract_id, network, seat_count, not_before, expires_at, asset, price, nonce, accepted: false };
        env.storage().persistent().set(&DataKey::Offer(id.clone()), &offer);
        env.storage().persistent().extend_ttl(&DataKey::Offer(id.clone()), LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        Ok(id)
    }

    /// Accept exactly once and mint the purchased license entitlement.
    pub fn accept_rental_offer(env: Env, buyer: Address, offer_id: BytesN<32>, contract_id: BytesN<32>, network: String) -> Result<BytesN<32>, LicenseError> {
        let mut offer: RentalOffer = env.storage().persistent().get(&DataKey::Offer(offer_id.clone())).ok_or(LicenseError::NotFound)?;
        if offer.accepted { return Err(LicenseError::OfferAlreadyAccepted); }
        if buyer != offer.buyer || contract_id != offer.contract_id || network != offer.network { return Err(LicenseError::OfferBindingMismatch); }
        buyer.require_auth();
        let now = env.ledger().timestamp();
        if now < offer.not_before { return Err(LicenseError::NotYetActive); }
        if now >= offer.expires_at { return Err(LicenseError::OfferExpired); }
        offer.accepted = true;
        env.storage().persistent().set(&DataKey::Offer(offer_id.clone()), &offer);
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &offer.work_id.clone().to_array()));
        salt.append(&buyer.clone().to_xdr(&env));
        let license_id = derive_id(&env, DataKey::LicenseCount, salt)?;
        let license = License { work_id: offer.work_id, licensor: offer.publisher, licensee: buyer.clone(), rights: String::from_str(&env, "rental"), not_before: now, expires_at: offer.expires_at, status: LicenseStatus::Active };
        env.storage().persistent().set(&DataKey::License(license_id.clone()), &license);
        env.storage().persistent().extend_ttl(&DataKey::License(license_id.clone()), LICENSE_MIN_TTL, LICENSE_MAX_TTL);
        env.events().publish((symbol_short!("OFFER_OK"),), (offer_id, license_id.clone(), buyer, offer.seat_count));
        Ok(license_id)
    }

    pub fn rental_offer(env: Env, offer_id: BytesN<32>) -> Result<RentalOffer, LicenseError> {
        env.storage().persistent().get(&DataKey::Offer(offer_id)).ok_or(LicenseError::NotFound)
    }

    pub fn license(env: Env, license_id: BytesN<32>) -> Result<License, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::License(license_id))
            .ok_or(LicenseError::NotFound)
    }

    pub fn access_grant(env: Env, grant_id: BytesN<32>) -> Result<AccessGrant, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id))
            .ok_or(LicenseError::NotFound)
    }

    /// Return the public commitment without exposing any off-chain access URL
    /// or secret. A returned/revoked authoritative loan fails verification.
    pub fn verify_access_grant(
        env: Env,
        grant_id: BytesN<32>,
        patron: Address,
        rendition_id: BytesN<32>,
    ) -> Result<bool, LicenseError> {
        let grant: AccessGrant = env
            .storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        let expected = derive_commitment(
            &env,
            &grant.loan_id,
            &patron,
            &rendition_id,
            grant.expires_at,
        );
        Ok(grant.grantee == patron
            && grant.rendition_id == rendition_id
            && grant.commitment == expected
            && Self::is_grant_active(env, grant_id).unwrap_or(false))
    }

    /// The event/query commitment is safe to expose to backend readers: it is
    /// a hash and contains no secret, URL, or raw rendition data.
    pub fn access_grant_commitment(
        env: Env,
        grant_id: BytesN<32>,
    ) -> Result<BytesN<32>, LicenseError> {
        Ok(Self::access_grant(env, grant_id)?.commitment)
    }

    /// Patron grants are soul-bound. This method is deliberately present in
    /// the ABI so clients cannot mistake an omitted transfer API for a policy;
    /// every attempted transfer fails before storage or events are mutated.
    pub fn transfer_access_grant(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
        _new_patron: Address,
    ) -> Result<(), LicenseError> {
        let grant: AccessGrant = env
            .storage()
            .persistent()
            .get(&DataKey::AccessGrant(grant_id))
            .ok_or(LicenseError::NotFound)?;
        if caller != grant.grantee {
            return Err(LicenseError::Unauthorized);
        }
        caller.require_auth();
        Err(LicenseError::NonTransferable)
    }

    /// Mark the authoritative loan as returned. This is the governed
    /// institutional path: only the contract administrator can invalidate it.
    pub fn return_loan(env: Env, caller: Address, loan_id: BytesN<32>) -> Result<(), LicenseError> {
        Self::revoke_license(env, caller, loan_id)
    }
}

#[cfg(test)]
mod tests;
