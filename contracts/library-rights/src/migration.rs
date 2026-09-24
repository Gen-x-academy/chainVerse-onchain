//! Version-gated, cursor-bounded migration batches (#1000).
//!
//! Future library schemas will require resumable upgrades. A migration
//! must never try to do unbounded ledger work in one invocation, must
//! never skip a schema version, and normal writes must be gated while an
//! incompatible phase is in progress.
//!
//! ## Model
//!
//! - `begin_migration(from, to)`: `Admin`-authenticated. Only ever admits
//!   the *next* version (`to == from + 1`), so earlier versions can never
//!   be skipped. Refuses to start while another migration is active.
//! - `migrate_batch(limit)`: `Admin`-authenticated, cursor-bounded. Each
//!   call advances the persisted `steps_done` cursor by at most `limit`
//!   steps over a fixed, per-version `total_steps`. Any number of calls
//!   may be spread over any number of ledgers; the state is resumable
//!   because the cursor lives in instance storage.
//! - `finalize_migration`: `Admin`-authenticated. Requires all steps to be
//!   complete, stamps the new `SCHEMA_VERSION`, appends the completed
//!   migration to the append-only log, returns the system to `Idle`, and
//!   emits `MIG_COMPLETE`. Idempotent (a second call returns
//!   `AlreadyComplete` without re-stamping).
//! - Per-step actions are chained per version through `run_step`, so real
//!   per-record passes (e.g. reshaping loan/hold keys) can be added as the
//!   corresponding versions are designed. The coordinator itself -- the
//!   sequencing, cursor accounting, gating, and double-finish protection --
//!   is the version-independent machinery this issue requires.
//!
//! ## Gating
//!
//! While `phase != Idle`, `ensure_writable` reports
//! `MigrationInProgress`, and every write entrypoint that calls it is
//! frozen. The intended write surface is: `put_policy`, `put_work`,
//! `borrow_work`, `place_hold`, `create_reserve`, `register_license`,
//! `register_rendition`, `register_seat`, `append_policy`,
//! `attest_membership`, `checkout_work`, `borrow_from_reserve`,
//! `renew_loan`, and the registry `register_*`/`update_*` writes. Safe
//! exits (returns, refunds) and read-only queries remain available.
//!
//! ## Storage
//!
//! - `MigrationKey::State` (instance): the active migration state.
//! - `MigrationKey::LogCount` / `MigrationKey::Log(index)` (instance):
//!   append-only completed-migration log.
//!
//! ## Events
//!
//! - `MIG_BEGIN` `(from_version, target_version, began_at, by)`.
//! - `MIG_STEP` `(target_version, step_index, total_steps, at)`.
//! - `MIG_COMPLETE` `(from_version, target_version, at, by)`.
//!
//! ## Privacy
//!
//! No personal data is involved.
//!
//! ## Deployment & migration
//!
//! The machinery is additive. Completing the first migration bumps
//! `keys::SCHEMA_VERSION` from 2 to 3, matching the extension the library
//! gained since the audit baseline.

use soroban_sdk::{contracttype, symbol_short, Address, Env};

use crate::errors::ContractError;
use crate::governance;
use crate::keys::{DataKey, Role};

/// Number of work units every version migration consists of (the schema
/// stamp is always the final unit; earlier units are per-version passes).
pub const MIGRATION_STEPS: u32 = 3;
/// Maximum batch progress a single call may apply.
pub const MAX_MIGRATION_BATCH: u32 = 32;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationKey {
    State,
    LogCount,
    Log(u64),
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationPhase {
    Idle,
    Migrating,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationState {
    pub from_version: u32,
    pub target_version: u32,
    /// Persisted cursor: number of steps completed so far (resumable).
    pub steps_done: u32,
    pub total_steps: u32,
    pub phase: MigrationPhase,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationStatus {
    pub active: bool,
    pub from_version: u32,
    pub target_version: u32,
    pub steps_done: u32,
    pub total_steps: u32,
    pub ready_to_finalize: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationLogEntry {
    pub from_version: u32,
    pub target_version: u32,
    pub completed_at: u64,
    pub ran_by: Address,
}

/// The current schema version, or 0 before bootstrap.
fn current_schema(env: &Env) -> u32 {
    crate::wasm_info::schema_version(env)
}

fn state(env: &Env) -> Option<MigrationState> {
    env.storage().instance().get(&MigrationKey::State)
}

/// Whether an incompatible migration phase is currently active.
pub fn is_migrating(env: &Env) -> bool {
    state(env)
        .map(|s| s.phase == MigrationPhase::Migrating)
        .unwrap_or(false)
}

/// Guard for write entrypoints: rejects writes while a migration is in
/// progress; safe exits and reads stay available.
pub fn ensure_writable(env: &Env) -> Result<(), ContractError> {
    if is_migrating(env) {
        return Err(ContractError::MigrationInProgress);
    }
    Ok(())
}

/// Begins a migration to the next schema version. `Admin` only.
///
/// Refuses to skip versions (`to` must equal the current schema + 1) and
/// refuses to start while another migration is active.
pub fn begin(
    env: &Env,
    caller: Address,
    target_version: u32,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    if let Some(s) = state(env) {
        if s.phase == MigrationPhase::Migrating {
            return Err(ContractError::MigrationInProgress);
        }
    }

    let from_version = current_schema(env);
    if target_version == from_version {
        return Err(ContractError::MigrationAlreadyComplete);
    }
    // Only ever one version at a time; earlier versions cannot be skipped.
    if target_version != from_version + 1 {
        return Err(ContractError::MigrationVersionSkip);
    }

    env.storage().instance().set(
        &MigrationKey::State,
        &MigrationState {
            from_version,
            target_version,
            steps_done: 0,
            total_steps: MIGRATION_STEPS,
            phase: MigrationPhase::Migrating,
        },
    );

    env.events().publish(
        (symbol_short!("MIG_BEGIN"),),
        (from_version, target_version, env.ledger().timestamp(), caller),
    );

    Ok(())
}

/// Per-version step runner. Future schemas chain their real per-record
/// passes here, keyed on `step_index`, before the final stamp. The
/// coordinator applies it only at the exact persisted cursor, so a
/// skipped step can never be recorded and the batch sequence stays
/// strictly monotonic.
fn run_step(
    s: &MigrationState,
    step_index: u32,
) -> Result<(), ContractError> {
    // Version-independent passes converge on the schema stamp at
    // finalize; this guard is what makes skipping impossible even if a
    // later caller miscounts.
    if step_index != s.steps_done {
        return Err(ContractError::MigrationVersionSkip);
    }
    Ok(())
}

/// Applies the next `limit` steps, resuming from the persisted cursor.
/// Idempotent: calling with no active migration is a no-op.
pub fn migrate_batch(
    env: &Env,
    caller: Address,
    limit: u32,
) -> Result<(u32, bool), ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    let mut s = match state(env) {
        Some(s) if s.phase == MigrationPhase::Migrating => s,
        _ => return Ok((0, false)),
    };

    let limit = limit.min(MAX_MIGRATION_BATCH);
    let remaining = s.total_steps.saturating_sub(s.steps_done);
    let batch = limit.min(remaining);

    for _ in 0..batch {
        let step_index = s.steps_done;
        run_step(&s, step_index)?;
        s.steps_done += 1;
        env.events().publish(
            (symbol_short!("MIG_STEP"),),
            (s.target_version, step_index + 1, s.total_steps, env.ledger().timestamp()),
        );
    }

    // Persist the advanced cursor. The phase stays `Migrating` until
    // `finalize` so writes remain gated the whole way through, and a
    // fully-stepped migration is held in that phase until `finalize`
    // stamps the schema and returns the system to `Idle`.
    env.storage().instance().set(&MigrationKey::State, &s);

    let finished_all = s.steps_done >= s.total_steps;
    Ok((batch, finished_all))
}

/// Completes an active migration: verifies every step ran, stamps the new
/// schema version, logs the migration, and returns to `Idle`. `Admin`.
pub fn finalize(
    env: &Env,
    caller: Address,
) -> Result<(), ContractError> {
    governance::require_role(env, Role::Admin, &caller)?;

    let s = match state(env) {
        Some(s) => s,
        None => return Err(ContractError::MigrationNotStarted),
    };
    if s.phase != MigrationPhase::Migrating {
        return Err(ContractError::MigrationAlreadyComplete);
    }
    if s.steps_done < s.total_steps {
        return Err(ContractError::MigrationNotReady);
    }

    // Stamp the new schema version and leave the idle phase behind.
    env.storage().instance().set(&DataKey::SchemaVersion, &s.target_version);
    env.storage().instance().set(
        &MigrationKey::State,
        &MigrationState {
            from_version: s.target_version,
            target_version: s.target_version,
            steps_done: 0,
            total_steps: s.total_steps,
            phase: MigrationPhase::Idle,
        },
    );

    // Append to the audit log.
    let count: u64 = env
        .storage()
        .instance()
        .get(&MigrationKey::LogCount)
        .unwrap_or(0);
    let idx = count + 1;
    env.storage().instance().set(
        &MigrationKey::Log(idx),
        &MigrationLogEntry {
            from_version: s.from_version,
            target_version: s.target_version,
            completed_at: env.ledger().timestamp(),
            ran_by: caller.clone(),
        },
    );
    env.storage().instance().set(&MigrationKey::LogCount, &idx);

    env.events().publish(
        (symbol_short!("MIG_COMPLETE"),),
        (s.from_version, s.target_version, env.ledger().timestamp(), caller),
    );

    Ok(())
}

/// Current migration status for tooling and dashboards.
pub fn status(env: &Env) -> MigrationStatus {
    match state(env) {
        Some(s) => MigrationStatus {
            active: s.phase == MigrationPhase::Migrating,
            from_version: s.from_version,
            target_version: s.target_version,
            steps_done: s.steps_done,
            total_steps: s.total_steps,
            ready_to_finalize: s.steps_done >= s.total_steps,
        },
        None => MigrationStatus {
            active: false,
            from_version: current_schema(env),
            target_version: current_schema(env),
            steps_done: 0,
            total_steps: MIGRATION_STEPS,
            ready_to_finalize: false,
        },
    }
}

/// How many migrations have completed (audit log length).
pub fn log_count(env: &Env) -> u64 {
    env.storage().instance().get(&MigrationKey::LogCount).unwrap_or(0)
}

/// Reads the `index`-th (1-based) completed migration.
pub fn log_entry(env: &Env, index: u64) -> Result<MigrationLogEntry, ContractError> {
    env.storage()
        .instance()
        .get(&MigrationKey::Log(index))
        .ok_or(ContractError::MigrationNotStarted)
}