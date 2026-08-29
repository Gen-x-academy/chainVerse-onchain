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

#![no_std]

const LICENSE_MIN_TTL: u32 = 100_000;
const LICENSE_MAX_TTL: u32 = 500_000;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, String,
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
    NonTransferable = 12,
    LoanReturned = 13,
    InvalidRendition = 14,
}

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

#[contracttype]
pub enum DataKey {
    Admin,
    LicenseCount,
    License(BytesN<32>),
    GrantCount,
    AccessGrant(BytesN<32>),
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
    pub fn grant_license(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        licensee: Address,
        rights: String,
        not_before: u64,
        expires_at: u64,
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

    /// Admin-only: revoke a license. Existing derived access grants stop
    /// being valid immediately (`is_grant_active` re-checks the parent
    /// license on every read).
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
