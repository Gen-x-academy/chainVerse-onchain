//! Global emergency pause (#997).
//!
//! ## Scope
//!
//! A compromised issuer, accounting defect, or coordinated exploit needs
//! an immediate containment mechanism. Pausing the library freezes every
//! operation that *creates* a lending, membership, or configuration
//! obligation while deliberately leaving *reducing* operations (returns,
//! hold cancellations, refunds) and read-only queries available, so
//! patrons can still unwind their positions during an incident.
//!
//! ### Blocked while paused (obligation-creating / configuration)
//!
//! - **New loans:** `checkout_work`, `checkout`, `borrow_work`,
//!   `borrow_from_reserve`, `claim_hold`.
//! - **New holds and reserves:** `place_hold`, `create_reserve`.
//! - **Catalog and policy writes:** `put_work`, `put_policy`,
//!   `append_policy`, `register_license`, `register_rendition`,
//!   `register_seat`, `register_work`, `register_edition`,
//!   `update_metadata`, `update_content_hash`, `commit_classification`,
//!   `attest_provenance`.
//! - **Membership and renewal obligations:** `attest_membership`,
//!   `renew_loan`.
//! - **Configuration:** `add_keeper`, `remove_keeper`,
//!   `set_course_registry`.
//!
//! ### Still allowed while paused (reducing / admin / read)
//!
//! - **Returns and refunds:** `return_work`, `return_to_reserve`,
//!   `cancel_hold`. These reduce obligations and must never be blocked.
//! - **Emergency content controls:** `quarantine_work`,
//!   `restore_quarantined_work`, `deactivate_work`, `legal_takedown_work`
//!   stay available because quarantine/takedown *is* the containment
//!   response.
//! - **Read-only queries and `version()`** are unaffected.
//! - **Pause administration itself:** `pause_library` must always be
//!   reachable, and governed `unpause_library` must be reachable once the
//!   incident is resolved.
//!
//! ## Authorization
//!
//! - `pause`: `Admin` or `Emergency`, a single authenticated call, and the
//!   caller must attach a non-zero `reason_hash` (privacy: only the hash
//!   of the incident write-up is ever stored).
//! - `unpause`: `Admin` only (governed restoration), also authenticated
//!   and reason-bearing.
//!
//! ## Storage
//!
//! Instance storage (no TTL, survives infinitely):
//! `Paused(bool)`, `PausedAt(u64)`, `PausedBy(Address)`,
//! `PauseReason(BytesN<32>)`. All vanished on unpause.
//!
//! ## Events
//!
//! - `PAUSED` `(paused_at, paused_by, reason_hash)`.
//! - `UNPAUSED` `(unpaused_at, unpaused_by, reason_hash)`.
//!
//! ## Privacy
//!
//! Only a hash of the pause reason is stored; the full incident write-up
//! lives off-chain and is referenced by its content address.
//!
//! ## Deployment & migration
//!
//! Purely additive: new instance keys, new events, and a new error
//! discriminant (`Paused = 200`). No existing key layout or entrypoint
//! signature changes, so `SCHEMA_VERSION` is unchanged.

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::Role;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PauseKey {
    Paused,
    PausedAt,
    PausedBy,
    PauseReason,
}

/// Snapshot of the pause state, exposed for dashboards and monitoring.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PauseStatus {
    pub paused: bool,
    pub paused_at: u64,
    pub paused_by: Option<Address>,
    pub reason_hash: Option<BytesN<32>>,
}

/// Whether the library is currently paused.
pub fn is_paused(env: &Env) -> bool {
    env.storage().instance().get::<_, bool>(&PauseKey::Paused)
        .unwrap_or(false)
}

/// Guard called at the top of every obligation-creating and
/// configuration entrypoint. Succeeds while the library is not paused.
pub fn ensure_active(env: &Env) -> Result<(), ContractError> {
    if is_paused(env) {
        return Err(ContractError::Paused);
    }
    Ok(())
}

/// Pauses the library. `Admin` or `Emergency`, single authenticated call.
pub fn pause(
    env: &Env,
    caller: Address,
    reason_hash: BytesN<32>,
) -> Result<(), ContractError> {
    let is_admin = governance::has_role(env, Role::Admin, &caller).unwrap_or(false);
    let is_emergency = governance::has_role(env, Role::Emergency, &caller).unwrap_or(false);
    if !is_admin && !is_emergency {
        return Err(ContractError::NotAdmin);
    }
    caller.require_auth();

    if reason_hash.to_array() == [0u8; 32] {
        return Err(ContractError::InvalidHash);
    }

    if is_paused(env) {
        return Err(ContractError::AlreadyPaused);
    }

    let now = env.ledger().timestamp();
    env.storage().instance().set(&PauseKey::Paused, &true);
    env.storage().instance().set(&PauseKey::PausedAt, &now);
    env.storage().instance().set(&PauseKey::PausedBy, &caller.clone());
    env.storage().instance().set(&PauseKey::PauseReason, &reason_hash.clone());

    env.events().publish(
        (symbol_short!("PAUSED"),),
        (now, caller, reason_hash),
    );

    Ok(())
}

/// Governed unpause. `Admin` only, reason-bearing for audit.
pub fn unpause(
    env: &Env,
    caller: Address,
    reason_hash: BytesN<32>,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    if reason_hash.to_array() == [0u8; 32] {
        return Err(ContractError::InvalidHash);
    }

    if !is_paused(env) {
        return Err(ContractError::NotPaused);
    }

    let now = env.ledger().timestamp();
    env.storage().instance().remove(&PauseKey::Paused);
    env.storage().instance().remove(&PauseKey::PausedAt);
    env.storage().instance().remove(&PauseKey::PausedBy);
    env.storage().instance().remove(&PauseKey::PauseReason);

    env.events().publish(
        (symbol_short!("UNPAUSED"),),
        (now, caller, reason_hash),
    );

    Ok(())
}

/// Status snapshot for queries and monitors.
pub fn status(env: &Env) -> PauseStatus {
    PauseStatus {
        paused: is_paused(env),
        paused_at: env.storage().instance().get(&PauseKey::PausedAt).unwrap_or(0),
        paused_by: env.storage().instance().get(&PauseKey::PausedBy),
        reason_hash: env.storage().instance().get(&PauseKey::PauseReason),
    }
}