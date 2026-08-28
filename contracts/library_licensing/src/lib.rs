//! #940 — Enforce license validity windows.
//!
//! E-library licenses carry an explicit validity window: `not_before`
//! (inclusive) and `expires_at` (exclusive). Every state-changing path
//! validates the window, and any arithmetic performed on timestamps uses
//! checked arithmetic so an overflow fails the call deterministically
//! instead of wrapping around (the release profile has `overflow-checks`
//! on, which would otherwise panic on an overflow).
//!
//! Derived access grants are clamped to the parent license's window, so a
//! sub-grant can never start before or outlive the rights it derives from.
//! Granting a license is admin-gated (mirrors the vault `require_admin`
//! pattern); deriving a grant is gated on the licensee.
//!
//! #942 — Per-format entitlements.
//!
//! Each license can carry entitlements bound to an explicit rendition id
//! (e.g. `EPUB`, `PDF`, `AUDIO`) and an allowed access mode
//! ([`AccessMode::Borrow`] or [`AccessMode::AccessibleAlternative`]).
//! Entitlements are queried by exact `(rendition_id, access_mode)` pair,
//! so borrowing one format can never unlock another, while accessible
//! alternatives can be granted intentionally. The entitlement mapping is
//! queryable within bounds via [`LibraryLicensing::entitlements`].
//!
//! #943 — Concurrent digital seats.
//!
//! Licenses also track a `total_seats` budget with an `allocated_seats`
//! counter. Allocation uses checked arithmetic and can never exceed the
//! supply (competing calls fail with `NoSeatsAvailable`); release
//! restores exactly one seat. The `allocated <= total` invariant holds on
//! every lifecycle path: issuance starts at zero, allocation is rejected
//! once the budget is exhausted or the license is not active, and release
//! is rejected when nothing is allocated.

#![no_std]

const LICENSE_MIN_TTL: u32 = 100_000;
const LICENSE_MAX_TTL: u32 = 500_000;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, xdr::ToXdr, Address,
    Bytes, BytesN, Env, String, Symbol, Vec,
};

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
    /// #943 — a license must reserve at least one concurrent seat.
    InvalidSeats = 12,
    /// #943 — every seat of the license is currently allocated.
    NoSeatsAvailable = 13,
    /// #943 — release was attempted while no seat was allocated.
    NoSeatsAllocated = 14,
    /// #942 — no entitlement exists for the given (license, rendition).
    EntitlementNotFound = 15,
    /// A monotonic counter overflowed; the call failed deterministically
    /// instead of silently wrapping.
    Overflow = 16,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LicenseStatus {
    Active,
    Revoked,
}

/// #942 — the access modes an entitlement may allow.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessMode {
    /// The rendition may be borrowed/read in its primary form.
    Borrow,
    /// The rendition may be accessed through an accessible alternative
    /// (e.g. alt-text EPUB). Granted intentionally, never implied by a
    /// `Borrow` entitlement on another rendition.
    AccessibleAlternative,
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
    /// #943 — maximum number of concurrent digital seats the license may
    /// allocate. Fixed at issuance (must be > 0).
    pub total_seats: u32,
    /// #943 — seats currently allocated. Always `<= total_seats`.
    pub allocated_seats: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessGrant {
    pub license_id: BytesN<32>,
    pub grantee: Address,
    pub not_before: u64,
    pub expires_at: u64,
}

/// #942 — an entitlement binds one rendition id of the licensed work to
/// one allowed access mode. Stored under
/// `DataKey::Entitlement(license_id, rendition_id)`; rendition ids are
/// also kept in an ordered `Vec` per license for bounded queries.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entitlement {
    pub license_id: BytesN<32>,
    pub rendition_id: Symbol,
    pub access_mode: AccessMode,
    pub granted_at: u64,
}

#[contracttype]
pub enum DataKey {
    Admin,
    LicenseCount,
    License(BytesN<32>),
    GrantCount,
    AccessGrant(BytesN<32>),
    /// #942 — `Entitlement(license_id, rendition_id, access_mode)` ->
    /// `Entitlement`. One entry per (rendition, access mode) pair.
    Entitlement(BytesN<32>, Symbol, AccessMode),
    /// #942 — ordered (rendition, access mode) pairs per license, for
    /// bounded queries.
    EntitlementKeys(BytesN<32>),
}

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

#[contract]
pub struct LibraryLicensing;

#[contractimpl]
impl LibraryLicensing {
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

    /// #940 — issue a license for `work_id` to `licensee` with an explicit
    /// validity window. `not_before` is inclusive and `expires_at` is
    /// exclusive, so a zero-length (`not_before == expires_at`) or inverted
    /// window is rejected. Enforcement happens at access time
    /// (`is_license_active`, `derive_access_grant`).
    ///
    /// #943 — `total_seats` reserves the license's concurrent-seat budget
    /// (must be > 0; `InvalidSeats` otherwise). Issuance always starts with
    /// zero allocated seats, so the `allocated <= total` invariant holds
    /// from the first lifecycle path.
    #[allow(clippy::too_many_arguments)]
    pub fn grant_license(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        licensee: Address,
        rights: String,
        not_before: u64,
        expires_at: u64,
        total_seats: u32,
    ) -> Result<BytesN<32>, LicenseError> {
        require_admin(&env, &caller)?;
        if rights.is_empty() {
            return Err(LicenseError::InvalidRights);
        }
        // #940 — the window must have positive extent. Equal timestamps mean
        // a zero-length license (active at no point since the end is
        // exclusive) and are rejected together with inverted windows.
        if not_before >= expires_at {
            return Err(LicenseError::InvalidWindow);
        }
        // #943 — a license with no seats can never be used; reject up front.
        if total_seats == 0 {
            return Err(LicenseError::InvalidSeats);
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
            total_seats,
            allocated_seats: 0,
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
            (id.clone(), licensee, not_before, expires_at, total_seats),
        );
        Ok(id)
    }

    /// Admin-only: revoke a license. Existing derived access grants stop
    /// being valid immediately (`is_grant_active` re-checks the parent
    /// license on every read). Seat allocation is also rejected on a
    /// revoked license, while release remains available so allocated seats
    /// can be cleaned up.
    pub fn revoke_license(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
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

    /// #940 — the licensee derives a bounded access grant for `grantee`
    /// lasting `duration` seconds. The grant window starts now and is
    /// clamped to the parent license's window, so a derived grant can never
    /// outlive the license it derives from. The duration addition uses
    /// checked arithmetic: an overflowing duration fails with
    /// `WindowOverflow` instead of wrapping to a past timestamp.
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
        // Only the licensee can delegate access under their own license.
        if caller != license.licensee {
            return Err(LicenseError::Unauthorized);
        }
        caller.require_auth();
        let now = env.ledger().timestamp();
        // #940 — boundary enforcement at derivation time: the license must be
        // inside its window. `not_before` is inclusive (`now == not_before`
        // is active), `expires_at` is exclusive (`now == expires_at` is
        // expired).
        if now < license.not_before {
            return Err(LicenseError::NotYetActive);
        }
        if now >= license.expires_at {
            return Err(LicenseError::Expired);
        }
        if license.status != LicenseStatus::Active {
            return Err(LicenseError::LicenseRevoked);
        }
        // #940 — checked arithmetic on the timestamp: adding the requested
        // duration must not overflow.
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

    /// #940 — read-only check: is the derived grant currently usable? Both
    /// the grant's own window and the parent license's window/status must be
    /// satisfied, so revoking a license immediately kills every grant
    /// derived from it.
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

    // ===== #942 — per-format entitlements =====

    /// #942 — admin-only: bind `rendition_id` on `license_id` to
    /// `access_mode`. Entitlements are keyed by the exact
    /// `(rendition_id, access_mode)` pair, so granting `Borrow` on one
    /// rendition never unlocks another rendition or mode, while an
    /// `AccessibleAlternative` can be granted intentionally without
    /// disturbing the primary entitlement. Granting an already-granted
    /// pair is idempotent (refreshes the TTL). Rejected when the license
    /// does not exist or is revoked.
    pub fn grant_entitlement(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        rendition_id: Symbol,
        access_mode: AccessMode,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if license.status != LicenseStatus::Active {
            return Err(LicenseError::LicenseRevoked);
        }

        let key = DataKey::Entitlement(license_id.clone(), rendition_id.clone(), access_mode);
        if let Some(ent) = env.storage().persistent().get::<DataKey, Entitlement>(&key) {
            // Idempotent re-grant: refresh the TTL and leave the record
            // (and the key list) untouched.
            env.storage()
                .persistent()
                .extend_ttl(&key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);
            env.events().publish(
                (symbol_short!("ENT_UPD"),),
                (license_id, rendition_id, ent.access_mode),
            );
            return Ok(());
        }

        // Append the (rendition, mode) pair to the ordered key list for
        // bounded queries.
        let keys_key = DataKey::EntitlementKeys(license_id.clone());
        let mut keys: Vec<(Symbol, AccessMode)> = env
            .storage()
            .persistent()
            .get(&keys_key)
            .unwrap_or_else(|| vec![&env]);
        let len = keys.len();
        let next = len.checked_add(1).ok_or(LicenseError::Overflow)?;
        keys.push_back((rendition_id.clone(), access_mode));
        env.storage().persistent().set(&keys_key, &keys);
        env.storage()
            .persistent()
            .extend_ttl(&keys_key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);

        let ent = Entitlement {
            license_id: license_id.clone(),
            rendition_id: rendition_id.clone(),
            access_mode,
            granted_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &ent);
        env.storage()
            .persistent()
            .extend_ttl(&key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);

        env.events().publish(
            (symbol_short!("ENT_GRANT"),),
            (license_id, rendition_id, ent.access_mode, next),
        );
        Ok(())
    }

    /// #942 — admin-only: remove the `(rendition_id, access_mode)`
    /// entitlement from `license_id`. Fails with `EntitlementNotFound`
    /// when no such pair exists.
    pub fn revoke_entitlement(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        rendition_id: Symbol,
        access_mode: AccessMode,
    ) -> Result<(), LicenseError> {
        require_admin(&env, &caller)?;
        let key = DataKey::Entitlement(license_id.clone(), rendition_id.clone(), access_mode);
        let ent: Entitlement = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(LicenseError::EntitlementNotFound)?;

        // Remove the pair from the ordered key list so queries stay bounded.
        let keys_key = DataKey::EntitlementKeys(license_id.clone());
        let mut keys: Vec<(Symbol, AccessMode)> = env
            .storage()
            .persistent()
            .get(&keys_key)
            .unwrap_or_else(|| vec![&env]);
        if let Some(pos) = keys
            .iter()
            .position(|(rid, mode)| rid == rendition_id && mode == access_mode)
        {
            keys.remove(pos as u32);
        }
        env.storage().persistent().set(&keys_key, &keys);
        env.storage()
            .persistent()
            .extend_ttl(&keys_key, LICENSE_MIN_TTL, LICENSE_MAX_TTL);

        env.storage().persistent().remove(&key);
        env.events().publish(
            (symbol_short!("ENT_REVK"),),
            (license_id, rendition_id, ent.access_mode),
        );
        Ok(())
    }

    /// #942 — read-only: is `rendition_id` usable under `license_id` with
    /// exactly `access_mode`? Returns `Ok(false)` when the license is not
    /// currently active (outside its window or revoked) or when the exact
    /// `(rendition_id, access_mode)` pair is not granted -- so borrowing
    /// one format never unlocks another format or mode.
    pub fn is_entitled(
        env: Env,
        license_id: BytesN<32>,
        rendition_id: Symbol,
        access_mode: AccessMode,
    ) -> Result<bool, LicenseError> {
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        let now = env.ledger().timestamp();
        if license.status != LicenseStatus::Active
            || now < license.not_before
            || now >= license.expires_at
        {
            return Ok(false);
        }
        let key = DataKey::Entitlement(license_id, rendition_id, access_mode);
        Ok(env.storage().persistent().has(&key))
    }

    /// #942 — read-only: returns the stored entitlement for the exact
    /// `(license_id, rendition_id, access_mode)` triple, or
    /// `EntitlementNotFound`.
    pub fn entitlement(
        env: Env,
        license_id: BytesN<32>,
        rendition_id: Symbol,
        access_mode: AccessMode,
    ) -> Result<Entitlement, LicenseError> {
        let key = DataKey::Entitlement(license_id, rendition_id, access_mode);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(LicenseError::EntitlementNotFound)
    }

    /// #942 — read-only: returns how many entitlements `license_id` has.
    pub fn entitlements_len(env: Env, license_id: BytesN<32>) -> u32 {
        let keys_key = DataKey::EntitlementKeys(license_id);
        env.storage()
            .persistent()
            .get::<DataKey, Vec<(Symbol, AccessMode)>>(&keys_key)
            .map(|keys| keys.len())
            .unwrap_or(0)
    }

    /// #942 — read-only: returns up to `limit` entitlements of
    /// `license_id` starting at `from` (0-based). Out-of-range windows
    /// clamp to the stored length, so the mapping is always queryable
    /// within bounds.
    pub fn entitlements(
        env: Env,
        license_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Vec<Entitlement> {
        let keys_key = DataKey::EntitlementKeys(license_id.clone());
        let keys: Vec<(Symbol, AccessMode)> = env
            .storage()
            .persistent()
            .get(&keys_key)
            .unwrap_or_else(|| vec![&env]);
        let len = keys.len();
        if from >= len || limit == 0 {
            return vec![&env];
        }
        let end = from.saturating_add(limit).min(len);
        let mut out: Vec<Entitlement> = vec![&env];
        for i in from..end {
            let (rendition_id, access_mode) = keys.get(i).unwrap();
            let key = DataKey::Entitlement(license_id.clone(), rendition_id.clone(), access_mode);
            if let Some(ent) = env.storage().persistent().get::<DataKey, Entitlement>(&key) {
                out.push_back(ent);
            }
        }
        out
    }

    // ===== #943 — concurrent digital seats =====

    /// #943 — the licensee allocates one concurrent digital seat on
    /// `license_id`. Uses checked arithmetic and can never exceed the
    /// license's `total_seats` supply: once the budget is exhausted,
    /// competing calls fail with `NoSeatsAvailable`. Allocation is only
    /// allowed while the license is inside its validity window and not
    /// revoked.
    pub fn allocate_seat(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
    ) -> Result<(), LicenseError> {
        let mut license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if caller != license.licensee {
            return Err(LicenseError::Unauthorized);
        }
        caller.require_auth();
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
        if license.allocated_seats >= license.total_seats {
            return Err(LicenseError::NoSeatsAvailable);
        }
        // Checked arithmetic: the invariant `allocated <= total` means this
        // cannot overflow, but the increment is explicit per the issue's
        // checked-arithmetic requirement.
        let allocated = license
            .allocated_seats
            .checked_add(1)
            .ok_or(LicenseError::NoSeatsAvailable)?;
        license.allocated_seats = allocated;
        env.storage()
            .persistent()
            .set(&DataKey::License(license_id.clone()), &license);
        env.storage().persistent().extend_ttl(
            &DataKey::License(license_id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("SEAT_NEW"),),
            (license_id, license.allocated_seats, license.total_seats),
        );
        Ok(())
    }

    /// #943 — the licensee releases one concurrent digital seat, restoring
    /// exactly one unit of supply. Rejected with `NoSeatsAllocated` when
    /// nothing is allocated (underflow guard). Release stays available on
    /// expired/revoked licenses so seats can always be cleaned up; the
    /// decrement is checked so it can never wrap.
    pub fn release_seat(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
    ) -> Result<(), LicenseError> {
        let mut license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id.clone()))
            .ok_or(LicenseError::NotFound)?;
        if caller != license.licensee {
            return Err(LicenseError::Unauthorized);
        }
        caller.require_auth();
        if license.allocated_seats == 0 {
            return Err(LicenseError::NoSeatsAllocated);
        }
        let allocated = license
            .allocated_seats
            .checked_sub(1)
            .ok_or(LicenseError::NoSeatsAllocated)?;
        license.allocated_seats = allocated;
        env.storage()
            .persistent()
            .set(&DataKey::License(license_id.clone()), &license);
        env.storage().persistent().extend_ttl(
            &DataKey::License(license_id.clone()),
            LICENSE_MIN_TTL,
            LICENSE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("SEAT_RELS"),),
            (license_id, license.allocated_seats, license.total_seats),
        );
        Ok(())
    }

    /// #943 — read-only: how many seats of `license_id` remain available
    /// (`total - allocated`). Always non-negative: `allocated <= total` is
    /// maintained on every lifecycle path.
    pub fn available_seats(env: Env, license_id: BytesN<32>) -> Result<u32, LicenseError> {
        let license: License = env
            .storage()
            .persistent()
            .get(&DataKey::License(license_id))
            .ok_or(LicenseError::NotFound)?;
        Ok(license.total_seats - license.allocated_seats)
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
}

#[cfg(test)]
mod tests;
