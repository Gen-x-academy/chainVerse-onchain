//! Canonical work, edition, and rendition registry (#928, #929).
//!
//! ## #928 -- register canonical work commitments
//!
//! `register_work` maps a canonical work id to a bounded metadata
//! commitment and a pseudonymous custodian. Registration:
//! - **validates identifiers** -- all-zero ids/hashes are rejected;
//! - **authenticates the issuer** -- only the `PolicyManager` role can
//!   register, and `require_auth` proves the caller consented;
//! - **prevents overwrite** -- an id can only be registered once;
//! - **renews TTL** -- every entry, version snapshot, and child index is
//!   written with the CATALOG TTL tier and renewed on every touch;
//! - **emits a versioned event** -- `WRK_NEW` carries the work id, the
//!   version, and the metadata hash.
//!
//! ## #929 -- distinguish works, editions, and renditions
//!
//! Editions attach only to works and renditions only to editions; works
//! are always parent-less. Kinds are fixed at registration and there is
//! no re-parenting, so the graph is a forest: relationships can never
//! cycle and can never cross an invalid parent kind. Children queries
//! (`children`) are cursor/limit-bounded ([`CHILDREN_MAX_PAGE`]).
//!
//! ## Versioning (#932, #933)
//!
//! Every mutation (`register_*`, `update_metadata`, `update_content_hash`)
//! appends an immutable [`VersionSnapshot`] to the entry's history, so
//! each version's metadata URI and content hash stay exactly as they
//! were registered and can be verified at any later time.

use soroban_sdk::{symbol_short, Address, BytesN, Env, Vec};

use crate::content;
use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use crate::metadata;
use crate::types::{
    CatalogEntry, ChildrenPage, ContentCommitment, ContentState, EntryKind, MetadataCommitment,
    VersionSnapshot,
};

/// #929 -- children queries return at most this many ids per page.
pub const CHILDREN_MAX_PAGE: u32 = 50;

/// #928/#929 -- a registry identifier must carry real bytes; all-zero
/// ids are rejected.
fn validate_identifier(env: &Env, id: &BytesN<32>) -> Result<(), ContractError> {
    if id == &BytesN::from_array(env, &[0u8; 32]) {
        return Err(ContractError::InvalidIdentifier);
    }
    Ok(())
}

fn has_entry(env: &Env, id: &BytesN<32>) -> bool {
    env.storage().persistent().has(&DataKey::Entry(id.clone()))
}

/// Loads the current catalog entry for `id`, renewing its TTL.
pub fn get_entry(env: &Env, id: &BytesN<32>) -> Result<CatalogEntry, ContractError> {
    let key = DataKey::Entry(id.clone());
    let entry: CatalogEntry = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::EntryNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(entry)
}

/// Writes the current entry with the CATALOG TTL tier.
fn write_entry(env: &Env, id: &BytesN<32>, entry: &CatalogEntry) {
    let key = DataKey::Entry(id.clone());
    env.storage().persistent().set(&key, entry);
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
}

/// Appends `snapshot` to the entry's immutable version history and
/// returns its 1-based version number. Snapshots are never mutated, so a
/// version's metadata URI and content hash are immutable once recorded.
fn append_version(
    env: &Env,
    id: &BytesN<32>,
    snapshot: &VersionSnapshot,
) -> Result<u32, ContractError> {
    let count_key = DataKey::EntryVersionCount(id.clone());
    let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
    let next = count.checked_add(1).ok_or(ContractError::Overflow)?;
    let version_key = DataKey::EntryVersion(id.clone(), next);
    env.storage().persistent().set(&version_key, snapshot);
    env.storage()
        .persistent()
        .extend_ttl(&version_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    env.storage().persistent().set(&count_key, &next);
    env.storage()
        .persistent()
        .extend_ttl(&count_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(next)
}

/// Indexes `child_id` under `parent_id` for bounded children queries.
fn record_child(
    env: &Env,
    parent_id: &BytesN<32>,
    child_id: &BytesN<32>,
) -> Result<(), ContractError> {
    let count_key = DataKey::ChildCount(parent_id.clone());
    let n: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
    let index_key = DataKey::ChildIndex(parent_id.clone(), n);
    env.storage().persistent().set(&index_key, child_id);
    env.storage()
        .persistent()
        .extend_ttl(&index_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    let next = n.checked_add(1).ok_or(ContractError::Overflow)?;
    env.storage().persistent().set(&count_key, &next);
    env.storage()
        .persistent()
        .extend_ttl(&count_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(())
}

/// Builds the immutable snapshot for a newly created version.
fn make_snapshot(
    env: &Env,
    version: u32,
    metadata: &MetadataCommitment,
    content: &ContentState,
    custodian: &Address,
    registered_by: &Address,
) -> VersionSnapshot {
    VersionSnapshot {
        version,
        metadata: metadata.clone(),
        content: content.clone(),
        custodian: custodian.clone(),
        registered_by: registered_by.clone(),
        registered_at: env.ledger().timestamp(),
    }
}

/// #928 -- registers a canonical work: id -> bounded metadata
/// commitment + pseudonymous custodian. PolicyManager-only; rejects
/// all-zero ids/hashes, rejects duplicate ids, and publishes a versioned
/// `WRK_NEW` event.
pub fn register_work(
    env: &Env,
    caller: &Address,
    work_id: &BytesN<32>,
    metadata: &MetadataCommitment,
    custodian: &Address,
) -> Result<u32, ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;
    validate_identifier(env, work_id)?;
    metadata::validate_metadata(env, metadata)?;
    if has_entry(env, work_id) {
        return Err(ContractError::AlreadyRegistered);
    }

    let entry = CatalogEntry {
        kind: EntryKind::Work,
        parent: None,
        version: 1,
        metadata: metadata.clone(),
        content: ContentState::None,
        custodian: custodian.clone(),
    };
    let snapshot = make_snapshot(env, 1, metadata, &ContentState::None, custodian, caller);
    write_entry(env, work_id, &entry);
    append_version(env, work_id, &snapshot)?;

    env.events().publish(
        (symbol_short!("WRK_NEW"),),
        (work_id.clone(), 1u32, metadata.manifest_hash.clone()),
    );
    Ok(1)
}

/// #929 -- registers an edition of `parent_work_id`. The parent must be
/// an existing work (editions can only hang off works), and `edition_id`
/// must be unused. Publishes a versioned `EDN_NEW` event.
pub fn register_edition(
    env: &Env,
    caller: &Address,
    parent_work_id: &BytesN<32>,
    edition_id: &BytesN<32>,
    metadata: &MetadataCommitment,
    custodian: &Address,
) -> Result<u32, ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;
    validate_identifier(env, edition_id)?;
    metadata::validate_metadata(env, metadata)?;
    if has_entry(env, edition_id) {
        return Err(ContractError::AlreadyRegistered);
    }
    let parent = get_entry(env, parent_work_id)?;
    if parent.kind != EntryKind::Work {
        return Err(ContractError::InvalidParent);
    }

    let entry = CatalogEntry {
        kind: EntryKind::Edition,
        parent: Some(parent_work_id.clone()),
        version: 1,
        metadata: metadata.clone(),
        content: ContentState::None,
        custodian: custodian.clone(),
    };
    let snapshot = make_snapshot(env, 1, metadata, &ContentState::None, custodian, caller);
    write_entry(env, edition_id, &entry);
    append_version(env, edition_id, &snapshot)?;
    record_child(env, parent_work_id, edition_id)?;

    env.events().publish(
        (symbol_short!("EDN_NEW"),),
        (
            edition_id.clone(),
            parent_work_id.clone(),
            1u32,
            metadata.manifest_hash.clone(),
        ),
    );
    Ok(1)
}

/// #929 + #932 -- registers a rendition (concrete digital format) of
/// `parent_edition_id` with its algorithm-tagged content hash. The
/// parent must be an existing edition (renditions can only hang off
/// editions). Publishes a versioned `RND_NEW` event carrying the hash.
pub fn register_rendition(
    env: &Env,
    caller: &Address,
    parent_edition_id: &BytesN<32>,
    rendition_id: &BytesN<32>,
    content: &ContentCommitment,
    metadata: &MetadataCommitment,
    custodian: &Address,
) -> Result<u32, ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;
    validate_identifier(env, rendition_id)?;
    metadata::validate_metadata(env, metadata)?;
    content::validate_content(env, content)?;
    if has_entry(env, rendition_id) {
        return Err(ContractError::AlreadyRegistered);
    }
    let parent = get_entry(env, parent_edition_id)?;
    if parent.kind != EntryKind::Edition {
        return Err(ContractError::InvalidParent);
    }

    let content_state = ContentState::Committed(content.clone());
    let entry = CatalogEntry {
        kind: EntryKind::Rendition,
        parent: Some(parent_edition_id.clone()),
        version: 1,
        metadata: metadata.clone(),
        content: content_state.clone(),
        custodian: custodian.clone(),
    };
    let snapshot = make_snapshot(env, 1, metadata, &content_state, custodian, caller);
    write_entry(env, rendition_id, &entry);
    append_version(env, rendition_id, &snapshot)?;
    record_child(env, parent_edition_id, rendition_id)?;

    env.events().publish(
        (symbol_short!("RND_NEW"),),
        (
            rendition_id.clone(),
            parent_edition_id.clone(),
            1u32,
            content.algorithm,
            content.digest.clone(),
        ),
    );
    Ok(1)
}

/// #933 -- updates the metadata commitment of any entry, creating a new
/// version. The previous version's URI/hash remain immutable in the
/// version history. Publishes a versioned `MET_UPD` event.
pub fn update_metadata(
    env: &Env,
    caller: &Address,
    entry_id: &BytesN<32>,
    metadata: &MetadataCommitment,
) -> Result<u32, ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;
    metadata::validate_metadata(env, metadata)?;
    let entry = get_entry(env, entry_id)?;
    let new_version = entry
        .version
        .checked_add(1)
        .ok_or(ContractError::Overflow)?;

    let updated = CatalogEntry {
        kind: entry.kind,
        parent: entry.parent.clone(),
        version: new_version,
        metadata: metadata.clone(),
        content: entry.content.clone(),
        custodian: entry.custodian.clone(),
    };
    let snapshot = make_snapshot(
        env,
        new_version,
        metadata,
        &updated.content,
        &updated.custodian,
        caller,
    );
    write_entry(env, entry_id, &updated);
    append_version(env, entry_id, &snapshot)?;

    env.events().publish(
        (symbol_short!("MET_UPD"),),
        (
            entry_id.clone(),
            entry.version,
            new_version,
            metadata.manifest_hash.clone(),
        ),
    );
    Ok(new_version)
}

/// #932 -- replaces the content commitment of a rendition, creating a
/// new version. The previous version's hash is preserved immutably in
/// the version history. Rejected for non-rendition entries
/// (`InvalidKind`). Publishes a versioned `HASH_UPD` event.
pub fn update_content_hash(
    env: &Env,
    caller: &Address,
    rendition_id: &BytesN<32>,
    content: &ContentCommitment,
) -> Result<u32, ContractError> {
    governance::require_role(env, Role::PolicyManager, caller)?;
    content::validate_content(env, content)?;
    let entry = get_entry(env, rendition_id)?;
    if entry.kind != EntryKind::Rendition {
        return Err(ContractError::InvalidKind);
    }
    let new_version = entry
        .version
        .checked_add(1)
        .ok_or(ContractError::Overflow)?;

    let new_content = ContentState::Committed(content.clone());
    let updated = CatalogEntry {
        kind: entry.kind,
        parent: entry.parent.clone(),
        version: new_version,
        metadata: entry.metadata.clone(),
        content: new_content.clone(),
        custodian: entry.custodian.clone(),
    };
    let snapshot = make_snapshot(
        env,
        new_version,
        &updated.metadata,
        &new_content,
        &updated.custodian,
        caller,
    );
    write_entry(env, rendition_id, &updated);
    append_version(env, rendition_id, &snapshot)?;

    env.events().publish(
        (symbol_short!("HASH_UPD"),),
        (
            rendition_id.clone(),
            entry.version,
            new_version,
            content.algorithm,
            content.digest.clone(),
        ),
    );
    Ok(new_version)
}

/// Returns the current catalog entry for `id`, renewing its TTL.
pub fn entry(env: &Env, entry_id: &BytesN<32>) -> Result<CatalogEntry, ContractError> {
    get_entry(env, entry_id)
}

/// Returns how many versions have been recorded for `id`.
pub fn version_count(env: &Env, entry_id: &BytesN<32>) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::EntryVersionCount(entry_id.clone()))
        .unwrap_or(0)
}

/// Returns the immutable snapshot for `version` (1-based) of `entry_id`,
/// queryable only within bounds.
pub fn get_version(
    env: &Env,
    entry_id: &BytesN<32>,
    version: u32,
) -> Result<VersionSnapshot, ContractError> {
    let count = version_count(env, entry_id);
    if version == 0 || version > count {
        return Err(ContractError::VersionNotFound);
    }
    let key = DataKey::EntryVersion(entry_id.clone(), version);
    let snapshot: VersionSnapshot = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::VersionNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
    Ok(snapshot)
}

/// #929 -- cursor/limit-bounded children query: returns up to `limit`
/// child ids of `parent_id` starting at `cursor`. `limit` must be in
/// `1..=CHILDREN_MAX_PAGE` and `cursor` within the child list; the
/// returned page reports the next cursor and whether it is the final
/// page, so callers can page through every child deterministically.
pub fn children(
    env: &Env,
    parent_id: &BytesN<32>,
    cursor: u32,
    limit: u32,
) -> Result<ChildrenPage, ContractError> {
    if limit == 0 || limit > CHILDREN_MAX_PAGE {
        return Err(ContractError::InvalidLimit);
    }
    let total: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::ChildCount(parent_id.clone()))
        .unwrap_or(0);
    if cursor > total {
        return Err(ContractError::InvalidLimit);
    }

    let mut ids: Vec<BytesN<32>> = Vec::new(env);
    let mut idx = cursor;
    while idx < total && ids.len() < limit {
        let key = DataKey::ChildIndex(parent_id.clone(), idx);
        match env.storage().persistent().get(&key) {
            Some(id) => {
                env.storage()
                    .persistent()
                    .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
                ids.push_back(id);
            }
            None => break,
        }
        idx += 1;
    }
    let done = idx >= total;
    Ok(ChildrenPage {
        ids,
        next_cursor: idx,
        done,
    })
}
