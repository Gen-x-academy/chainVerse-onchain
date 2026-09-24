//! Semantic storage/ABI versions and the approved WASM commitment (#999).
//!
//! Library clients need to verify that the deployed contract ABI matches
//! what their SDK expects, and that the code on-chain is the code the
//! issuer approved, *before* submitting transactions. This module exposes
//! three stable facts and one governed mutation:
//!
//! - **`schema_version`** — the current storage schema
//!   (`keys::SCHEMA_VERSION`, stamped once at bootstrap). A bump in this
//!   value is the signal to consult an upgrade/migration path before
//!   trusting decoded values.
//! - **`abi_version`** — the semantic ABI revision of the *entrypoint
//!   surface* exposed by this build (independent of storage schema).
//!   Incremented only in a breaking-interface release.
//! - **`approved_wasm_hash`** — the SHA-256 digest of the WASM blob the
//!   issuer currently approves for deployment. It starts unset (all-zero)
//!   and changes *only* through an `Admin`-authenticated,
//!   reason-free-but-auditable `approve_wasm_hash` call that publishes
//!   the previous hash, the new hash, the ledger time, and the caller in
//!   the upgrade event for indexers and deployment tooling.
//!
//! `deploy_info` bundles all three for a single compatibility probe.
//!
//! ## Authorization
//!
//! `approve_wasm_hash`: `Admin` role, authenticated. Zero digests are
//! rejected (all-zero is the "unset" sentinel and can never be a real
//! approval).
//!
//! ## Storage
//!
//! - `DataKey::SchemaVersion` (instance, written by bootstrap).
//! - `InfoKey::ApprovedWasmHash` (instance, written only by
//!   `approve_wasm_hash`).
//!
//! ## Events
//!
//! `UPGRADE_AUTH` `(previous_hash, new_hash, approved_at, approved_by)`.
//!
//! ## Privacy
//!
//! No personal data is involved; only public code/schema identifiers are
//! exposed.
//!
//! ## Deployment & migration
//!
//! Additive. `schema_version` tracks the storage layout; `abi_version`
//! tracks the interface revision; the approved WASM hash is what
//! deployment tooling checks before pushing a new blob. No existing
//! storage layout changes.

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role};

/// Semantic ABI revision of the entrypoint surface in this release.
/// Bumped only for breaking interface changes.
pub const ABI_VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InfoKey {
    ApprovedWasmHash,
}

/// A single compatibility probe: schema + ABI revision + approved WASM.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployInfo {
    pub schema_version: u32,
    pub abi_version: u32,
    pub approved_wasm_hash: BytesN<32>,
}

fn approved_hash(env: &Env) -> BytesN<32> {
    env.storage()
        .instance()
        .get::<_, BytesN<32>>(&InfoKey::ApprovedWasmHash)
        .unwrap_or(BytesN::from_array(env, &[0; 32]))
}

/// The current storage schema version; 0 before bootstrap.
pub fn schema_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get::<_, u32>(&DataKey::SchemaVersion)
        .unwrap_or(0)
}

/// The semantic ABI revision of this build's entrypoint surface.
pub fn abi_version(_env: &Env) -> u32 {
    ABI_VERSION
}

/// The approved WASM commitment (all-zero unless authorized).
pub fn approved_wasm_hash(env: &Env) -> BytesN<32> {
    approved_hash(env)
}

/// The bundled probe for deployment tooling.
pub fn deploy_info(env: &Env) -> DeployInfo {
    DeployInfo {
        schema_version: schema_version(env),
        abi_version: ABI_VERSION,
        approved_wasm_hash: approved_hash(env),
    }
}

/// Governed update of the approved WASM commitment. `Admin` only.
///
/// Publishes the previous and new digests in the upgrade event so
/// deployment pipelines can validate and so indexers can reconstruct the
/// full approval history.
pub fn approve_wasm_hash(
    env: &Env,
    caller: Address,
    new_hash: BytesN<32>,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    // The all-zero digest is the "unset" sentinel and can never become a
    // real approval; a valid content address is required.
    if new_hash.to_array() == [0u8; 32] {
        return Err(ContractError::InvalidHash);
    }

    let previous = approved_hash(env);
    let approved_at = env.ledger().timestamp();
    env.storage()
        .instance()
        .set(&InfoKey::ApprovedWasmHash, &new_hash.clone());

    env.events().publish(
        (symbol_short!("UPGRADE_AUTH"),),
        (previous, new_hash, approved_at, caller),
    );

    Ok(())
}