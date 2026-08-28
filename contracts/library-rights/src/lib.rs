#![no_std]

//! # Library Rights Contract
//!
//! On-chain foundation for the E-Library feature. Kept decoupled from the
//! `escrow` contract -- library rights (works, licenses, loans, holds)
//! are a distinct domain from escrowed payments and do not import or
//! depend on escrow types/state.
//!
//! ## Issue history
//! - **#924 (foundation):** deployable shell, versioned ABI, typed
//!   errors.
//! - **#925 (storage):** versioned [`keys::DataKey`]/[`Role`] scheme and
//!   per-domain TTL tiers.
//! - **#926 (governance):** one-time four-role bootstrap (`Admin`,
//!   `Treasury`, `PolicyManager`, `Emergency`) in [`governance`]. This
//!   replaces #924's placeholder single-admin `initialize`/`get_admin`
//!   -- the crate has never been deployed, so this is a pre-release
//!   evolution, not a migration of live state.
//! - **#927 (privacy):** [`WorkRecord`] holds only a content hash and a
//!   pseudonymous custodian address -- no names, emails, raw content,
//!   reading position, or staff notes ever land on-chain.
//! - **#952–#954:** governed rendition migration, integrity quarantine, and
//!   scoped pseudonymous membership attestations.
//!
//! ## Impact summary
//! - **ABI:** governance, work-state, quarantine, and membership-attestation
//!   entrypoints are exposed alongside the original work APIs.
//! - **Storage:** persistent, versioned keys per [`keys::DataKey`], each
//!   TTL-tiered by domain and renewed on every read/write that touches
//!   it. `SchemaVersion` lives in instance storage.
//! - **Events:** `BOOTSTRP` published once, on successful bootstrap.
//! - **Privacy:** see [`types`] -- hash + pseudonymous address only.
//! - **Deployment:** new, independently deployable contract; no existing
//!   contract is replaced.
//! - **Migration:** none yet -- no prior on-chain state exists. Future
//!   schema changes bump [`keys::SCHEMA_VERSION`].

mod errors;
mod governance;
mod keys;
mod types;

pub use errors::ContractError;
pub use keys::{DataKey, Role};
pub use types::{
    ContentStatus, MembershipAttestation, MembershipStatus, QuarantineRecord, WorkRecord,
};

use keys::{DataKey as DK, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use soroban_sdk::{
    contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env, String,
};

const CONTRACT_VERSION: &str = "0.5.0";

fn membership_id(
    env: &Env,
    wallet: &Address,
    claim_commitment: &BytesN<32>,
    institution_domain_hash: &BytesN<32>,
    network_id: &BytesN<32>,
    nonce: u64,
) -> BytesN<32> {
    let mut input = Bytes::new(env);
    input.append(&wallet.to_xdr(env));
    input.append(&Bytes::from_slice(env, &claim_commitment.to_array()));
    input.append(&Bytes::from_slice(env, &institution_domain_hash.to_array()));
    input.append(&Bytes::from_slice(env, &network_id.to_array()));
    input.append(&Bytes::from_slice(env, &nonce.to_be_bytes()));
    env.crypto().sha256(&input).into()
}

#[contract]
pub struct LibraryRightsContract;

#[contractimpl]
impl LibraryRightsContract {
    /// One-time bootstrap: assigns all four governance roles. Each
    /// address must independently authorize its own assignment;
    /// duplicate addresses across roles are rejected. Fails if the
    /// contract has already been bootstrapped.
    pub fn bootstrap(
        env: Env,
        admin: Address,
        treasury: Address,
        policy_manager: Address,
        emergency: Address,
    ) -> Result<(), ContractError> {
        governance::bootstrap(&env, admin, treasury, policy_manager, emergency)
    }

    /// Returns the address currently holding `role`.
    pub fn get_role(env: Env, role: Role) -> Result<Address, ContractError> {
        governance::get_role(&env, role)
    }

    /// Registers a work's content hash and custodian. Restricted to the
    /// `PolicyManager` role.
    pub fn put_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        work_hash: BytesN<32>,
        custodian: Address,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let key = DK::Work(work_id);
        let record = WorkRecord {
            work_hash,
            custodian,
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        Ok(())
    }

    /// Marks a work unavailable through ordinary catalog deactivation.
    pub fn deactivate_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        Self::set_content_status(&env, work_id, ContentStatus::Deactivated)
    }

    /// Records a legal takedown as a distinct state from technical quarantine.
    pub fn legal_takedown_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        Self::set_content_status(&env, work_id, ContentStatus::LegalTakedown)
    }

    /// Emergency path for a failed content-integrity commitment. Only the
    /// Emergency role may invoke it; the original work hash and record remain
    /// intact, while access becomes unavailable immediately.
    pub fn quarantine_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        reason_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::Emergency, &caller)?;
        let work_key = DK::Work(work_id.clone());
        if !env.storage().persistent().has(&work_key) {
            return Err(ContractError::WorkNotFound);
        }
        let status_key = DK::ContentStatus(work_id.clone());
        if env.storage().persistent().get(&status_key) == Some(ContentStatus::Quarantined) {
            return Err(ContractError::AlreadyQuarantined);
        }
        if env.storage().persistent().get(&status_key) == Some(ContentStatus::LegalTakedown) {
            return Err(ContractError::InvalidStateTransition);
        }
        env.storage()
            .persistent()
            .set(&status_key, &ContentStatus::Quarantined);
        let quarantine = QuarantineRecord {
            reason_hash,
            quarantined_at: env.ledger().timestamp(),
            quarantined_by: caller,
            restored_at: None,
            restoration_review_hash: None,
        };
        let quarantine_key = DK::Quarantine(work_id.clone());
        env.storage().persistent().set(&quarantine_key, &quarantine);
        env.storage()
            .persistent()
            .extend_ttl(&status_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        env.storage()
            .persistent()
            .extend_ttl(&quarantine_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        env.events().publish(
            (symbol_short!("QUARANTIN"),),
            (work_id, quarantine.reason_hash, quarantine.quarantined_at),
        );
        Ok(())
    }

    /// Restores a quarantined work only after a PolicyManager supplies an
    /// opaque review record hash. Restoration cannot erase the quarantine
    /// evidence; it updates the same record with the review outcome.
    pub fn restore_quarantined_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        review_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let status_key = DK::ContentStatus(work_id.clone());
        if env.storage().persistent().get(&status_key) != Some(ContentStatus::Quarantined) {
            return Err(ContractError::InvalidStateTransition);
        }
        let quarantine_key = DK::Quarantine(work_id.clone());
        let mut record: QuarantineRecord = env
            .storage()
            .persistent()
            .get(&quarantine_key)
            .ok_or(ContractError::InvalidStateTransition)?;
        record.restored_at = Some(env.ledger().timestamp());
        record.restoration_review_hash = Some(review_hash);
        env.storage().persistent().set(&quarantine_key, &record);
        env.storage()
            .persistent()
            .set(&status_key, &ContentStatus::Active);
        env.events()
            .publish((symbol_short!("QUAR_REST"),), (work_id, record.restored_at));
        Ok(())
    }

    pub fn content_status(env: Env, work_id: BytesN<32>) -> Result<ContentStatus, ContractError> {
        if !env.storage().persistent().has(&DK::Work(work_id.clone())) {
            return Err(ContractError::WorkNotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DK::ContentStatus(work_id))
            .unwrap_or(ContentStatus::Active))
    }

    pub fn quarantine_record(
        env: Env,
        work_id: BytesN<32>,
    ) -> Result<QuarantineRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Quarantine(work_id))
            .ok_or(ContractError::InvalidStateTransition)
    }

    pub fn is_work_accessible(env: Env, work_id: BytesN<32>) -> Result<bool, ContractError> {
        Ok(Self::content_status(env, work_id)? == ContentStatus::Active)
    }

    fn set_content_status(
        env: &Env,
        work_id: BytesN<32>,
        status: ContentStatus,
    ) -> Result<(), ContractError> {
        let work_key = DK::Work(work_id.clone());
        if !env.storage().persistent().has(&work_key) {
            return Err(ContractError::WorkNotFound);
        }
        let status_key = DK::ContentStatus(work_id.clone());
        if env.storage().persistent().get(&status_key) == Some(ContentStatus::Quarantined) {
            return Err(ContractError::InvalidStateTransition);
        }
        env.storage().persistent().set(&status_key, &status);
        env.storage()
            .persistent()
            .extend_ttl(&status_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        env.events()
            .publish((symbol_short!("WRK_STATE"),), (work_id, status));
        Ok(())
    }

    /// Returns the stored record for `work_id`, renewing its TTL.
    pub fn get_work(env: Env, work_id: BytesN<32>) -> Result<WorkRecord, ContractError> {
        let key = DK::Work(work_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::WorkNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        Ok(record)
    }

    /// Issues a pseudonymous membership attestation. The caller supplies only
    /// commitments: a claim digest, an institution-domain digest, and a
    /// network identifier digest. The plaintext claim must never be sent to
    /// this contract. Issuing a new attestation rotates the wallet's current
    /// pointer and revokes the prior record without deleting its history.
    pub fn attest_membership(
        env: Env,
        caller: Address,
        wallet: Address,
        claim_commitment: BytesN<32>,
        institution_domain_hash: BytesN<32>,
        network_id: BytesN<32>,
        expires_at: u64,
    ) -> Result<BytesN<32>, ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let issued_at = env.ledger().timestamp();
        if expires_at <= issued_at {
            return Err(ContractError::InvalidStateTransition);
        }
        let count: u64 = env
            .storage()
            .instance()
            .get(&DK::MembershipCount)
            .unwrap_or(0);
        let next = count
            .checked_add(1)
            .ok_or(ContractError::InvalidStateTransition)?;
        env.storage().instance().set(&DK::MembershipCount, &next);
        let id = membership_id(
            &env,
            &wallet,
            &claim_commitment,
            &institution_domain_hash,
            &network_id,
            next,
        );
        if let Some(previous_id) = env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&DK::MembershipCurrent(wallet.clone()))
        {
            if let Some(mut previous) = env
                .storage()
                .persistent()
                .get::<_, MembershipAttestation>(&DK::MembershipAttestation(previous_id.clone()))
            {
                previous.status = MembershipStatus::Revoked;
                env.storage()
                    .persistent()
                    .set(&DK::MembershipAttestation(previous_id.clone()), &previous);
                env.events()
                    .publish((symbol_short!("MEM_REVOK"),), previous_id);
            }
        }
        let attestation = MembershipAttestation {
            wallet: wallet.clone(),
            claim_commitment,
            institution_domain_hash,
            network_id,
            nonce: next,
            issued_at,
            expires_at,
            status: MembershipStatus::Active,
        };
        env.storage()
            .persistent()
            .set(&DK::MembershipAttestation(id.clone()), &attestation);
        env.storage()
            .persistent()
            .set(&DK::MembershipCurrent(wallet.clone()), &id);
        env.storage().persistent().extend_ttl(
            &DK::MembershipAttestation(id.clone()),
            CATALOG_MIN_TTL,
            CATALOG_MAX_TTL,
        );
        env.storage().persistent().extend_ttl(
            &DK::MembershipCurrent(wallet),
            CATALOG_MIN_TTL,
            CATALOG_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("MEM_ISSUE"),),
            (id.clone(), issued_at, expires_at),
        );
        Ok(id)
    }

    /// Revokes a membership without exposing the institution's underlying
    /// claim. The prior record remains available for an audit trail.
    pub fn revoke_membership(
        env: Env,
        caller: Address,
        attestation_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let key = DK::MembershipAttestation(attestation_id.clone());
        let mut attestation: MembershipAttestation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::MembershipNotFound)?;
        if attestation.status == MembershipStatus::Revoked {
            return Err(ContractError::AlreadyRevoked);
        }
        attestation.status = MembershipStatus::Revoked;
        env.storage().persistent().set(&key, &attestation);
        if env
            .storage()
            .persistent()
            .get(&DK::MembershipCurrent(attestation.wallet.clone()))
            == Some(attestation_id.clone())
        {
            env.storage()
                .persistent()
                .remove(&DK::MembershipCurrent(attestation.wallet.clone()));
        }
        env.events()
            .publish((symbol_short!("MEM_REVOK"),), attestation_id);
        Ok(())
    }

    /// Proves borrowing eligibility without revealing a name or student
    /// number. The caller must present the same institution and network
    /// domain commitments used at issuance, preventing cross-scope replay.
    pub fn is_membership_active(
        env: Env,
        wallet: Address,
        claim_commitment: BytesN<32>,
        institution_domain_hash: BytesN<32>,
        network_id: BytesN<32>,
    ) -> bool {
        let current_id: BytesN<32> = match env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&DK::MembershipCurrent(wallet.clone()))
        {
            Some(id) => id,
            None => return false,
        };
        let attestation: MembershipAttestation = match env
            .storage()
            .persistent()
            .get(&DK::MembershipAttestation(current_id.clone()))
        {
            Some(value) => value,
            None => return false,
        };
        let expected_id = membership_id(
            &env,
            &wallet,
            &claim_commitment,
            &institution_domain_hash,
            &network_id,
            attestation.nonce,
        );
        let now = env.ledger().timestamp();
        attestation.status == MembershipStatus::Active
            && current_id == expected_id
            && now >= attestation.issued_at
            && now < attestation.expires_at
    }

    pub fn membership_attestation(
        env: Env,
        attestation_id: BytesN<32>,
    ) -> Result<MembershipAttestation, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::MembershipAttestation(attestation_id))
            .ok_or(ContractError::MembershipNotFound)
    }

    /// Returns this contract's ABI version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod tests;
