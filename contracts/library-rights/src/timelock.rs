//! Governed-change timelock (#995).
//!
//! High-impact governed actions (role transfers, course-registry
//! repointing, timelock reconfiguration) must not take effect instantly:
//! patrons and integrators need a reaction window. This module queues
//! typed actions behind a configurable delay and only executes them once
//! the delay has elapsed.
//!
//! ## Properties
//!
//! - **Bounded.** `min_delay`/`max_delay` are kept inside the constant
//!   window `[TIMELOCK_MIN_BOUND, TIMELOCK_MAX_BOUND]`, and the pending
//!   queue is capped at [`MAX_QUEUED_CHANGES`]. `eta` (the activation
//!   time) is validated against the *current* window at queue time.
//! - **Not bypassable by reproposal.** Every queued change carries a
//!   change id derived from the caller, the typed action, `eta`, the
//!   queue time, and a monotonic counter, so a fresh proposal always gets
//!   a fresh id and must serve its *own* full delay. While an equivalent
//!   proposal is pending, duplicates from the same proposer are rejected
//!   outright, and once executed the id is tombstoned in instance storage
//!   (no TTL) so it can never be replayed.
//! - **Explicit activation time.** `queue_change` records `eta` and emits
//!   `CHG_QUEUED(change_id, proposer, eta, queued_at)`; `pending_change`
//!   and `pending_changes` expose it to every integrator.
//!
//! ## Authorization
//!
//! - `queue`: `Admin` role, authenticated.
//! - `cancel`: the original proposer or `Admin`. An executing change
//!   cannot be cancelled.
//! - `execute`: anyone, authenticated, but only after `eta`; no privileged
//!   bot or keeper is required (execution is trustless).
//! - `TransferRole` additionally requires the *recipient* to authorize and
//!   accept the role at execution time.
//!
//! ## Storage
//!
//! Instance storage (no TTL, authoritative):
//! `Config(TimelockConfig)`, `Counter(u64)`, `PendingIds(Vec<BytesN<32>>)`,
//! `QueuedChange(BytesN<32>)`, `Executed(BytesN<32>)` tombstones.
//!
//! ## Events
//!
//! - `CHG_QUEUED` `(change_id, proposer, eta, queued_at)`.
//! - `CHG_CANCEL` `(change_id, cancelled_by, cancelled_at)`.
//! - `CHG_EXEC` `(change_id, executed_by, executed_at)`.
//! - `ROLE_XFER` `(role, to)` / `REG_REPT` `(registry)` /
//!   `TL_CFG` `(min_delay, max_delay)` on the dispatched action itself.
//!
//! ## Privacy
//!
//! Actions carry only public data (addresses, roles, numbers) -- no
//! personal metadata.
//!
//! ## Deployment & migration
//!
//! Purely additive: new instance keys, new events, new error
//! discriminants. No existing storage layout or entrypoint signature
//! changes; `SCHEMA_VERSION` is unchanged.

use soroban_sdk::{contracttype, symbol_short, Address, Bytes, BytesN, Env, Vec};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role, GOVERNANCE_MAX_TTL, GOVERNANCE_MIN_TTL};

/// Hard floor for a timelock delay (1 hour).
pub const TIMELOCK_MIN_BOUND: u64 = 60 * 60;
/// Hard ceiling for a timelock delay (180 days).
pub const TIMELOCK_MAX_BOUND: u64 = 180 * 24 * 60 * 60;
/// Default window installed on first use: 7 to 30 days.
pub const DEFAULT_MIN_DELAY: u64 = 7 * 24 * 60 * 60;
pub const DEFAULT_MAX_DELAY: u64 = 30 * 24 * 60 * 60;
/// Upper bound on concurrently pending changes.
pub const MAX_QUEUED_CHANGES: u32 = 64;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelockKey {
    Config,
    Counter,
    PendingIds,
    QueuedChange(BytesN<32>),
    Executed(BytesN<32>),
}

/// The configured delay window; defaulted, never stored until changed.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelockConfig {
    pub min_delay: u64,
    pub max_delay: u64,
}

/// The typed, high-impact changes that must pass through the timelock.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum GovernedAction {
    /// Transfer one governance role to a new address. The recipient must
    /// authorize acceptance at execution time.
    TransferRole { role: Role, to: Address },
    /// Repoint the course-registry integration address.
    UpdateCourseRegistry(Address),
    /// Reconfigure the timelock's own delay window (still bounded).
    UpdateTimelockConfig { min_delay: u64, max_delay: u64 },
}

/// A queued, pending governed change.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedChange {
    pub change_id: BytesN<32>,
    pub action: GovernedAction,
    pub proposer: Address,
    /// Ledger timestamp the change was queued at.
    pub queued_at: u64,
    /// Earliest ledger timestamp at which the change may execute.
    pub eta: u64,
    pub executed: bool,
    pub cancelled: bool,
}

/// The currently configured delay window, or the default window on first use.
pub fn config(env: &Env) -> TimelockConfig {
    env.storage()
        .instance()
        .get::<_, TimelockConfig>(&TimelockKey::Config)
        .unwrap_or(TimelockConfig {
            min_delay: DEFAULT_MIN_DELAY,
            max_delay: DEFAULT_MAX_DELAY,
        })
}

fn pending_ids(env: &Env) -> Vec<BytesN<32>> {
    env.storage()
        .instance()
        .get::<_, Vec<BytesN<32>>>(&TimelockKey::PendingIds)
        .unwrap_or_else(|| Vec::new(env))
}

fn action_salt(env: &Env, action: &GovernedAction) -> Bytes {
    let mut salt = Bytes::new(env);
    match action {
        GovernedAction::TransferRole { role, to } => {
            salt.append(&Bytes::from_slice(env, &[1]));
            salt.append(&Bytes::from_slice(env, &role_tag(*role)));
            salt.append(&to.to_xdr(env));
        }
        GovernedAction::UpdateCourseRegistry(registry) => {
            salt.append(&Bytes::from_slice(env, &[2]));
            salt.append(&registry.to_xdr(env));
        }
        GovernedAction::UpdateTimelockConfig { min_delay, max_delay } => {
            salt.append(&Bytes::from_slice(env, &[3]));
            salt.append(&Bytes::from_slice(env, &min_delay.to_be_bytes()));
            salt.append(&Bytes::from_slice(env, &max_delay.to_be_bytes()));
        }
    }
    salt
}

fn role_tag(role: Role) -> [u8; 4] {
    match role {
        Role::Admin => 1u32.to_be_bytes(),
        Role::Treasury => 2u32.to_be_bytes(),
        Role::PolicyManager => 3u32.to_be_bytes(),
        Role::Emergency => 4u32.to_be_bytes(),
        Role::Librarian => 5u32.to_be_bytes(),
    }
}

fn remove_from_pending(env: &Env, change_id: &BytesN<32>) {
    let mut kept = Vec::new(env);
    for id in pending_ids(env).iter() {
        if &id != change_id {
            kept.push_back(id.clone());
        }
    }
    env.storage().instance().set(&TimelockKey::PendingIds, &kept);
}

/// Validates and publishes the bounded delay window.
fn validate_and_store_config(
    env: &Env,
    min_delay: u64,
    max_delay: u64,
) -> Result<(), ContractError> {
    if min_delay < TIMELOCK_MIN_BOUND
        || max_delay > TIMELOCK_MAX_BOUND
        || min_delay > max_delay
    {
        return Err(ContractError::TimelockConfigInvalid);
    }
    env.storage().instance().set(
        &TimelockKey::Config,
        &TimelockConfig { min_delay, max_delay },
    );
    Ok(())
}

/// Queues a governed change behind the configured delay. `Admin` only.
pub fn queue(
    env: &Env,
    caller: Address,
    action: GovernedAction,
    eta: u64,
) -> Result<BytesN<32>, ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    let cfg = config(env);
    let now = env.ledger().timestamp();
    let min_acceptable = now
        .checked_add(cfg.min_delay)
        .ok_or(ContractError::InvalidTimestamp)?;
    let max_acceptable = now
        .checked_add(cfg.max_delay)
        .ok_or(ContractError::InvalidTimestamp)?;
    if eta < min_acceptable || eta > max_acceptable {
        return Err(ContractError::TimelockEtaInvalid);
    }

    let pending = pending_ids(env);
    if pending.len() >= MAX_QUEUED_CHANGES {
        return Err(ContractError::TimelockQueueFull);
    }
    // Reject duplicate pending proposals from the same proposer: a
    // reproposal cannot be used to stack or shortcut the delay.
    for id in pending.iter() {
        let queued = env.storage().instance().get::<_, QueuedChange>(
            &TimelockKey::QueuedChange(id.clone()),
        );
        if let Some(q) = queued {
            if q.proposer == caller && q.action == action && !q.cancelled {
                return Err(ContractError::TimelockAlreadyQueued);
            }
        }
    }

    let queued_at = now;
    let seq: u64 = env
        .storage()
        .instance()
        .get(&TimelockKey::Counter)
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(ContractError::Overflow)?;
    env.storage().instance().set(&TimelockKey::Counter, &seq);

    let mut salt = Bytes::new(env);
    salt.append(&caller.to_xdr(env));
    salt.append(&action_salt(env, &action));
    salt.append(&Bytes::from_slice(env, &eta.to_be_bytes()));
    salt.append(&Bytes::from_slice(env, &queued_at.to_be_bytes()));
    salt.append(&Bytes::from_slice(env, &seq.to_be_bytes()));
    let change_id: BytesN<32> = env.crypto().sha256(&salt).into();

    let change = QueuedChange {
        change_id: change_id.clone(),
        action,
        proposer: caller.clone(),
        queued_at,
        eta,
        executed: false,
        cancelled: false,
    };
    env.storage()
        .instance()
        .set(&TimelockKey::QueuedChange(change_id.clone()), &change);

    let mut pending = pending_ids(env);
    pending.push_back(change_id.clone());
    env.storage().instance().set(&TimelockKey::PendingIds, &pending);

    env.events().publish(
        (symbol_short!("CHG_QUEUED"),),
        (change_id.clone(), caller, eta, queued_at),
    );

    Ok(change_id)
}

/// Cancels a pending change before it executes. The original proposer or
/// `Admin`; executing or already-executed changes are not cancellable.
pub fn cancel(
    env: &Env,
    caller: Address,
    change_id: BytesN<32>,
) -> Result<(), ContractError> {
    let key = TimelockKey::QueuedChange(change_id.clone());
    let mut change: QueuedChange = env
        .storage()
        .instance()
        .get(&key)
        .ok_or(ContractError::TimelockNotFound)?;

    let is_admin = governance::has_role(env, Role::Admin, &caller).unwrap_or(false);
    if caller != change.proposer && !is_admin {
        return Err(ContractError::NotAdmin);
    }
    caller.require_auth();

    if change.executed {
        return Err(ContractError::TimelockAlreadyExecuted);
    }
    if change.cancelled {
        return Err(ContractError::TimelockCancelled);
    }

    change.cancelled = true;
    env.storage().instance().set(&key, &change);
    remove_from_pending(env, &change_id);

    env.events().publish(
        (symbol_short!("CHG_CANCEL"),),
        (change_id, caller, env.ledger().timestamp()),
    );

    Ok(())
}

/// Executes a pending change once its delay has elapsed. Anyone may
/// trigger execution; the recipient of a role transfer must still accept.
pub fn execute(
    env: &Env,
    caller: Address,
    change_id: BytesN<32>,
) -> Result<(), ContractError> {
    caller.require_auth();

    let key = TimelockKey::QueuedChange(change_id.clone());
    let mut change: QueuedChange = env
        .storage()
        .instance()
        .get(&key)
        .ok_or(ContractError::TimelockNotFound)?;

    if change.executed || env.storage().instance().get::<_, bool>(&TimelockKey::Executed(change_id.clone())).unwrap_or(false) {
        return Err(ContractError::TimelockAlreadyExecuted);
    }
    if change.cancelled {
        return Err(ContractError::TimelockCancelled);
    }
    if env.ledger().timestamp() < change.eta {
        return Err(ContractError::TimelockNotReady);
    }

    dispatch(env, &change.action)?;

    change.executed = true;
    env.storage().instance().set(&key, &change);
    // Tombstone: executed changes live forever and can never be replayed.
    env.storage()
        .instance()
        .set(&TimelockKey::Executed(change_id.clone()), &true);
    remove_from_pending(env, &change_id);

    env.events().publish(
        (symbol_short!("CHG_EXEC"),),
        (change_id, caller, env.ledger().timestamp()),
    );

    Ok(())
}

fn dispatch(env: &Env, action: &GovernedAction) -> Result<(), ContractError> {
    match action {
        GovernedAction::TransferRole { role, to } => {
            let key = DataKey::Role(*role);
            if let Some(current) = env.storage().persistent().get::<_, Address>(&key) {
                if current == *to {
                    // No-op transfer; the change is a no-op.
                    return Ok(());
                }
            }
            // The recipient must authorize its own acceptance.
            to.require_auth();
            env.storage().persistent().set(&key, to);
            env.storage().persistent().extend_ttl(&key, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);
            env.events()
                .publish((symbol_short!("ROLE_XFER"),), (*role, to.clone()));
        }
        GovernedAction::UpdateCourseRegistry(registry) => {
            env.storage().instance().set(&DataKey::CourseRegistry, registry);
            env.events()
                .publish((symbol_short!("REG_REPT"),), registry.clone());
        }
        GovernedAction::UpdateTimelockConfig { min_delay, max_delay } => {
            let (min_delay, max_delay) = (*min_delay, *max_delay);
            validate_and_store_config(env, min_delay, max_delay)?;
            env.events()
                .publish((symbol_short!("TL_CFG"),), (min_delay, max_delay));
        }
    }
    Ok(())
}

/// Reads a single queued change.
pub fn pending_change(env: &Env, change_id: BytesN<32>) -> Result<QueuedChange, ContractError> {
    env.storage()
        .instance()
        .get(&TimelockKey::QueuedChange(change_id))
        .ok_or(ContractError::TimelockNotFound)
}

/// Lists the ids of all currently pending (not yet executed/cancelled)
/// changes, bounded by [`MAX_QUEUED_CHANGES`].
pub fn pending_ids_list(env: &Env) -> Vec<BytesN<32>> {
    pending_ids(env)
}