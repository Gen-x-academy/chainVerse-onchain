//! Acquisition and donation provenance commitments (#934).
//!
//! The future library flow needs auditable provenance (how a work was
//! acquired -- purchase, donation, or institutional acquisition) without
//! publishing donor or invoice details. This module stores only the hash
//! of the off-chain provenance document plus the attestation metadata.
//! Corrections append a new record linked through `previous_hash` rather
//! than overwriting history.

use soroban_sdk::{symbol_short, Address, BytesN, Env};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use crate::types::{ProvenanceRecord, ProvenanceType};

/// Role-gated (#934): attest the provenance of `work_id`.
///
/// - Only the `PolicyManager` role can attest.
/// - An all-zero hash is rejected (`InvalidHash`) -- the off-chain
///   provenance document must be content-addressed by a real hash.
/// - Private document contents (donor names, invoice numbers, agreement
///   text) never land on-chain; only the hash is committed.
/// - History is append-only: each new record links back to the previous
///   one via `previous_hash`, and the `PROV_NEW` event carries both the
///   old and the new hash so corrections are fully auditable.
pub fn attest_provenance(
    env: &Env,
    caller: &Address,
    work_id: BytesN<32>,
    provenance_type: ProvenanceType,
    provenance_hash: BytesN<32>,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;

    if provenance_hash == BytesN::from_array(env, &[0u8; 32]) {
        return Err(ContractError::InvalidHash);
    }

    let count_key = DataKey::ProvenanceCount(work_id.clone());
    let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0);
    let next = count.checked_add(1).ok_or(ContractError::Overflow)?;

    let previous_hash: Option<BytesN<32>> = if next > 1 {
        env.storage()
            .persistent()
            .get::<DataKey, ProvenanceRecord>(&DataKey::Provenance(work_id.clone(), next - 1))
            .map(|r| r.provenance_hash)
    } else {
        None
    };

    let record = ProvenanceRecord {
        work_id: work_id.clone(),
        provenance_type,
        provenance_hash: provenance_hash.clone(),
        attested_by: caller.clone(),
        attested_at: env.ledger().timestamp(),
        previous_hash: previous_hash.clone(),
    };

    let key = DataKey::Provenance(work_id.clone(), next);
    env.storage().persistent().set(&key, &record);
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

    env.storage().persistent().set(&count_key, &next);
    env.storage()
        .persistent()
        .extend_ttl(&count_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

    let old_hash = previous_hash
        .clone()
        .unwrap_or_else(|| BytesN::from_array(env, &[0u8; 32]));
    env.events().publish(
        (symbol_short!("PROV_NEW"),),
        (
            work_id,
            provenance_type,
            old_hash,
            provenance_hash,
            caller.clone(),
            record.attested_at,
        ),
    );

    Ok(())
}

/// Returns the number of attested provenance records for `work_id`
/// (0 when none have been attested yet).
pub fn provenance_len(env: &Env, work_id: &BytesN<32>) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::ProvenanceCount(work_id.clone()))
        .unwrap_or(0)
}

/// Returns the `index`-th provenance record for `work_id` (1-based),
/// rejecting out-of-bounds indexes with `ProvenanceNotFound` so the
/// history is only ever queryable within bounds.
pub fn get_provenance(
    env: &Env,
    work_id: &BytesN<32>,
    index: u64,
) -> Result<ProvenanceRecord, ContractError> {
    let count = provenance_len(env, work_id);
    if index == 0 || index > count {
        return Err(ContractError::ProvenanceNotFound);
    }
    let key = DataKey::Provenance(work_id.clone(), index);
    let record: ProvenanceRecord = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::ProvenanceNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(record)
}
