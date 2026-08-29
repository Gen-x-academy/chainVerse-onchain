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

#[contracttype]
pub enum DataKey {
    Admin,
    LicenseCount,
    License(BytesN<32>),
    GrantCount,
    AccessGrant(BytesN<32>),
    Allocation(BytesN<32>),
    AllocationTotal(BytesN<32>),
    AllocationLimit(BytesN<32>),
    AttestationCount,
    Attestation(BytesN<32>),
    OfferCount,
    Offer(BytesN<32>),
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
