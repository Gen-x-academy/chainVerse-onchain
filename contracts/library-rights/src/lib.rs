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
//! - **#928 (canonical works):** [`registry::register_work`] maps a
//!   canonical work id to a bounded metadata commitment and custodian,
//!   with identifier validation, PolicyManager authentication, overwrite
//!   prevention, TTL renewal, and a versioned `WRK_NEW` event.
//! - **#929 (works/editions/renditions):** [`registry`] fixes parent
//!   edges to `Work -> Edition -> Rendition` (no cycles, no invalid
//!   parents) and exposes cursor/limit-bounded `children` queries.
//! - **#932 (content hashes):** [`content`] anchors algorithm-tagged
//!   digests per rendition (allowlisted [`HashAlgorithm`] enum, non-zero
//!   digests) that are immutable per version, with a read-only
//!   `verify_content` check.
//! - **#933 (metadata URIs):** [`metadata`] validates scheme + length on
//!   every metadata commitment and `registry::update_metadata` creates
//!   versions instead of mutating.
//!
//! ## Impact summary
//! - **ABI:** `bootstrap(admin, treasury, policy_manager, emergency)`,
//!   `get_role(role)`, `put_work(caller, work_id, work_hash, custodian)`,
//!   `get_work(work_id)`, `register_work(caller, work_id, metadata,
//!   custodian)`, `register_edition(caller, parent_work_id, edition_id,
//!   metadata, custodian)`, `register_rendition(caller, parent_edition_id,
//!   rendition_id, content, metadata, custodian)`, `update_metadata(caller,
//!   entry_id, metadata)`, `update_content_hash(caller, rendition_id,
//!   content)`, `entry(entry_id)`, `entry_version(entry_id, version)`,
//!   `entry_version_count(entry_id)`, `children(parent_id, cursor,
//!   limit)`, `verify_content(rendition_id, algorithm, digest)`,
//!   `version()`.
//! - **Storage:** persistent, versioned keys per [`keys::DataKey`], each
//!   TTL-tiered by domain and renewed on every read/write that touches
//!   it. `SchemaVersion` lives in instance storage. Entry version
//!   snapshots are append-only (never overwritten) so each version's
//!   commitments stay immutable.
//! - **Events:** `BOOTSTRP` on bootstrap; `WRK_NEW` (work_id, version,
//!   metadata_hash), `EDN_NEW` (edition_id, parent, version,
//!   metadata_hash), `RND_NEW` (rendition_id, parent, version,
//!   algorithm, digest), `MET_UPD` (entry_id, old_version, new_version,
//!   metadata_hash), `HASH_UPD` (rendition_id, old_version, new_version,
//!   algorithm, digest).
//! - **Privacy:** see [`types`] -- hash + pseudonymous address only;
//!   metadata manifests, content files, and access URLs never land
//!   on-chain, only their content-addressed commitments.
//! - **Deployment:** additive evolution of the existing library-rights
//!   contract; no existing entry point or storage layout is replaced.
//! - **Migration:** none required -- new keys are additive. Future
//!   schema changes bump [`keys::SCHEMA_VERSION`].

mod content;
mod errors;
mod governance;
mod keys;
mod metadata;
mod registry;
mod types;

pub use errors::ContractError;
pub use keys::{DataKey, Role};
pub use types::{
    CatalogEntry, ChildrenPage, ContentCommitment, ContentState, EntryKind, HashAlgorithm,
    MetadataCommitment, VersionSnapshot, WorkRecord,
};

use keys::{DataKey as DK, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

const CONTRACT_VERSION: &str = "0.6.0";

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

    /// #928 — registers a canonical work: id -> bounded metadata
    /// commitment + pseudonymous custodian. PolicyManager-only; rejects
    /// all-zero ids/hashes, rejects duplicate ids, renews TTL, and
    /// publishes a versioned `WRK_NEW` event.
    pub fn register_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        metadata: MetadataCommitment,
        custodian: Address,
    ) -> Result<u32, ContractError> {
        registry::register_work(&env, &caller, &work_id, &metadata, &custodian)
    }

    /// #929 — registers an edition under an existing work.
    /// PolicyManager-only; the parent must be a work and the id unused.
    pub fn register_edition(
        env: Env,
        caller: Address,
        parent_work_id: BytesN<32>,
        edition_id: BytesN<32>,
        metadata: MetadataCommitment,
        custodian: Address,
    ) -> Result<u32, ContractError> {
        registry::register_edition(
            &env,
            &caller,
            &parent_work_id,
            &edition_id,
            &metadata,
            &custodian,
        )
    }

    /// #929 + #932 — registers a rendition under an existing edition with
    /// its algorithm-tagged content hash. PolicyManager-only; the parent
    /// must be an edition and the id unused.
    pub fn register_rendition(
        env: Env,
        caller: Address,
        parent_edition_id: BytesN<32>,
        rendition_id: BytesN<32>,
        content: ContentCommitment,
        metadata: MetadataCommitment,
        custodian: Address,
    ) -> Result<u32, ContractError> {
        registry::register_rendition(
            &env,
            &caller,
            &parent_edition_id,
            &rendition_id,
            &content,
            &metadata,
            &custodian,
        )
    }

    /// #933 — updates the metadata commitment of any entry, creating a
    /// new version. The previous version stays immutable in the history.
    pub fn update_metadata(
        env: Env,
        caller: Address,
        entry_id: BytesN<32>,
        metadata: MetadataCommitment,
    ) -> Result<u32, ContractError> {
        registry::update_metadata(&env, &caller, &entry_id, &metadata)
    }

    /// #932 — replaces the content commitment of a rendition, creating a
    /// new version. Rejected for non-rendition entries.
    pub fn update_content_hash(
        env: Env,
        caller: Address,
        rendition_id: BytesN<32>,
        content: ContentCommitment,
    ) -> Result<u32, ContractError> {
        registry::update_content_hash(&env, &caller, &rendition_id, &content)
    }

    /// Returns the current catalog entry for `entry_id`, renewing its
    /// TTL.
    pub fn entry(env: Env, entry_id: BytesN<32>) -> Result<CatalogEntry, ContractError> {
        registry::entry(&env, &entry_id)
    }

    /// Returns the immutable snapshot for `version` (1-based) of
    /// `entry_id`, queryable only within bounds.
    pub fn entry_version(
        env: Env,
        entry_id: BytesN<32>,
        version: u32,
    ) -> Result<VersionSnapshot, ContractError> {
        registry::get_version(&env, &entry_id, version)
    }

    /// Returns how many versions have been recorded for `entry_id`.
    pub fn entry_version_count(env: Env, entry_id: BytesN<32>) -> u32 {
        registry::version_count(&env, &entry_id)
    }

    /// #929 — cursor/limit-bounded children query for `parent_id`.
    pub fn children(
        env: Env,
        parent_id: BytesN<32>,
        cursor: u32,
        limit: u32,
    ) -> Result<ChildrenPage, ContractError> {
        registry::children(&env, &parent_id, cursor, limit)
    }

    /// #932 — read-only verification of a rendition's current content
    /// commitment against `(algorithm, digest)`.
    pub fn verify_content(
        env: Env,
        rendition_id: BytesN<32>,
        algorithm: HashAlgorithm,
        digest: BytesN<32>,
    ) -> Result<bool, ContractError> {
        content::verify_content(&env, &rendition_id, algorithm, &digest)
    }

    /// Returns this contract's ABI version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod tests;
