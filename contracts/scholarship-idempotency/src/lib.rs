#![no_std]
#![allow(clippy::too_many_arguments)]

//! Scholarship idempotency contract.
//!
//! Supports safe retries for submissions, decisions, acceptance, milestones,
//! payouts, refunds, and notifications by binding an idempotency key to an
//! `(actor, operation)` pair and ensuring that concurrent or duplicate
//! retries produce exactly one outcome.
//!
//! ## Design
//!
//! - **Actor + operation binding**: a key is `BytesN<32>` derived by the
//!   caller. It is stored against the `(actor, operation)` pair, so a key
//!   created by actor A for operation X cannot be replayed by actor B or
//!   for operation Y. Concurrent retries on the same key return the first
//!   stored outcome without re-executing.
//! - **One outcome per key**: a key transitions `Pending → Succeeded |
//!   Failed`. A `Pending` key blocks concurrent execution (callers must
//!   wait, not re-submit). `Succeeded`/`Failed` keys are immutable.
//! - **Bounded retention**: keys have a configurable TTL in seconds, capped
//!   at `MAX_KEY_TTL_SECONDS`. After the TTL the storage entry may expire
//!   from the ledger; the contract does not keep any unbounded index.
//! - **Checked arithmetic**: all counter increments and TTL calculations
//!   are checked.
//!
//! ## Privacy / ownership notes
//!
//! Only `Address`, a discriminant `Symbol` (operation name), `BytesN<32>`
//! (the key), and numeric metadata are stored. No payload, answer, or
//! private data is written. Callers must derive the key off-chain using
//! a secure hash of their own data.
//!
//! ## Migration notes
//!
//! Adding new `Operation` variants does not affect existing keys. Keys are
//! keyed by `(actor, operation, key_bytes)` so they are independent across
//! actors and operations.
//!
//! ## Adversarial tests
//!
//! See `tests/` for cross-actor key theft attempts, concurrent retry
//! serialization, expired key re-use, and bounded-retention enforcement.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol,
};

const CONTRACT_VERSION: u32 = 1;

/// Hard ceiling on a key's TTL to prevent infinite retention.
const MAX_KEY_TTL_SECONDS: u64 = 7_776_000; // 90 days

/// Default TTL in seconds if the caller does not specify one.
const DEFAULT_KEY_TTL_SECONDS: u64 = 86_400; // 24 hours

/// Minimum TTL ledgers for storage extension (~6 hours at 5 s/ledger).
const KEY_MIN_TTL: u32 = 4_320;
/// Maximum TTL ledgers for storage extension (~90 days at 5 s/ledger).
const KEY_MAX_TTL: u32 = 1_555_200;

/// Maximum concurrent Pending keys per (actor, operation). This bounds
/// storage abuse and makes "am I in flight?" queries tractable.
const MAX_PENDING_PER_ACTOR_OP: u32 = 32;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The idempotency key was created by a different actor or for a different
    /// operation — cross-actor replay is rejected.
    KeyActorMismatch = 4,
    /// The idempotency key is already in `Pending` state (a prior execution is
    /// in flight). The caller should wait and retry.
    KeyInFlight = 5,
    /// The key does not exist (may have expired).
    KeyNotFound = 6,
    /// The key has already been completed (`Succeeded` or `Failed`) and is
    /// immutable.
    KeyAlreadyCompleted = 7,
    /// The requested TTL exceeds `MAX_KEY_TTL_SECONDS`.
    TtlTooLong = 8,
    /// TTL must be > 0.
    InvalidTtl = 9,
    /// Per-(actor, operation) pending ceiling reached.
    PendingLimitExceeded = 10,
    /// Arithmetic overflow (practically unreachable).
    Overflow = 11,
}

// ---------------------------------------------------------------------------
// Operation identifiers
// ---------------------------------------------------------------------------

/// The operations that require idempotency protection.
/// Additional operations can be added by the admin via `register_operation`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Submission = 1,
    Decision = 2,
    Acceptance = 3,
    Milestone = 4,
    Payout = 5,
    Refund = 6,
    Notification = 7,
}

impl Operation {
    pub fn as_symbol(self, env: &Env) -> Symbol {
        match self {
            Operation::Submission    => Symbol::new(env, "submission"),
            Operation::Decision      => Symbol::new(env, "decision"),
            Operation::Acceptance    => Symbol::new(env, "acceptance"),
            Operation::Milestone     => Symbol::new(env, "milestone"),
            Operation::Payout        => Symbol::new(env, "payout"),
            Operation::Refund        => Symbol::new(env, "refund"),
            Operation::Notification  => Symbol::new(env, "notification"),
        }
    }
}

// ---------------------------------------------------------------------------
// Key lifecycle
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyStatus {
    /// In-flight: no result yet.
    Pending = 1,
    /// Completed successfully.
    Succeeded = 2,
    /// Completed with a failure.
    Failed = 3,
}

/// One idempotency key record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdempotencyRecord {
    /// The key bytes supplied by the caller.
    pub key: BytesN<32>,
    /// The actor who created the key.
    pub actor: Address,
    /// The operation this key is bound to.
    pub operation: Symbol,
    pub status: KeyStatus,
    pub created_at: u64,
    /// Ledger timestamp after which the key may be considered expired.
    pub expires_at: u64,
    /// Set when the key transitions to Succeeded or Failed.
    pub completed_at: u64,
    /// Optional caller-supplied result commitment (e.g. hash of the outcome).
    /// Zero bytes means no commitment was provided.
    pub result_hash: BytesN<32>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Idempotency record: (actor, operation_symbol, key_bytes).
    Record(Address, Symbol, BytesN<32>),
    /// Per-(actor, operation) pending count for ceiling enforcement.
    PendingCount(Address, Symbol),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ScholarshipIdempotencyContract;

#[contractimpl]
impl ScholarshipIdempotencyContract {
    /// One-time bootstrap.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: transfer admin role to a new address.
    pub fn transfer_admin(env: Env, admin: Address, new_admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    // ── key creation ──────────────────────────────────────────────────────

    /// Create an idempotency key for `(actor, operation)` with a given TTL.
    ///
    /// - If the key already exists and is `Pending`, returns `KeyInFlight`.
    /// - If the key already exists and is completed, returns
    ///   `KeyAlreadyCompleted` with the stored record unchanged.
    /// - If the key does not exist (or has expired and been evicted), creates
    ///   a new `Pending` record.
    ///
    /// `ttl_seconds` is capped at `MAX_KEY_TTL_SECONDS`. Pass `0` to use
    /// the default (`DEFAULT_KEY_TTL_SECONDS`).
    pub fn create_key(
        env: Env,
        actor: Address,
        operation: Operation,
        key: BytesN<32>,
        ttl_seconds: u64,
    ) -> Result<IdempotencyRecord, ContractError> {
        actor.require_auth();
        let op_sym = operation.as_symbol(&env);
        let effective_ttl = if ttl_seconds == 0 {
            DEFAULT_KEY_TTL_SECONDS
        } else {
            if ttl_seconds > MAX_KEY_TTL_SECONDS {
                return Err(ContractError::TtlTooLong);
            }
            ttl_seconds
        };

        let record_key = DataKey::Record(actor.clone(), op_sym.clone(), key.clone());

        // Idempotency: return existing record if key is known.
        if let Some(existing) = env.storage().persistent().get::<DataKey, IdempotencyRecord>(&record_key) {
            // Actor binding: reject if a different actor somehow constructed
            // the same key bytes (defense-in-depth).
            if existing.actor != actor {
                return Err(ContractError::KeyActorMismatch);
            }
            match existing.status {
                KeyStatus::Pending => return Err(ContractError::KeyInFlight),
                KeyStatus::Succeeded | KeyStatus::Failed => {
                    return Err(ContractError::KeyAlreadyCompleted)
                }
            }
        }

        // Enforce per-(actor, operation) pending ceiling.
        let count_key = DataKey::PendingCount(actor.clone(), op_sym.clone());
        let count: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u32);
        if count >= MAX_PENDING_PER_ACTOR_OP {
            return Err(ContractError::PendingLimitExceeded);
        }

        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(effective_ttl)
            .ok_or(ContractError::Overflow)?;
        let zero_hash = BytesN::from_array(&env, &[0u8; 32]);

        let record = IdempotencyRecord {
            key: key.clone(),
            actor: actor.clone(),
            operation: op_sym.clone(),
            status: KeyStatus::Pending,
            created_at: now,
            expires_at,
            completed_at: 0,
            result_hash: zero_hash,
        };

        env.storage().persistent().set(&record_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&record_key, KEY_MIN_TTL, KEY_MAX_TTL);

        let new_count = count.checked_add(1).ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&count_key, &new_count);
        env.storage()
            .persistent()
            .extend_ttl(&count_key, KEY_MIN_TTL, KEY_MAX_TTL);

        env.events().publish(
            (symbol_short!("IDMPNEW"),),
            (actor, op_sym, key),
        );
        Ok(record)
    }

    // ── key completion ────────────────────────────────────────────────────

    /// Complete a `Pending` key as `Succeeded`. Only the actor who created
    /// the key may complete it. Optionally attaches a `result_hash`
    /// commitment to the outcome.
    ///
    /// Concurrent retries: if two workers race to complete the same key,
    /// Soroban's serialized invocation model ensures exactly one write wins.
    /// The loser reads `KeyAlreadyCompleted` on its next call.
    pub fn complete_success(
        env: Env,
        actor: Address,
        operation: Operation,
        key: BytesN<32>,
        result_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        actor.require_auth();
        Self::transition(&env, &actor, &operation.as_symbol(&env), &key, KeyStatus::Succeeded, result_hash)
    }

    /// Complete a `Pending` key as `Failed`. Only the actor who created the
    /// key may mark it failed.
    pub fn complete_failure(
        env: Env,
        actor: Address,
        operation: Operation,
        key: BytesN<32>,
        result_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        actor.require_auth();
        Self::transition(&env, &actor, &operation.as_symbol(&env), &key, KeyStatus::Failed, result_hash)
    }

    fn transition(
        env: &Env,
        actor: &Address,
        op_sym: &Symbol,
        key: &BytesN<32>,
        new_status: KeyStatus,
        result_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        let record_key = DataKey::Record(actor.clone(), op_sym.clone(), key.clone());
        let mut record: IdempotencyRecord = env
            .storage()
            .persistent()
            .get(&record_key)
            .ok_or(ContractError::KeyNotFound)?;

        if record.actor != *actor {
            return Err(ContractError::KeyActorMismatch);
        }
        match record.status {
            KeyStatus::Pending => {}
            KeyStatus::Succeeded | KeyStatus::Failed => {
                return Err(ContractError::KeyAlreadyCompleted)
            }
        }

        record.status = new_status;
        record.completed_at = env.ledger().timestamp();
        record.result_hash = result_hash;

        env.storage().persistent().set(&record_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&record_key, KEY_MIN_TTL, KEY_MAX_TTL);

        // Decrement pending count.
        let count_key = DataKey::PendingCount(actor.clone(), op_sym.clone());
        let count: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u32);
        if count > 0 {
            env.storage().persistent().set(&count_key, &(count - 1));
        }

        let sym = match new_status {
            KeyStatus::Succeeded => symbol_short!("IDMPOK"),
            KeyStatus::Failed    => symbol_short!("IDMPFAIL"),
            KeyStatus::Pending   => symbol_short!("IDMPNEW"),
        };
        env.events().publish((sym,), (actor.clone(), op_sym.clone(), key.clone()));
        Ok(())
    }

    // ── key queries ───────────────────────────────────────────────────────

    pub fn get_record(
        env: Env,
        actor: Address,
        operation: Operation,
        key: BytesN<32>,
    ) -> Result<IdempotencyRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Record(actor, operation.as_symbol(&env), key))
            .ok_or(ContractError::KeyNotFound)
    }

    /// True iff a key exists AND is in `Pending` state (another execution is
    /// in flight).
    pub fn is_in_flight(env: Env, actor: Address, operation: Operation, key: BytesN<32>) -> bool {
        match env.storage().persistent().get::<DataKey, IdempotencyRecord>(
            &DataKey::Record(actor, operation.as_symbol(&env), key),
        ) {
            Some(r) => r.status == KeyStatus::Pending,
            None => false,
        }
    }

    /// True iff a key exists AND is `Succeeded`.
    pub fn is_succeeded(env: Env, actor: Address, operation: Operation, key: BytesN<32>) -> bool {
        match env.storage().persistent().get::<DataKey, IdempotencyRecord>(
            &DataKey::Record(actor, operation.as_symbol(&env), key),
        ) {
            Some(r) => r.status == KeyStatus::Succeeded,
            None => false,
        }
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
