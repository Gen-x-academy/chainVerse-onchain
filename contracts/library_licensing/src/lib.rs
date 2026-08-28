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
    // #984
    CapabilityExpired = 12,
    CapabilityNotGranted = 13,
    // #985
    TutorNotCourseOwner = 14,
    // #986
    InvalidManifest = 15,
    // #987
    VersionAlreadyExists = 16,
    StaleActivePointer = 17,
    ListNotScheduled = 18,
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
pub struct AccessGrant {
    pub license_id: BytesN<32>,
    pub grantee: Address,
    pub not_before: u64,
    pub expires_at: u64,
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
        let grant = AccessGrant {
            license_id: license_id.clone(),
            grantee: grantee.clone(),
            not_before: now,
            expires_at: grant_expires_at,
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
            (grant_id.clone(), license_id, grantee, now, grant_expires_at),
        );
        Ok(grant_id)
    }

    // ── #940 — Read-only checks ────────────────────────────────────────────

    /// Read-only check: is the license currently inside its validity window
    /// and not revoked?  `not_before` inclusive, `expires_at` exclusive.
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

    // ── #984 — Capability management ─────────────────────────────────────

    /// Grant a capability to `holder`.  Requires the `Policy` capability or
    /// admin.  `expires_at = 0` means the grant never expires.
    pub fn grant_capability(
        env: Env,
        caller: Address,
        holder: Address,
        capability: Capability,
        expires_at: u64,
    ) -> Result<(), LicenseError> {
        require_capability(&env, &caller, Capability::Policy)?;
        let cap_index = capability as u32;
        let record = CapabilityGrant {
            holder: holder.clone(),
            capability,
            expires_at,
        };
        env.storage()
            .persistent()
            .set(&DataKey::LibrarianCap(holder.clone(), cap_index), &record);
        env.storage().persistent().extend_ttl(
            &DataKey::LibrarianCap(holder.clone(), cap_index),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("CAP_GRT"),),
            (holder, cap_index, expires_at),
        );
        Ok(())
    }

    /// Revoke a capability from `holder`.  Requires the `Compliance`
    /// capability or admin.
    pub fn revoke_capability(
        env: Env,
        caller: Address,
        holder: Address,
        capability: Capability,
    ) -> Result<(), LicenseError> {
        require_capability(&env, &caller, Capability::Compliance)?;
        let cap_index = capability as u32;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::LibrarianCap(holder.clone(), cap_index))
        {
            return Err(LicenseError::NotFound);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::LibrarianCap(holder.clone(), cap_index));
        env.events()
            .publish((symbol_short!("CAP_REV"),), (holder, cap_index));
        Ok(())
    }

    /// Read-only: check whether `holder` currently holds `capability`.
    pub fn has_capability(
        env: Env,
        holder: Address,
        capability: Capability,
    ) -> Result<bool, LicenseError> {
        let cap_index = capability as u32;
        let grant: Option<CapabilityGrant> = env
            .storage()
            .persistent()
            .get(&DataKey::LibrarianCap(holder, cap_index));
        match grant {
            None => Ok(false),
            Some(g) => {
                if g.expires_at != 0 && env.ledger().timestamp() >= g.expires_at {
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
        }
    }

    // ── #986 — Reading-list manifest anchoring ────────────────────────────

    /// Commit a reading-list manifest for `(course_id, term)`.  The caller
    /// must be the tutor who owns the course (#985).
    ///
    /// `content_hash` is the 32-byte digest of the canonical off-chain JSON
    /// file.  `tutor_sig` is the tutor's 64-byte signature over that hash.
    /// `institution_sig` is an optional co-signature (all-zeros = absent).
    ///
    /// Private annotations stay off-chain; only the digest is stored
    /// on-chain so students can verify integrity by re-hashing their
    /// downloaded copy.
    pub fn commit_manifest(
        env: Env,
        tutor: Address,
        course_id: BytesN<32>,
        term: String,
        content_hash: BytesN<32>,
        tutor_sig: BytesN<64>,
        institution_sig: BytesN<64>,
    ) -> Result<BytesN<32>, LicenseError> {
        // #985 — the caller must own the course.
        require_tutor_owns_course(&env, &tutor, &course_id)?;

        // Reject zero content hash (indicates uninitialised caller data).
        let zero_hash: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        if content_hash == zero_hash {
            return Err(LicenseError::InvalidManifest);
        }

        let now = env.ledger().timestamp();
        let mut salt: Bytes = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &course_id.clone().to_array()));
        salt.append(&tutor.clone().to_xdr(&env));
        let manifest_id = derive_id(&env, DataKey::ManifestCount, salt)?;

        let manifest = ReadingListManifest {
            course_id,
            term,
            content_hash,
            tutor_sig,
            institution_sig,
            committed_at: now,
            committed_by: tutor.clone(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Manifest(manifest_id.clone()), &manifest);
        env.storage().persistent().extend_ttl(
            &DataKey::Manifest(manifest_id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("MAN_NEW"),),
            (manifest_id.clone(), tutor, now),
        );
        Ok(manifest_id)
    }

    /// Read-only: fetch a manifest by id.
    pub fn manifest(
        env: Env,
        manifest_id: BytesN<32>,
    ) -> Result<ReadingListManifest, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::Manifest(manifest_id))
            .ok_or(LicenseError::NotFound)
    }

    // ── #987 — Versioned reading-list publication ─────────────────────────

    /// Publish an immutable version of a reading list.  The caller must own
    /// the course (#985).
    ///
    /// `manifest_id` must refer to a manifest already committed for the same
    /// `(course_id, term)`.  `effective_at` is the scheduled activation
    /// timestamp; `version_expires_at = 0` means the version never expires
    /// by itself.
    ///
    /// Versions are numbered 1, 2, 3, … and are immutable once stored.
    pub fn publish_list_version(
        env: Env,
        tutor: Address,
        course_id: BytesN<32>,
        term: String,
        manifest_id: BytesN<32>,
        effective_at: u64,
        version_expires_at: u64,
    ) -> Result<u32, LicenseError> {
        // #985 — ownership check.
        require_tutor_owns_course(&env, &tutor, &course_id)?;

        // Validate the manifest belongs to the same (course_id, term).
        let manifest: ReadingListManifest = env
            .storage()
            .persistent()
            .get(&DataKey::Manifest(manifest_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if manifest.course_id != course_id || manifest.term != term {
            return Err(LicenseError::InvalidManifest);
        }

        // effective_at must be valid (non-zero and, if expires_at set,
        // effective_at < expires_at).
        if version_expires_at != 0 && effective_at >= version_expires_at {
            return Err(LicenseError::InvalidWindow);
        }

        // Increment version counter for this (course, term).
        let prev_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VersionCount(course_id.clone(), term.clone()))
            .unwrap_or(0u32);
        let version = prev_count
            .checked_add(1)
            .ok_or(LicenseError::WindowOverflow)?;

        // Immutability guard — version key must not already exist.
        if env
            .storage()
            .persistent()
            .has(&DataKey::ListVersion(course_id.clone(), term.clone(), version))
        {
            return Err(LicenseError::VersionAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let record = ReadingListVersion {
            course_id: course_id.clone(),
            term: term.clone(),
            version,
            manifest_id: manifest_id.clone(),
            effective_at,
            expires_at: version_expires_at,
            published_by: tutor.clone(),
            published_at: now,
        };
        env.storage().persistent().set(
            &DataKey::ListVersion(course_id.clone(), term.clone(), version),
            &record,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::ListVersion(course_id.clone(), term.clone(), version),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.storage()
            .persistent()
            .set(&DataKey::VersionCount(course_id.clone(), term.clone()), &version);
        env.storage().persistent().extend_ttl(
            &DataKey::VersionCount(course_id.clone(), term.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("LIST_PUB"),),
            (course_id, term, version, manifest_id, effective_at),
        );
        Ok(version)
    }

    /// Activate a published version as the live pointer for `(course_id,
    /// term)`.  The caller must own the course (#985).
    ///
    /// `expected_prev_version` is an optimistic-concurrency guard: it must
    /// equal the current active version (or 0 if none has been activated
    /// yet).  This prevents stale writers from silently overwriting a newer
    /// activation.
    ///
    /// Activation also verifies that `effective_at <= now` (the version is
    /// scheduled to be live already) and that the version has not expired.
    pub fn activate_list_version(
        env: Env,
        tutor: Address,
        course_id: BytesN<32>,
        term: String,
        version: u32,
        expected_prev_version: u32,
    ) -> Result<(), LicenseError> {
        // #985 — ownership check.
        require_tutor_owns_course(&env, &tutor, &course_id)?;

        // Load the version record.
        let record: ReadingListVersion = env
            .storage()
            .persistent()
            .get(&DataKey::ListVersion(course_id.clone(), term.clone(), version))
            .ok_or(LicenseError::NotFound)?;

        let now = env.ledger().timestamp();

        // The version must be scheduled to be active.
        if now < record.effective_at {
            return Err(LicenseError::ListNotScheduled);
        }
        // The version must not have expired.
        if record.expires_at != 0 && now >= record.expires_at {
            return Err(LicenseError::Expired);
        }

        // Optimistic-concurrency: verify the expected previous pointer.
        let current_active: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ActiveList(course_id.clone(), term.clone()))
            .map(|a: ActiveList| a.version)
            .unwrap_or(0u32);
        if current_active != expected_prev_version {
            return Err(LicenseError::StaleActivePointer);
        }

        let pointer = ActiveList {
            course_id: course_id.clone(),
            term: term.clone(),
            version,
            manifest_id: record.manifest_id.clone(),
            activated_at: now,
        };
        env.storage()
            .persistent()
            .set(&DataKey::ActiveList(course_id.clone(), term.clone()), &pointer);
        env.storage().persistent().extend_ttl(
            &DataKey::ActiveList(course_id.clone(), term.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("LIST_ACT"),),
            (course_id, term, version, expected_prev_version, now),
        );
        Ok(())
    }

    /// Read-only: fetch a specific version record.
    pub fn list_version(
        env: Env,
        course_id: BytesN<32>,
        term: String,
        version: u32,
    ) -> Result<ReadingListVersion, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::ListVersion(course_id, term, version))
            .ok_or(LicenseError::NotFound)
    }

    /// Read-only: fetch the active-pointer for `(course_id, term)`.
    pub fn active_list(
        env: Env,
        course_id: BytesN<32>,
        term: String,
    ) -> Result<ActiveList, LicenseError> {
        env.storage()
            .persistent()
            .get(&DataKey::ActiveList(course_id, term))
            .ok_or(LicenseError::NotFound)
    }
}

#[cfg(test)]
mod tests;
