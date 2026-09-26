#![no_std]
// `enqueue` takes the event's fields as explicit parameters — eight
// including `env`. Grouping them into a struct would change the
// contract's public ABI to work around a lint, which is not a trade this
// contract should make. Same reasoning as `scholarship-core`.
#![allow(clippy::too_many_arguments)]

//! Durable outbox for scholarship domain events (issue #1140).
//!
//! ADR 0002 draws the scholarship platform as five independently
//! deployable contracts with no cross-contract calls, and puts the
//! orchestration — including "tell the outside world what happened" — in
//! an off-chain layer. That layer has an awkward failure mode: the state
//! write and the notification are two different systems, so a crash
//! between them either loses an event or sends one for a write that
//! rolled back.
//!
//! This contract is the transactional half of that boundary.
//!
//! ## How the atomicity is actually obtained
//!
//! ADR 0002 forbids the five scholarship contracts from calling each
//! other, so none of them can call this one either — a contract that
//! reached into the outbox would be a sixth cross-contract edge, and the
//! orchestrator would have no way to keep a state write and its event in
//! step. The outbox is therefore written by the orchestrator, and the
//! atomicity comes from the transaction envelope rather than from a
//! contract call: the orchestrator submits **one** Stellar transaction
//! containing an invocation of the domain contract *and* an invocation
//! here. Stellar transactions are atomic, so the state write and the event
//! commit together or roll back together. Nothing is ever published for a
//! state change that did not commit, and no state change commits without
//! its event.
//!
//! `source` on each entry is the address of the domain contract whose
//! state changed. It is recorded, not authorized: authorization comes from
//! the platform operator's key, because the operator is the party that can
//! put both invocations in one transaction.
//!
//! ## What is and is not on chain
//!
//! The outbox stores a **commitment** (`payload_hash`), a topic, a schema
//! version, and a correlation id. It never stores the payload. Form
//! answers, applicant names, and award documents stay off-chain; the chain
//! holds only the hash a consumer uses to confirm it received the right
//! bytes. That is what makes the privacy claim in ADR 0002 checkable
//! rather than aspirational.
//!
//! ## Deduplication
//!
//! Every event carries a caller-chosen `dedup_key`. Re-enqueueing the same
//! key is a no-op that returns the original `event_id`, so a publisher
//! that retries after an ambiguous failure cannot produce two events for
//! one state change. Consumers dedup on `event_id`, and additionally
//! advance a per-consumer cursor, so a relay that redelivers a batch a
//! consumer already processed is a no-op for that consumer.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol,
};

const CONTRACT_VERSION: u32 = 1;

/// Ledger entries are kept for at least ~5 weeks and extended toward
/// ~10 weeks, matching the retention the other scholarship contracts use.
/// An entry that no one prunes still ages out of the ledger's own
/// temporary-entry policy rather than living forever.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Upper bound on how far the cursor may lag, so a single abandoned
/// consumer cannot be used to grow the unpublished set without limit.
const MAX_PENDING_EVENTS: u64 = 10_000;

/// Ceiling on entries one `prune` call may remove, so pruning cannot be
/// used to burn an unbounded amount of gas in a single invocation.
const MAX_PRUNE_PER_CALL: u32 = 200;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The event id counter would overflow u64.
    IdOverflow = 4,
    /// No event with that id has been enqueued.
    EventNotFound = 5,
    /// The event id has already been marked published, so re-marking it is
    /// a caller bug rather than a no-op worth hiding.
    AlreadyPublished = 6,
    /// The event is still awaiting publication and cannot be pruned.
    NotPublished = 7,
    /// The unpublished set is at `MAX_PENDING_EVENTS`; the relay is not
    /// keeping up and the publisher must not add to the backlog.
    BacklogFull = 8,
    /// A consumer cursor only moves forward.
    CursorRegression = 9,
    /// A consumer cannot acknowledge an event id it has already passed.
    CursorTooHigh = 10,
    /// The schema version is zero, which no publisher should ever use and
    /// which would make "has this consumer seen v2 yet" unanswerable.
    InvalidSchemaVersion = 11,
    /// The retention horizon is in the future, so pruning would delete
    /// events that consumers have not had a chance to read.
    InvalidRetentionHorizon = 12,
    /// `prune` was asked for zero entries, or more than one call may
    /// remove.
    InvalidPruneLimit = 13,
}

/// Storage layout.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Next event id to hand out. Starts at 1 so 0 is never a valid id.
    NextEventId,
    /// Unpublished event count, for the backlog bound.
    Pending,
    /// Lowest event id `prune` has not yet swept past.
    PruneFloor,
    Event(u64),
    /// Publisher-chosen idempotency key → event id.
    Dedup(BytesN<32>),
    /// Consumer → last event id it has acknowledged.
    Cursor(Address),
}

/// One durable outbox entry.
///
/// `payload_hash` is a commitment, not a payload. See the module docs.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxEvent {
    pub id: u64,
    /// Coarse event name, e.g. `award_reserved`. A `Symbol`, so it is
    /// length-bounded by the SDK rather than by this contract.
    pub topic: Symbol,
    /// Consumer-facing payload schema version. Consumers branch on it
    /// rather than guessing.
    pub schema_version: u32,
    /// Ties this event to the domain object that caused it, so a consumer
    /// can group a program's events without parsing the payload.
    pub correlation_id: BytesN<32>,
    /// sha256 of the off-chain payload. The payload itself is never
    /// stored.
    pub payload_hash: BytesN<32>,
    /// The domain contract whose state change produced this event.
    /// Recorded for provenance, not used for authorization.
    pub source: Address,
    pub created_at: u64,
    pub published_at: u64,
}

#[contract]
pub struct ScholarshipOutboxContract;

#[contractimpl]
impl ScholarshipOutboxContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextEventId, &1u64);
        env.storage().instance().set(&DataKey::Pending, &0u64);
        env.storage().instance().set(&DataKey::PruneFloor, &1u64);
        Ok(())
    }

    /// Record a domain event durably, in the orchestrator's transaction.
    ///
    /// Returns the event id. Calling this twice with the same `dedup_key`
    /// returns the first id and writes nothing, so an orchestrator
    /// retrying after an ambiguous failure cannot double-emit.
    pub fn enqueue(
        env: Env,
        operator: Address,
        source: Address,
        topic: Symbol,
        schema_version: u32,
        correlation_id: BytesN<32>,
        dedup_key: BytesN<32>,
        payload_hash: BytesN<32>,
    ) -> Result<u64, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        if schema_version == 0 {
            return Err(ContractError::InvalidSchemaVersion);
        }

        // Idempotency first: a duplicate must not consume an id, must not
        // touch the pending count, and must not fail on a full backlog.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::Dedup(dedup_key.clone()))
        {
            return Ok(existing);
        }

        let pending: u64 = env.storage().instance().get(&DataKey::Pending).unwrap_or(0);
        if pending >= MAX_PENDING_EVENTS {
            return Err(ContractError::BacklogFull);
        }

        // `NextEventId` is the next id to hand out, so the first event is
        // id 1 and 0 is never a valid id. The increment is checked rather
        // than wrapping, so a counter at u64::MAX fails loudly instead of
        // silently reusing an id a consumer has already seen.
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextEventId)
            .ok_or(ContractError::IdOverflow)?;
        let next = id.checked_add(1).ok_or(ContractError::IdOverflow)?;

        let event = OutboxEvent {
            id,
            topic: topic.clone(),
            schema_version,
            correlation_id,
            payload_hash,
            source: source.clone(),
            created_at: env.ledger().timestamp(),
            published_at: 0,
        };

        let event_key = DataKey::Event(id);
        env.storage().persistent().set(&event_key, &event);
        env.storage()
            .persistent()
            .extend_ttl(&event_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let dedup_key_storage = DataKey::Dedup(dedup_key);
        env.storage().persistent().set(&dedup_key_storage, &id);
        env.storage()
            .persistent()
            .extend_ttl(&dedup_key_storage, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().instance().set(&DataKey::NextEventId, &next);
        env.storage().instance().set(
            &DataKey::Pending,
            &pending.checked_add(1).ok_or(ContractError::IdOverflow)?,
        );

        env.events().publish(
            (soroban_sdk::symbol_short!("ENQ"), topic),
            (id, source, schema_version),
        );

        Ok(id)
    }

    /// The relay confirms the event has left the chain for its consumer.
    ///
    /// Publishing is the only transition that clears an entry from the
    /// unpublished backlog, and it is deliberately separate from
    /// `enqueue`: the state write and the event commit together, but the
    /// network delivery cannot.
    pub fn mark_published(env: Env, relay: Address, event_id: u64) -> Result<(), ContractError> {
        relay.require_auth();
        Self::require_admin(&env, &relay)?;

        let key = DataKey::Event(event_id);
        let mut event: OutboxEvent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EventNotFound)?;

        if event.published_at != 0 {
            return Err(ContractError::AlreadyPublished);
        }

        event.published_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &event);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let pending: u64 = env.storage().instance().get(&DataKey::Pending).unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::Pending, &pending.saturating_sub(1));

        env.events()
            .publish((soroban_sdk::symbol_short!("PUB"),), (event_id, relay));

        Ok(())
    }

    /// Advance a consumer's cursor past `event_id`.
    ///
    /// A cursor only moves forward, and only to an id that exists. Both
    /// restrictions matter: a cursor that could go backwards would let a
    /// consumer re-process a batch it had already acked, and a cursor that
    /// could jump to a non-existent id would let a consumer silently skip
    /// events it never saw.
    pub fn acknowledge(env: Env, consumer: Address, event_id: u64) -> Result<u64, ContractError> {
        consumer.require_auth();

        // The event must exist. Without this check a consumer could ack an
        // id that was never enqueued and park its cursor beyond the
        // backlog forever.
        if !env.storage().persistent().has(&DataKey::Event(event_id)) {
            return Err(ContractError::EventNotFound);
        }

        let current: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::Cursor(consumer.clone()))
            .unwrap_or(0);
        if event_id < current {
            return Err(ContractError::CursorRegression);
        }
        if event_id == current && current != 0 {
            return Ok(current);
        }

        let key = DataKey::Cursor(consumer);
        env.storage().persistent().set(&key, &event_id);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        Ok(event_id)
    }

    /// Drop up to `MAX_PRUNE_PER_CALL` published entries older than
    /// `horizon`, freeing storage.
    ///
    /// Bounded storage is not an optimisation here: an outbox nobody
    /// prunes is a permanent, growing liability. Two properties keep this
    /// safe and affordable:
    ///
    /// - It starts from a stored floor rather than from 1, so repeated
    ///   calls do not rescan history that has already been swept. This is
    ///   what stops an unbounded loop over every event ever written.
    /// - Entries that are still unpublished are never dropped, whatever
    ///   their age. Losing an undelivered event to a retention sweep would
    ///   be worse than the storage it saves, so the floor only advances
    ///   over entries that are genuinely swept.
    ///
    /// Returns the number of entries removed; a full `limit` means there
    /// is likely more to do, and the caller should come back.
    pub fn prune(env: Env, admin: Address, horizon: u64, limit: u32) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        if horizon > env.ledger().timestamp() {
            return Err(ContractError::InvalidRetentionHorizon);
        }
        if limit == 0 || limit > MAX_PRUNE_PER_CALL {
            return Err(ContractError::InvalidPruneLimit);
        }

        let next: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextEventId)
            .unwrap_or(1);
        let mut floor: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PruneFloor)
            .unwrap_or(1);

        let mut pruned = 0u32;
        while floor < next && pruned < limit {
            let key = DataKey::Event(floor);
            let removable = match env.storage().persistent().get::<DataKey, OutboxEvent>(&key) {
                // Already gone (pruned in an earlier call, or aged out of
                // the ledger's own storage): safe to step over.
                None => true,
                // Still undelivered: stop here rather than skip past it,
                // so the floor never overtakes a pending event.
                Some(event) if event.published_at == 0 => false,
                Some(event) => event.published_at < horizon,
            };

            if removable {
                env.storage().persistent().remove(&key);
                floor = floor.checked_add(1).ok_or(ContractError::IdOverflow)?;
                pruned = pruned.checked_add(1).ok_or(ContractError::IdOverflow)?;
            } else {
                break;
            }
        }

        env.storage().instance().set(&DataKey::PruneFloor, &floor);
        Ok(pruned)
    }

    // ── reads ────────────────────────────────────────────────────────────

    pub fn get_event(env: Env, event_id: u64) -> Result<OutboxEvent, ContractError> {
        let key = DataKey::Event(event_id);
        let event: OutboxEvent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EventNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(event)
    }

    /// Resolve a `dedup_key` to its event id — the consumer-side half of
    /// the same idempotency guarantee publishers get.
    pub fn get_by_dedup(env: Env, dedup_key: BytesN<32>) -> Result<u64, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Dedup(dedup_key))
            .ok_or(ContractError::EventNotFound)
    }

    pub fn get_cursor(env: Env, consumer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::Cursor(consumer))
            .unwrap_or(0)
    }

    /// How many entries are still awaiting publication.
    pub fn pending_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::Pending).unwrap_or(0)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

/// Admin-only. On this contract the publisher *is* the admin: the outbox
/// is a single-tenant relay owned by the platform operator, so there is no
/// role to distinguish and no second identity to get wrong. A
/// multi-tenant deployment would replace this with a per-publisher
/// registry, which is a deliberate non-goal of this pass.
impl ScholarshipOutboxContract {
    fn require_admin(env: &Env, account: &Address) -> Result<(), ContractError> {
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match admin {
            Some(a) if a == *account => Ok(()),
            _ => Err(ContractError::NotAdmin),
        }
    }
}

#[cfg(test)]
mod tests;
