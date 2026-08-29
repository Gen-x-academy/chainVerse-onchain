#![no_std]
//! #976 — Record lost and damaged attestations.
//!
//! Authorized librarians attach reason-coded, SHA-256 evidence-hash
//! attestations to a loan. The evidence document stays off-chain; only its
//! content-addressed hash lands on-chain (ADR-0001 I4). Corrections append a
//! new hash to an immutable history list — prior hashes are never erased.
//! Once marked `Resolved`, an attestation cannot receive a charge for the same
//! incident (`mark_charged` returns `AlreadyResolved`), and a second
//! `mark_charged` call on an already-charged attestation returns `AlreadyCharged`.
//!
//! ## ABI
//! `initialize`, `add_librarian`, `remove_librarian`, `attest`, `resolve`,
//! `append_correction`, `mark_charged`, `get_attestation`, `get_history`.
//!
//! ## Storage
//! Instance: `Admin`, `Librarian(addr)`, `AttestationCount`.
//! Persistent (TTL-tiered): `Attestation(id)`, `AttestationHistory(id)`.
//!
//! ## Events
//! `ATST_NEW`, `ATST_RES`, `ATST_COR`, `ATST_CHG`.
//!
//! ## Privacy
//! Only the SHA-256 hash of off-chain evidence is stored — no names, content,
//! or reading history ever lands on-chain (ADR-0001 I4).
//!
//! ## Migration
//! New independent contract; no prior on-chain state.

#[allow(unused_imports)]
use soroban_sdk::xdr::ToXdr;

const ATST_MIN_TTL: u32 = 100_000;
const ATST_MAX_TTL: u32 = 500_000;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, vec, Address, Bytes,
    BytesN, Env, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PhysicalError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyResolved = 5,
    AlreadyCharged = 6,
    Overflow = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttestationReason {
    Lost,
    Damaged,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttestationStatus {
    Open,
    Resolved,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestationRecord {
    pub loan_id: BytesN<32>,
    pub librarian: Address,
    pub reason: AttestationReason,
    /// SHA-256 of the initial off-chain evidence document.
    pub evidence_hash: BytesN<32>,
    pub status: AttestationStatus,
    pub created_at: u64,
    /// Set to `true` when a charge has been levied for this incident.
    pub charged: bool,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Librarian(Address),
    AttestationCount,
    Attestation(BytesN<32>),
    AttestationHistory(BytesN<32>),
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), PhysicalError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(PhysicalError::NotInitialized)?;
    if *caller != admin {
        return Err(PhysicalError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn require_librarian(env: &Env, caller: &Address) -> Result<(), PhysicalError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(PhysicalError::NotInitialized)?;
    if *caller == admin {
        caller.require_auth();
        return Ok(());
    }
    let is_lib: bool = env
        .storage()
        .instance()
        .get(&DataKey::Librarian(caller.clone()))
        .unwrap_or(false);
    if !is_lib {
        return Err(PhysicalError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// Collision-resistant id derivation (ADR-0001 I3): monotonic nonce mixed with
/// the ledger timestamp, loan id, and caller XDR.
fn next_attestation_id(
    env: &Env,
    loan_id: &BytesN<32>,
    caller: &Address,
) -> Result<BytesN<32>, PhysicalError> {
    let n: u64 = env
        .storage()
        .instance()
        .get(&DataKey::AttestationCount)
        .unwrap_or(0u64);
    let next = n.checked_add(1).ok_or(PhysicalError::Overflow)?;
    env.storage()
        .instance()
        .set(&DataKey::AttestationCount, &next);
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    input.append(&Bytes::from_slice(
        env,
        &env.ledger().timestamp().to_be_bytes(),
    ));
    input.append(&Bytes::from_slice(env, &loan_id.to_array()));
    input.append(&caller.to_xdr(env));
    Ok(env.crypto().sha256(&input).into())
}

fn save_attestation(env: &Env, id: &BytesN<32>, record: &AttestationRecord) {
    env.storage()
        .persistent()
        .set(&DataKey::Attestation(id.clone()), record);
    env.storage().persistent().extend_ttl(
        &DataKey::Attestation(id.clone()),
        ATST_MIN_TTL,
        ATST_MAX_TTL,
    );
}

#[contract]
pub struct LibraryPhysical;

#[contractimpl]
impl LibraryPhysical {
    /// One-time bootstrap. Sets the contract admin and must be called before any
    /// other function.
    pub fn initialize(env: Env, admin: Address) -> Result<(), PhysicalError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(PhysicalError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: grant `librarian` the right to create attestations.
    pub fn add_librarian(
        env: Env,
        caller: Address,
        librarian: Address,
    ) -> Result<(), PhysicalError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::Librarian(librarian.clone()), &true);
        env.events()
            .publish((symbol_short!("LIB_ADD"),), (librarian,));
        Ok(())
    }

    /// Admin-only: revoke `librarian`'s attestation rights.
    pub fn remove_librarian(
        env: Env,
        caller: Address,
        librarian: Address,
    ) -> Result<(), PhysicalError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::Librarian(librarian.clone()), &false);
        env.events()
            .publish((symbol_short!("LIB_REM"),), (librarian,));
        Ok(())
    }

    /// Attach a reason-coded attestation to `loan_id`. The `evidence_hash` is
    /// the SHA-256 of the off-chain evidence document (ADR-0001 I4). Returns
    /// the new attestation id.
    pub fn attest(
        env: Env,
        caller: Address,
        loan_id: BytesN<32>,
        reason: AttestationReason,
        evidence_hash: BytesN<32>,
    ) -> Result<BytesN<32>, PhysicalError> {
        require_librarian(&env, &caller)?;
        let id = next_attestation_id(&env, &loan_id, &caller)?;
        let record = AttestationRecord {
            loan_id: loan_id.clone(),
            librarian: caller.clone(),
            reason,
            evidence_hash: evidence_hash.clone(),
            status: AttestationStatus::Open,
            created_at: env.ledger().timestamp(),
            charged: false,
        };
        save_attestation(&env, &id, &record);
        // History starts with the initial evidence hash and is append-only.
        let history: Vec<BytesN<32>> = vec![&env, evidence_hash.clone()];
        env.storage()
            .persistent()
            .set(&DataKey::AttestationHistory(id.clone()), &history);
        env.storage().persistent().extend_ttl(
            &DataKey::AttestationHistory(id.clone()),
            ATST_MIN_TTL,
            ATST_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("ATST_NEW"),),
            (id.clone(), loan_id, caller, evidence_hash),
        );
        Ok(id)
    }

    /// Mark the attestation `Resolved`. A resolved attestation cannot receive a
    /// charge for the same incident. Returns `AlreadyResolved` on a second call.
    pub fn resolve(
        env: Env,
        caller: Address,
        attestation_id: BytesN<32>,
    ) -> Result<(), PhysicalError> {
        require_librarian(&env, &caller)?;
        let mut record: AttestationRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Attestation(attestation_id.clone()))
            .ok_or(PhysicalError::NotFound)?;
        if record.status == AttestationStatus::Resolved {
            return Err(PhysicalError::AlreadyResolved);
        }
        record.status = AttestationStatus::Resolved;
        save_attestation(&env, &attestation_id, &record);
        env.events()
            .publish((symbol_short!("ATST_RES"),), (attestation_id,));
        Ok(())
    }

    /// Append a corrected evidence hash without erasing prior history. Prior
    /// hashes are immutable — corrections only append (ADR-0001 I5).
    pub fn append_correction(
        env: Env,
        caller: Address,
        attestation_id: BytesN<32>,
        new_evidence_hash: BytesN<32>,
    ) -> Result<(), PhysicalError> {
        require_librarian(&env, &caller)?;
        let _: AttestationRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Attestation(attestation_id.clone()))
            .ok_or(PhysicalError::NotFound)?;
        let mut history: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&DataKey::AttestationHistory(attestation_id.clone()))
            .unwrap_or_else(|| vec![&env]);
        history.push_back(new_evidence_hash.clone());
        env.storage().persistent().set(
            &DataKey::AttestationHistory(attestation_id.clone()),
            &history,
        );
        env.storage().persistent().extend_ttl(
            &DataKey::AttestationHistory(attestation_id.clone()),
            ATST_MIN_TTL,
            ATST_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("ATST_COR"),),
            (attestation_id, new_evidence_hash),
        );
        Ok(())
    }

    /// Mark a charge levied for this incident. Returns `AlreadyResolved` if
    /// the attestation is resolved (a resolved item cannot be charged), and
    /// `AlreadyCharged` if the flag is already set — one charge per incident.
    pub fn mark_charged(
        env: Env,
        caller: Address,
        attestation_id: BytesN<32>,
    ) -> Result<(), PhysicalError> {
        require_librarian(&env, &caller)?;
        let mut record: AttestationRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Attestation(attestation_id.clone()))
            .ok_or(PhysicalError::NotFound)?;
        if record.status == AttestationStatus::Resolved {
            return Err(PhysicalError::AlreadyResolved);
        }
        if record.charged {
            return Err(PhysicalError::AlreadyCharged);
        }
        record.charged = true;
        save_attestation(&env, &attestation_id, &record);
        env.events()
            .publish((symbol_short!("ATST_CHG"),), (attestation_id,));
        Ok(())
    }

    pub fn get_attestation(
        env: Env,
        attestation_id: BytesN<32>,
    ) -> Result<AttestationRecord, PhysicalError> {
        env.storage()
            .persistent()
            .get(&DataKey::Attestation(attestation_id))
            .ok_or(PhysicalError::NotFound)
    }

    /// Returns the full ordered list of evidence hashes (initial + corrections).
    pub fn get_history(
        env: Env,
        attestation_id: BytesN<32>,
    ) -> Result<Vec<BytesN<32>>, PhysicalError> {
        let _: AttestationRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Attestation(attestation_id.clone()))
            .ok_or(PhysicalError::NotFound)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::AttestationHistory(attestation_id))
            .unwrap_or_else(|| vec![&env]))
    }
}

#[cfg(test)]
mod tests;
