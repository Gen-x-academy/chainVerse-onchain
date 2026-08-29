//! Taxonomy and audience classification commitments (#931).
//!
//! Subjects, genres, languages, and audience ratings live off-chain as
//! versioned manifests; this module anchors their integrity on-chain by
//! committing only the manifest hash, its schema version, and the
//! issuing role-holder. Updates are append-only: each new commitment
//! links back to the previous one and publishes an event carrying the
//! old and new hashes so indexers can follow the full history.

use soroban_sdk::{symbol_short, Address, BytesN, Env};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use crate::types::{ClassificationCommit, ClassificationKind};

/// Role-gated (#931): commit the hash of an off-chain classification
/// manifest for `kind` (Taxonomy or Audience).
///
/// - Only the `PolicyManager` role can attest.
/// - An all-zero hash is rejected (`InvalidHash`) -- a commitment must
///   carry a real content address.
/// - The previous commitment (if any) is preserved: the new record is
///   appended to the immutable history and linked through
///   `previous_hash`, and the `CLS_NEW` event carries both the old and
///   the new hash for indexers.
pub fn commit_classification(
    env: &Env,
    caller: &Address,
    kind: ClassificationKind,
    manifest_hash: BytesN<32>,
    schema_version: u32,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;

    if manifest_hash == BytesN::from_array(env, &[0u8; 32]) {
        return Err(ContractError::InvalidHash);
    }

    let current_key = DataKey::Classification(kind);
    let current: Option<ClassificationCommit> = env.storage().persistent().get(&current_key);
    let previous_hash = current.map(|c| c.manifest_hash);

    // Append to the immutable history before updating the current pointer
    // so a reader can never observe a current commitment without its
    // history entry.
    let count_key = DataKey::ClassificationCount(kind);
    let count: u64 = env.storage().persistent().get(&count_key).unwrap_or(0);
    let next = count.checked_add(1).ok_or(ContractError::Overflow)?;

    let commit = ClassificationCommit {
        manifest_hash: manifest_hash.clone(),
        schema_version,
        issuer: caller.clone(),
        previous_hash: previous_hash.clone(),
        committed_at: env.ledger().timestamp(),
    };

    let history_key = DataKey::ClassificationHistory(kind, next);
    env.storage().persistent().set(&history_key, &commit);
    env.storage()
        .persistent()
        .extend_ttl(&history_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

    env.storage().persistent().set(&count_key, &next);
    env.storage()
        .persistent()
        .extend_ttl(&count_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

    env.storage().persistent().set(&current_key, &commit);
    env.storage()
        .persistent()
        .extend_ttl(&current_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

    let old_hash = previous_hash
        .clone()
        .unwrap_or_else(|| BytesN::from_array(env, &[0u8; 32]));
    env.events().publish(
        (symbol_short!("CLS_NEW"),),
        (
            kind,
            old_hash,
            manifest_hash,
            schema_version,
            caller.clone(),
        ),
    );

    Ok(())
}

/// Returns the current (latest) classification commitment for `kind`,
/// renewing its TTL on read.
pub fn get_classification(
    env: &Env,
    kind: ClassificationKind,
) -> Result<ClassificationCommit, ContractError> {
    let key = DataKey::Classification(kind);
    let commit: ClassificationCommit = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::ClassificationNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(commit)
}

/// Returns the number of committed classification history entries for
/// `kind` (0 when nothing has been committed yet).
pub fn classification_history_len(env: &Env, kind: ClassificationKind) -> u64 {
    env.storage()
        .persistent()
        .get(&DataKey::ClassificationCount(kind))
        .unwrap_or(0)
}

/// Returns the `index`-th classification commitment for `kind` (1-based),
/// rejecting out-of-bounds indexes with `ClassificationNotFound` so the
/// history is only ever queryable within bounds.
pub fn classification_history(
    env: &Env,
    kind: ClassificationKind,
    index: u64,
) -> Result<ClassificationCommit, ContractError> {
    let count = classification_history_len(env, kind);
    if index == 0 || index > count {
        return Err(ContractError::ClassificationNotFound);
    }
    let key = DataKey::ClassificationHistory(kind, index);
    let commit: ClassificationCommit = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::ClassificationNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(commit)
}
