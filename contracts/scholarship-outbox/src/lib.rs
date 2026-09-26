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

mod webhook_signing;

pub use webhook_signing::{WebhookEnvelope, ENVELOPE_VERSION, PREIMAGE_LEN};

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
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

// ── Webhook bounds (issue #1141) ───────────────────────────────────────────

/// Cap on enrolled sponsor endpoints. Operator-gated, but still capped so
/// instance storage has a ceiling a single bad keypress cannot pass.
const MAX_ENDPOINTS: u64 = 100;

/// Cap on topics one endpoint may subscribe to, so a subscription filter
/// cannot be used to write an arbitrarily large list into instance storage.
const MAX_ENDPOINT_TOPICS: u32 = 16;

/// Delivery records retained per endpoint. Bounded, so "delivery history
/// is visible" cannot become unbounded state growth. Older attempts are
/// evicted; the per-event attempt counter survives so retry counts stay
/// accurate after eviction.
const MAX_DELIVERY_HISTORY: u32 = 256;

/// Longest window a signed delivery may claim to be valid for. Bounds how
/// long a captured signature stays useful if the sponsor never sees it.
const MAX_SIGNATURE_TTL: u64 = 86_400;

// ── Settlement bounds (issue #1142) ────────────────────────────────────────

/// Ceiling on simultaneously tracked settlement intents. Bounded so a
/// backend that never archives cannot grow instance storage without limit;
/// `archive_settlement` is the release valve, and it only accepts
/// already-settled intents so history cannot be dropped to make room for a
/// live dispute.
const MAX_TRACKED_INTENTS: u64 = 5_000;

/// Most ledger closes an intent may wait to become final. Bounded so a
/// caller cannot pass a threshold that makes an intent permanently
/// unreconcilable -- the failure mode of an unbounded finality window is
/// silence, which is worse than a late answer.
const MAX_FINALITY: u32 = 100;

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
    /// A zero hash was offered where a commitment is required, so "no
    /// secret configured" and "secret of all zeroes" cannot be confused.
    EmptyCommitment = 14,
    /// The endpoint subscribes to no topics, so it could never be sent
    /// anything.
    NoTopics = 15,
    /// The subscription list is longer than one call may write.
    TooManyTopics = 16,
    /// The endpoint id does not exist.
    EndpointNotFound = 17,
    /// Delivery is not strictly the next sequence for this endpoint.
    SequenceRegression = 18,
    /// The endpoint is enrolled at the cap.
    TooManyEndpoints = 19,
    /// The claimed expiry is not in the future, or is further out than
    /// `MAX_SIGNATURE_TTL`.
    InvalidExpiry = 20,
    /// The endpoint is deactivated, so it must not receive deliveries.
    EndpointInactive = 21,
    /// The event's topic is not in the endpoint's subscription, so
    /// delivering it would leak an event the sponsor did not subscribe to.
    NotSubscribed = 22,
    /// The delivery's signed window has closed, so it cannot be recorded.
    ExpiredDelivery = 23,
    /// No delivery record with that sequence.
    DeliveryNotFound = 24,
    /// The sponsor re-acknowledged a sequence it has already passed, which
    /// is what a replayed delivery looks like from the sponsor's side.
    ReplayDetected = 25,
    /// The caller is not the endpoint's owner. Endpoints belong to the
    /// sponsor, not to the platform.
    NotEndpointOwner = 26,
    // ── Settlement reconciliation (issue #1142) ──
    /// No settlement intent with that id.
    IntentNotFound = 27,
    /// That intent id is already tracked. Ids are content-derived, so a
    /// repeat means a producer bug, not an update.
    IntentExists = 28,
    /// The tracked-intent table is at its ceiling.
    TooManyIntents = 29,
    /// A finality threshold of zero, or above `MAX_FINALITY`.
    InvalidFinality = 30,
    /// An amount that is not positive. A settlement of zero or less is not
    /// a settlement, and allowing it would let a mismatch read as a match.
    InvalidAmount = 31,
    /// The observation arrived before the required ledger closes have
    /// passed. Reconciling early is the failure this whole contract exists
    /// to prevent.
    NotFinal = 32,
    /// A transaction has already been observed for this intent. Swapping
    /// the observed transaction is how a mismatch would be made to
    /// disappear.
    AlreadyObserved = 33,
    /// The intent is not in a state that allows this transition.
    InvalidState = 34,
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
    // ── Webhook state (issue #1141) ──
    /// Next endpoint id to hand out. Starts at 1 so 0 is never valid.
    NextEndpointId,
    EnrolledEndpoints,
    Endpoint(u64),
    /// `(endpoint, sequence)` → one delivery attempt.
    Delivery(u64, u64),
    /// Lowest sequence still retained for an endpoint, so eviction can
    /// resume instead of rescanning.
    DeliveryFloor(u64),
    /// How many delivery records an endpoint currently retains.
    DeliveryCount(u64),
    /// `(endpoint, event)` → how many times this event has been attempted.
    Attempts(u64, u64),
    /// Highest delivery sequence the sponsor itself has acknowledged.
    EndpointAck(u64),
    // ── Settlement state (issue #1142) ──
    TrackedIntents,
    Intent(BytesN<32>),
    /// Running count of intents that reached `Mismatched` or `Expired`, so
    /// a monitor can alert on the number without replaying every intent.
    AlertedIntents,
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

/// An approved sponsor endpoint (issue #1141).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookEndpoint {
    pub id: u64,
    /// The sponsor. Only this account may acknowledge deliveries.
    pub owner: Address,
    /// `sha256` of the endpoint URL. The URL itself is never stored: it is
    /// often a bearer capability, and it is off-chain data.
    pub url_hash: BytesN<32>,
    /// `sha256` of the current signing secret. The secret is never stored
    /// either — the relay holds it, the chain holds only its commitment.
    pub secret_hash: BytesN<32>,
    /// Subscribed topics. A delivery outside this list is refused, so the
    /// chain enforces "selected events" rather than trusting the relay to
    /// have filtered.
    pub topics: Vec<Symbol>,
    pub active: bool,
    /// Bumped by `rotate_secret`. Part of the signed preimage, so a
    /// rotation invalidates signatures captured under the previous secret
    /// without needing to revoke anything.
    pub secret_epoch: u32,
    /// Highest delivery sequence handed out for this endpoint.
    pub last_sequence: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

/// What became of one delivery attempt.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    /// The endpoint accepted it.
    Delivered,
    /// The attempt failed and is expected to be retried.
    Failed,
    /// Deliberately not sent — the event was outside the subscription.
    Skipped,
}

/// One delivery attempt, retained as visible history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryRecord {
    pub endpoint_id: u64,
    pub event_id: u64,
    pub sequence: u64,
    /// The secret generation that signed it, so a rotated endpoint's history
    /// stays interpretable.
    pub secret_epoch: u32,
    /// The expiry that was signed. A sponsor can tell a still-valid
    /// signature from an expired one without trusting the relay.
    pub expires_at: u64,
    /// Which attempt this was for this event, counting from 1. Makes
    /// retries visible rather than indistinguishable repeats.
    pub attempt: u32,
    /// Ledger time the attempt was recorded, not the time it happened.
    pub recorded_at: u64,
    pub outcome: DeliveryOutcome,
    /// Ledger time the sponsor acknowledged it, or 0 if it has not.
    pub acked_at: u64,
}

/// Where a settlement intent stands relative to what the chain says
/// actually happened (issue #1142).
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlementState {
    /// Opened, and either unobserved or observed but not yet final.
    Pending,
    /// The chain's authoritative amount matched the intent. Terminal-good.
    Reconciled,
    /// The chain disagrees with the intent. Requires operator resolution --
    /// it is never cleared by a later re-run, because a mismatch is a fact
    /// about a specific observation.
    Mismatched,
    /// The observation never arrived inside the finality window.
    Expired,
}

impl SettlementState {
    /// Whether this state should page somebody.
    ///
    /// Both alarming states are "the backend and the chain disagree" or
    /// "nothing happened at all", and neither resolves on its own.
    pub fn is_alerting(self) -> bool {
        matches!(self, SettlementState::Mismatched | SettlementState::Expired)
    }
}

/// A backend payment or award intent, tracked against the chain's own
/// record of it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementIntent {
    /// Producer-chosen id, normally a hash of the intent. Content-derived
    /// so a retry cannot create a second intent for one payment.
    pub intent_id: BytesN<32>,
    /// The domain contract the intent concerns. Recorded for provenance.
    pub source: Address,
    /// Ties the intent to the outbox event that announced it, so a
    /// settlement can be traced back to the state change that caused it.
    pub correlation_id: BytesN<32>,
    /// What the backend believes it is settling.
    pub expected_amount: i128,
    pub state: SettlementState,
    pub opened_at: u64,
    /// Ledger closes that must pass after the observed transaction's ledger
    /// before its outcome is treated as authoritative. Stored per intent
    /// because the right answer depends on the operation's risk, and
    /// hardcoding one number would be wrong for some of them.
    pub required_finality: u32,
    /// Zero until a transaction is observed.
    pub observed_tx_hash: BytesN<32>,
    /// Zero until a transaction is observed.
    pub observed_ledger: u32,
    /// What the chain actually settled, filled in at reconciliation.
    pub observed_amount: i128,
    /// How many times this intent was actually adjudicated. A repeat call
    /// on an already-decided intent is a complete no-op, so this counts
    /// adjudications rather than calls: a backend retrying while an intent
    /// is still pending shows up here, and post-settlement retries do not
    /// pollute the number.
    pub attempts: u32,
    /// Ledger time of the reconciliation that settled the state, or 0.
    pub reconciled_at: u64,
    /// `sha256` of the operator's note when a mismatch was resolved.
    pub resolution_hash: BytesN<32>,
}

/// The single shape a frontend reads (issue #1142: "frontend status uses
/// reconciled state").
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementView {
    pub intent_id: BytesN<32>,
    pub state: SettlementState,
    /// Present only when `state` is `Reconciled`. Zero otherwise, so an
    /// un-reconciled intent cannot be rendered as a settled amount by
    /// accident.
    pub settled_amount: i128,
    pub observed_amount: i128,
    pub reconciled_at: u64,
    pub attempts: u32,
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
        // Webhook counters (issue #1141). Set here rather than defaulted at
        // each use site, so a half-initialized contract is not a state the
        // rest of the code has to defend against.
        env.storage()
            .instance()
            .set(&DataKey::NextEndpointId, &1u64);
        env.storage()
            .instance()
            .set(&DataKey::EnrolledEndpoints, &0u64);
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
    // ═══════════════════════════════════════════════════════════════════════════
    // Signed sponsor webhooks (issue #1141)
    // ═══════════════════════════════════════════════════════════════════════════
    //
    // The trust boundary here is worth stating plainly, because it is the whole
    // design: **the chain never sends an HTTP request and never verifies an
    // off-chain signature.** It cannot — Soroban has no sockets, and a
    // signature is produced by a key the chain does not hold. So the on-chain
    // half is deliberately limited to the things a contract *can* enforce, and
    // the sponsor's verifier does the rest:
    //
    // | Property                          | Enforced where                    |
    // |-----------------------------------|-----------------------------------|
    // | exact authorization               | contract — see `register_endpoint`|
    // | "selected events only"            | contract — `record_delivery`      |
    // | secret rotation                   | contract — `rotate_secret`        |
    // | timestamped, ordered, gap-free     | contract — `record_delivery`      |
    // | replay resistance, on-chain       | contract — sequence + expiry      |
    // | replay resistance, at the sponsor | signature preimage + the sponsor's |
    // |                                   | own `EndpointAck`                 |
    // | the signature is authentic        | **off-chain** — the sponsor's HTTP|
    // |                                   | handler                           |
    // | delivery history is visible       | contract — `get_delivery`         |
    //
    // A sponsor MUST still verify the HMAC in its handler and MUST track the
    // highest sequence and `expires_at` it has accepted. This contract gives it
    // the bytes to verify against and the means to notice a replay; it cannot
    // make an HTTP endpoint verify anything for it. Documented that way in
    // `contracts/docs/scholarship-outbox.md` rather than implied by silence.

    /// Enroll a sponsor endpoint.
    ///
    /// Requires the operator's auth **and** the owner's. Bilateral consent
    /// is the point: neither the platform alone nor a sponsor alone can
    /// enroll an endpoint, so a compromised platform key cannot silently
    /// start shipping one sponsor's award events to an attacker's URL, and a
    /// sponsor cannot be enrolled without its key.
    ///
    /// Takes commitments, not secrets. The relay and the sponsor hold the
    /// URL and the signing secret; the chain holds their hashes.
    pub fn register_endpoint(
        env: Env,
        operator: Address,
        owner: Address,
        url_hash: BytesN<32>,
        secret_hash: BytesN<32>,
        topics: Vec<Symbol>,
    ) -> Result<u64, ContractError> {
        operator.require_auth();
        owner.require_auth();
        Self::require_admin(&env, &operator)?;
        Self::require_commitment(&env, &url_hash)?;
        Self::require_commitment(&env, &secret_hash)?;

        if topics.is_empty() {
            return Err(ContractError::NoTopics);
        }
        if topics.len() > MAX_ENDPOINT_TOPICS {
            return Err(ContractError::TooManyTopics);
        }

        let enrolled: u64 = env
            .storage()
            .instance()
            .get(&DataKey::EnrolledEndpoints)
            .unwrap_or(0);
        if enrolled >= MAX_ENDPOINTS {
            return Err(ContractError::TooManyEndpoints);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextEndpointId)
            .ok_or(ContractError::NotInitialized)?;
        let now = env.ledger().timestamp();

        let endpoint = WebhookEndpoint {
            id,
            owner: owner.clone(),
            url_hash,
            secret_hash,
            topics,
            active: true,
            // Epoch starts at 1 so zero is never a valid generation.
            secret_epoch: 1,
            last_sequence: 0,
            created_at: now,
            updated_at: now,
        };
        Self::save_endpoint(&env, &endpoint);

        env.storage().instance().set(
            &DataKey::NextEndpointId,
            &id.checked_add(1).ok_or(ContractError::IdOverflow)?,
        );
        env.storage().instance().set(
            &DataKey::EnrolledEndpoints,
            &enrolled.checked_add(1).ok_or(ContractError::IdOverflow)?,
        );

        env.events().publish(
            (soroban_sdk::symbol_short!("epadd"), id),
            (owner, now, endpoint.topics.clone()),
        );
        Ok(id)
    }

    /// Replace an endpoint's signing secret.
    ///
    /// Bumps `secret_epoch`, which is part of the signed preimage. A
    /// signature captured under the old secret therefore stops verifying
    /// the moment this is called, with no revocation list and nothing to
    /// remember — the sponsor recomputes the preimage it expects from the
    /// endpoint it just read on-chain, and gets a different digest. This is
    /// the property that makes rotation usable in an emergency: leaking a
    /// secret is a one-transaction fix.
    ///
    /// Works on an active endpoint, because rotating a live secret is the
    /// normal case, not an edge case.
    pub fn rotate_secret(
        env: Env,
        operator: Address,
        endpoint_id: u64,
        new_secret_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;
        Self::require_commitment(&env, &new_secret_hash)?;

        let mut endpoint = Self::load_endpoint(&env, endpoint_id)?;
        let epoch = endpoint
            .secret_epoch
            .checked_add(1)
            .ok_or(ContractError::IdOverflow)?;
        endpoint.secret_hash = new_secret_hash;
        endpoint.secret_epoch = epoch;
        endpoint.updated_at = env.ledger().timestamp();
        Self::save_endpoint(&env, &endpoint);

        env.events()
            .publish((soroban_sdk::symbol_short!("secrot"), endpoint_id), epoch);
        Ok(epoch)
    }

    /// Activate or deactivate an endpoint. Deactivation stops future
    /// deliveries but keeps the delivery history, so a sponsor that is
    /// offline for a week can be muted and still see what it missed.
    pub fn set_endpoint_active(
        env: Env,
        operator: Address,
        endpoint_id: u64,
        active: bool,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut endpoint = Self::load_endpoint(&env, endpoint_id)?;
        endpoint.active = active;
        endpoint.updated_at = env.ledger().timestamp();
        Self::save_endpoint(&env, &endpoint);

        env.events()
            .publish((soroban_sdk::symbol_short!("epswap"), endpoint_id), active);
        Ok(())
    }

    /// The digest a relay signs and a sponsor verifies.
    ///
    /// Read-only and unauthenticated on purpose: a sponsor verifying an
    /// incoming delivery should be able to recompute the expected digest
    /// from the chain alone, without holding the secret. That is what makes
    /// the secret prove *possession* rather than being the only source of
    /// truth — the sponsor checks the HMAC over these exact bytes, and can
    /// independently confirm the bytes are the ones this endpoint, this
    /// event, and this secret generation should have produced.
    ///
    /// Refuses to mint a digest for a sequence that is not the next one, or
    /// for an expiry outside `(now, now + MAX_SIGNATURE_TTL]`. Without
    /// those checks the operator could pre-sign arbitrary sequences and
    /// long-lived signatures, which would undercut the replay guarantees
    /// the ordering exists to provide.
    ///
    /// Note the two values this does *not* take from the caller: the
    /// network id and the contract address both come from the environment,
    /// so a caller cannot ask for a digest that is valid on another network
    /// or against another deployment.
    pub fn delivery_payload(
        env: Env,
        endpoint_id: u64,
        event_id: u64,
        sequence: u64,
        expires_at: u64,
    ) -> Result<BytesN<32>, ContractError> {
        let endpoint = Self::load_endpoint(&env, endpoint_id)?;
        if !endpoint.active {
            return Err(ContractError::EndpointInactive);
        }
        if sequence != Self::next_sequence(&endpoint)? {
            return Err(ContractError::SequenceRegression);
        }
        Self::check_expiry(env.ledger().timestamp(), expires_at)?;

        let event: OutboxEvent = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(ContractError::EventNotFound)?;

        let envelope = WebhookEnvelope {
            version: ENVELOPE_VERSION,
            // From the ledger, not from a parameter. Accepting a
            // caller-supplied network id would put the cross-network
            // replay defence in the caller's hands; this way a signature
            // for testnet cannot be minted at all on mainnet.
            network_id: env.ledger().network_id(),
            contract: env.current_contract_address(),
            endpoint_id,
            secret_epoch: endpoint.secret_epoch,
            event_id,
            sequence,
            expires_at,
        };
        Ok(webhook_signing::signing_payload(
            &env,
            &envelope,
            &event.payload_hash,
        ))
    }

    /// Whether this endpoint should receive this event at all.
    ///
    /// A relay that ignores this leaks events the sponsor did not subscribe
    /// to, and nothing downstream would catch it — so the check is on chain
    /// and `record_delivery` repeats it rather than trusting the caller.
    pub fn deliverable(env: Env, endpoint_id: u64, event_id: u64) -> Result<bool, ContractError> {
        let endpoint = Self::load_endpoint(&env, endpoint_id)?;
        if !endpoint.active {
            return Ok(false);
        }
        let event: OutboxEvent = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(ContractError::EventNotFound)?;
        Ok(endpoint.topics.contains(&event.topic))
    }

    /// Record one delivery attempt, and make it visible.
    ///
    /// Timestamped from the ledger, strictly ordered and gap-free per
    /// endpoint, and refused once the signed window has closed. A relay
    /// that retries an event gets a **new** sequence for the same event, so
    /// a retry is visible as a retry — `attempt` counts it — rather than
    /// being indistinguishable from a duplicate.
    ///
    /// A closed window is refused rather than recorded because a signature
    /// that has expired is exactly what a replay looks like; recording it
    /// would launder an attack into ordinary history.
    pub fn record_delivery(
        env: Env,
        operator: Address,
        endpoint_id: u64,
        event_id: u64,
        sequence: u64,
        expires_at: u64,
        outcome: DeliveryOutcome,
    ) -> Result<u32, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut endpoint = Self::load_endpoint(&env, endpoint_id)?;
        if !endpoint.active {
            return Err(ContractError::EndpointInactive);
        }
        if sequence != Self::next_sequence(&endpoint)? {
            return Err(ContractError::SequenceRegression);
        }

        let now = env.ledger().timestamp();
        if expires_at <= now {
            return Err(ContractError::ExpiredDelivery);
        }

        let event: OutboxEvent = env
            .storage()
            .persistent()
            .get(&DataKey::Event(event_id))
            .ok_or(ContractError::EventNotFound)?;
        // A `Skipped` outcome is the relay recording that it deliberately
        // did not send an event outside this endpoint's subscription, so it
        // is the one outcome allowed to reference an unsubscribed event --
        // otherwise the decision would be invisible and the sponsor could
        // not tell "nothing happened" from "you were filtered out".
        // Anything claiming to have been sent still has to be subscribed.
        if outcome != DeliveryOutcome::Skipped && !endpoint.topics.contains(&event.topic) {
            return Err(ContractError::NotSubscribed);
        }

        let attempt_key = DataKey::Attempts(endpoint_id, event_id);
        let attempt: u32 = env
            .storage()
            .persistent()
            .get(&attempt_key)
            .unwrap_or(0u32)
            .checked_add(1)
            .ok_or(ContractError::IdOverflow)?;
        env.storage().persistent().set(&attempt_key, &attempt);

        let record = DeliveryRecord {
            endpoint_id,
            event_id,
            sequence,
            secret_epoch: endpoint.secret_epoch,
            expires_at,
            attempt,
            recorded_at: now,
            outcome,
            acked_at: 0,
        };
        let record_key = DataKey::Delivery(endpoint_id, sequence);
        env.storage().persistent().set(&record_key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&record_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Evict the oldest record once the window is full. The attempt
        // counter is a separate key and survives eviction, so retry counts
        // stay right after the detail is gone.
        let count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::DeliveryCount(endpoint_id))
            .unwrap_or(0u32);
        if count >= MAX_DELIVERY_HISTORY {
            let floor: u64 = env
                .storage()
                .persistent()
                .get(&DataKey::DeliveryFloor(endpoint_id))
                .unwrap_or(1);
            let stale = DataKey::Delivery(endpoint_id, floor);
            env.storage().persistent().remove(&stale);
            env.storage().persistent().set(
                &DataKey::DeliveryFloor(endpoint_id),
                &floor.checked_add(1).ok_or(ContractError::IdOverflow)?,
            );
        } else {
            if count == 0 {
                env.storage()
                    .persistent()
                    .set(&DataKey::DeliveryFloor(endpoint_id), &sequence);
            }
            env.storage().persistent().set(
                &DataKey::DeliveryCount(endpoint_id),
                &count.checked_add(1).ok_or(ContractError::IdOverflow)?,
            );
        }

        endpoint.last_sequence = sequence;
        endpoint.updated_at = now;
        Self::save_endpoint(&env, &endpoint);

        env.events().publish(
            (soroban_sdk::symbol_short!("dlv"), endpoint_id, sequence),
            (event_id, attempt, outcome, now),
        );
        Ok(attempt)
    }

    /// The sponsor confirms it received a delivery.
    ///
    /// The relay's `record_delivery` records that it *tried*; only the
    /// sponsor's own signature records that something *arrived*. That
    /// distinction is the reason this method exists — the relay is not
    /// trusted to report its own success.
    ///
    /// Strictly increasing, so a replayed delivery presented a second time
    /// is refused. Requires the owner's auth and checks it, so no endpoint
    /// can be acknowledged on another sponsor's behalf.
    pub fn acknowledge_delivery(
        env: Env,
        owner: Address,
        endpoint_id: u64,
        sequence: u64,
    ) -> Result<(), ContractError> {
        owner.require_auth();

        let endpoint = Self::load_endpoint(&env, endpoint_id)?;
        if endpoint.owner != owner {
            return Err(ContractError::NotEndpointOwner);
        }
        if sequence == 0 || sequence > endpoint.last_sequence {
            return Err(ContractError::DeliveryNotFound);
        }

        let acked: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::EndpointAck(endpoint_id))
            .unwrap_or(0);
        if sequence <= acked {
            return Err(ContractError::ReplayDetected);
        }

        let record_key = DataKey::Delivery(endpoint_id, sequence);
        let mut record: DeliveryRecord = env
            .storage()
            .persistent()
            .get(&record_key)
            .ok_or(ContractError::DeliveryNotFound)?;

        let now = env.ledger().timestamp();
        record.acked_at = now;
        env.storage().persistent().set(&record_key, &record);
        env.storage()
            .persistent()
            .set(&DataKey::EndpointAck(endpoint_id), &sequence);

        env.events().publish(
            (soroban_sdk::symbol_short!("dack"), endpoint_id, sequence),
            now,
        );
        Ok(())
    }

    // ── Settlement reconciliation (#1142) ─────────────────────────────────
    //
    // The backend has an opinion (a payment or award intent); the chain has
    // a fact (a transaction that settled, in a ledger, with an amount).
    // This reconciles the two and refuses to let the opinion win.
    //
    // Four rules, in the order they matter:
    //
    // 1. **Finality is explicit and per-intent.** A Stellar transaction
    //    reported as successful is not final: the ledger containing it can
    //    still be rolled back, and a fee-bump or a later close can change
    //    the outcome. An intent is only reconcilable once
    //    `current_ledger - observed_ledger >= required_finality`. Zero is
    //    refused -- "final immediately" is exactly the bug this contract
    //    exists to catch.
    // 2. **Reprocessing is idempotent.** Reconciling a settled intent
    //    returns the same state and changes nothing, so a backend retry
    //    loop or a duplicate webhook cannot flip a verdict.
    // 3. **A mismatch is a fact, not a phase.** `Mismatched` is never
    //    cleared by re-running: the observed transaction is immutable, so
    //    the disagreement is too. Only an explicit `resolve_mismatch`
    //    moves it, which is what makes the alert trustworthy.
    // 4. **The UI cannot render a pending intent as settled.**
    //    `settlement_status` zeroes `settled_amount` unless the state is
    //    `Reconciled`.

    /// Open a settlement intent. The backend's claim, not yet compared to
    /// anything.
    pub fn open_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
        source: Address,
        correlation_id: BytesN<32>,
        expected_amount: i128,
        required_finality: u32,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        // Zero would make every intent final the instant it was observed,
        // which is the failure mode #1142 asks us to prevent. A ceiling
        // stops a caller parking an intent in `Pending` forever.
        if required_finality == 0 || required_finality > MAX_FINALITY {
            return Err(ContractError::InvalidFinality);
        }
        if expected_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Intent(intent_id.clone()))
        {
            return Err(ContractError::IntentExists);
        }

        let tracked: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TrackedIntents)
            .unwrap_or(0u64);
        if tracked >= MAX_TRACKED_INTENTS {
            return Err(ContractError::TooManyIntents);
        }

        let intent = SettlementIntent {
            intent_id: intent_id.clone(),
            source,
            correlation_id,
            expected_amount,
            state: SettlementState::Pending,
            opened_at: env.ledger().timestamp(),
            required_finality,
            observed_tx_hash: BytesN::from_array(&env, &[0u8; 32]),
            observed_ledger: 0,
            observed_amount: 0,
            attempts: 0,
            reconciled_at: 0,
            resolution_hash: BytesN::from_array(&env, &[0u8; 32]),
        };
        Self::save_intent(&env, &intent);
        env.storage().instance().set(
            &DataKey::TrackedIntents,
            &tracked.checked_add(1).ok_or(ContractError::IdOverflow)?,
        );

        env.events().publish(
            (soroban_sdk::symbol_short!("opn"), intent_id.clone()),
            (expected_amount, required_finality, env.ledger().timestamp()),
        );
        Ok(())
    }

    /// Report the transaction that was submitted for this intent.
    ///
    /// Idempotent for the same transaction, and **refuses a different one**
    /// once set. That refusal is the load-bearing part: if a second
    /// observation could replace the first, a backend could swap in a
    /// transaction that happens to agree with its intent and make a
    /// mismatch vanish.
    pub fn observe_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
        tx_hash: BytesN<32>,
        tx_ledger: u32,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut intent = Self::load_intent(&env, &intent_id)?;
        if intent.state != SettlementState::Pending {
            return Err(ContractError::InvalidState);
        }
        if intent.observed_ledger != 0 {
            return if intent.observed_tx_hash == tx_hash && intent.observed_ledger == tx_ledger {
                // Same transaction reported twice: nothing to do.
                Ok(())
            } else {
                Err(ContractError::AlreadyObserved)
            };
        }
        if tx_ledger == 0 || tx_ledger > env.ledger().sequence() {
            return Err(ContractError::NotFinal);
        }

        intent.observed_tx_hash = tx_hash;
        intent.observed_ledger = tx_ledger;
        Self::save_intent(&env, &intent);

        env.events()
            .publish((soroban_sdk::symbol_short!("obsv"), intent_id), tx_ledger);
        Ok(())
    }

    /// Compare the intent against what the chain settled.
    ///
    /// Returns the resulting state. Safe to call repeatedly: once an
    /// intent is decided, this returns its state and writes nothing at all,
    /// so a retry loop or a duplicate webhook cannot flip a verdict or
    /// disturb the record of how it was reached.
    pub fn reconcile_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
        observed_amount: i128,
    ) -> Result<SettlementState, ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut intent = Self::load_intent(&env, &intent_id)?;
        let now = env.ledger().timestamp();

        // Terminal-good and terminal-bad states are both sticky, and this
        // early return is what makes reprocessing a complete no-op: not
        // just the state, every field of the record.
        if intent.state != SettlementState::Pending {
            return Ok(intent.state);
        }

        if intent.observed_ledger == 0 {
            return Err(ContractError::NotFinal);
        }
        let passed = env
            .ledger()
            .sequence()
            .checked_sub(intent.observed_ledger)
            .ok_or(ContractError::NotFinal)?;
        if passed < intent.required_finality {
            return Err(ContractError::NotFinal);
        }

        intent.attempts = intent
            .attempts
            .checked_add(1)
            .ok_or(ContractError::IdOverflow)?;
        intent.observed_amount = observed_amount;
        intent.reconciled_at = now;

        if observed_amount == intent.expected_amount {
            intent.state = SettlementState::Reconciled;
        } else {
            intent.state = SettlementState::Mismatched;
            Self::raise_alert(&env)?;
        }
        Self::save_intent(&env, &intent);

        // A dedicated event plus a running count, so alerting does not
        // depend on someone tailing every settlement topic.
        env.events().publish(
            (soroban_sdk::symbol_short!("recon"), intent_id.clone()),
            (
                intent.state,
                intent.expected_amount,
                observed_amount,
                intent.attempts,
            ),
        );
        Ok(intent.state)
    }

    /// Give up on an intent whose transaction never appeared.
    ///
    /// Separate from `Expired`'s alerting because "no transaction" and "the
    /// wrong amount" need different responses: the first is usually a
    /// backend that failed to submit, the second is a real dispute.
    pub fn expire_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let mut intent = Self::load_intent(&env, &intent_id)?;
        if intent.state != SettlementState::Pending {
            return Err(ContractError::InvalidState);
        }
        intent.state = SettlementState::Expired;
        intent.reconciled_at = env.ledger().timestamp();
        Self::save_intent(&env, &intent);
        Self::raise_alert(&env)?;

        env.events().publish(
            (soroban_sdk::symbol_short!("expr"), intent_id),
            intent.reconciled_at,
        );
        Ok(())
    }

    /// Clear a mismatch or expiry with a recorded reason.
    ///
    /// The only way out of an alarming state short of archiving. The
    /// operator's note is committed by hash, so the resolution is
    /// auditable without putting free text on chain.
    pub fn resolve_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
        resolution_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;
        Self::require_commitment(&env, &resolution_hash)?;

        let mut intent = Self::load_intent(&env, &intent_id)?;
        if !intent.state.is_alerting() {
            // Resolving something that is fine, or something still pending,
            // is a caller bug worth surfacing rather than a no-op.
            return Err(ContractError::InvalidState);
        }
        intent.state = SettlementState::Reconciled;
        intent.reconciled_at = env.ledger().timestamp();
        intent.resolution_hash = resolution_hash;
        Self::save_intent(&env, &intent);

        env.events().publish(
            (soroban_sdk::symbol_short!("rsv"), intent_id),
            intent.reconciled_at,
        );
        Ok(())
    }

    /// Drop a settled intent, freeing a slot.
    ///
    /// Refused for `Pending`, `Mismatched`, and `Expired` on purpose: the
    /// release valve must not double as a way to erase an open dispute.
    pub fn archive_settlement(
        env: Env,
        operator: Address,
        intent_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        operator.require_auth();
        Self::require_admin(&env, &operator)?;

        let intent = Self::load_intent(&env, &intent_id)?;
        if intent.state != SettlementState::Reconciled {
            return Err(ContractError::InvalidState);
        }
        env.storage()
            .persistent()
            .remove(&DataKey::Intent(intent_id.clone()));
        let tracked: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TrackedIntents)
            .unwrap_or(0u64);
        env.storage()
            .instance()
            .set(&DataKey::TrackedIntents, &tracked.saturating_sub(1));

        // The removal itself is the record, so an auditor can see that an
        // intent existed and was archived even though it no longer does.
        env.events().publish(
            (soroban_sdk::symbol_short!("arch"), intent_id),
            env.ledger().timestamp(),
        );
        Ok(())
    }

    // ── Settlement views ──────────────────────────────────────────────────

    /// What a frontend should render. `settled_amount` is zero unless the
    /// state is `Reconciled`, so a pending or disputed intent cannot be
    /// displayed as a settled figure by accident.
    pub fn settlement_status(
        env: Env,
        intent_id: BytesN<32>,
    ) -> Result<SettlementView, ContractError> {
        let intent = Self::load_intent(&env, &intent_id)?;
        let settled_amount = if intent.state == SettlementState::Reconciled {
            intent.observed_amount
        } else {
            0
        };
        Ok(SettlementView {
            intent_id: intent.intent_id,
            state: intent.state,
            settled_amount,
            observed_amount: intent.observed_amount,
            reconciled_at: intent.reconciled_at,
            attempts: intent.attempts,
        })
    }

    /// The settled amount, or `NotFinal` if the intent has not been
    /// reconciled. A caller that wants to render a number must handle the
    /// error, which is the point.
    pub fn settled_amount(env: Env, intent_id: BytesN<32>) -> Result<i128, ContractError> {
        let intent = Self::load_intent(&env, &intent_id)?;
        match intent.state {
            SettlementState::Reconciled => Ok(intent.observed_amount),
            _ => Err(ContractError::InvalidState),
        }
    }

    pub fn get_intent(env: Env, intent_id: BytesN<32>) -> Result<SettlementIntent, ContractError> {
        Self::load_intent(&env, &intent_id)
    }

    /// Running count of intents in an alarming state. Meant to be polled by
    /// a monitor; it cannot go down except by resolving, so a rising number
    /// is unambiguous.
    pub fn alert_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::AlertedIntents)
            .unwrap_or(0u64)
    }

    pub fn tracked_intents(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::TrackedIntents)
            .unwrap_or(0u64)
    }

    // ── Views ─────────────────────────────────────────────────────────────

    pub fn get_endpoint(env: Env, endpoint_id: u64) -> Result<WebhookEndpoint, ContractError> {
        Self::load_endpoint(&env, endpoint_id)
    }

    pub fn get_delivery(
        env: Env,
        endpoint_id: u64,
        sequence: u64,
    ) -> Result<DeliveryRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Delivery(endpoint_id, sequence))
            .ok_or(ContractError::DeliveryNotFound)
    }

    /// How many times an event has been attempted for an endpoint. Survives
    /// history eviction.
    pub fn get_attempts(env: Env, endpoint_id: u64, event_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Attempts(endpoint_id, event_id))
            .unwrap_or(0)
    }

    pub fn get_acked_sequence(env: Env, endpoint_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::EndpointAck(endpoint_id))
            .unwrap_or(0)
    }

    pub fn delivery_history_count(env: Env, endpoint_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::DeliveryCount(endpoint_id))
            .unwrap_or(0)
    }

    /// Oldest sequence still retained, so a sponsor paging its history
    /// knows where the window starts instead of guessing.
    pub fn delivery_history_floor(env: Env, endpoint_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::DeliveryFloor(endpoint_id))
            .unwrap_or(0)
    }
}

/// Admin-only. On this contract the publisher *is* the admin: the outbox
/// is a single-tenant relay owned by the platform operator, so there is no
/// role to distinguish and no second identity to get wrong. A
/// multi-tenant deployment would replace this with a per-publisher
/// registry, which is a deliberate non-goal of this pass.
//
// The webhook methods below are a second, narrower authority: an endpoint's
// **owner** may acknowledge its own deliveries and nothing else, while the
// platform operator controls enrollment, secret rotation, and the delivery
// record.
impl ScholarshipOutboxContract {
    fn require_admin(env: &Env, account: &Address) -> Result<(), ContractError> {
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match admin {
            Some(a) if a == *account => Ok(()),
            _ => Err(ContractError::NotAdmin),
        }
    }

    fn load_endpoint(env: &Env, id: u64) -> Result<WebhookEndpoint, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Endpoint(id))
            .ok_or(ContractError::EndpointNotFound)
    }

    fn save_endpoint(env: &Env, endpoint: &WebhookEndpoint) {
        env.storage()
            .persistent()
            .set(&DataKey::Endpoint(endpoint.id), endpoint);
        // Endpoint state is read on every relay cycle and by every sponsor,
        // so it gets the longer retention window.
        env.storage().persistent().extend_ttl(
            &DataKey::Endpoint(endpoint.id),
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );
    }

    /// Refuses a zero commitment. An all-zeroes hash would be
    /// indistinguishable from "nothing configured", and it is not a value
    /// anybody chose deliberately.
    fn require_commitment(env: &Env, commitment: &BytesN<32>) -> Result<(), ContractError> {
        if *commitment == BytesN::from_array(env, &[0u8; 32]) {
            return Err(ContractError::EmptyCommitment);
        }
        Ok(())
    }

    /// The sequence a delivery must use: strictly the next one.
    ///
    /// Gap-free rather than merely increasing, so a sponsor that sees
    /// sequence 9 without 8 knows something was lost instead of silently
    /// continuing with a hole in its history.
    fn next_sequence(endpoint: &WebhookEndpoint) -> Result<u64, ContractError> {
        endpoint
            .last_sequence
            .checked_add(1)
            .ok_or(ContractError::IdOverflow)
    }

    fn load_intent(env: &Env, intent_id: &BytesN<32>) -> Result<SettlementIntent, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Intent(intent_id.clone()))
            .ok_or(ContractError::IntentNotFound)
    }

    fn save_intent(env: &Env, intent: &SettlementIntent) {
        env.storage()
            .persistent()
            .set(&DataKey::Intent(intent.intent_id.clone()), intent);
        env.storage().persistent().extend_ttl(
            &DataKey::Intent(intent.intent_id.clone()),
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );
    }

    fn raise_alert(env: &Env) -> Result<(), ContractError> {
        let alerts: u64 = env
            .storage()
            .instance()
            .get(&DataKey::AlertedIntents)
            .unwrap_or(0u64);
        env.storage().instance().set(
            &DataKey::AlertedIntents,
            &alerts.checked_add(1).ok_or(ContractError::IdOverflow)?,
        );
        Ok(())
    }

    /// Validates a claimed expiry against the ledger clock.
    fn check_expiry(now: u64, expires_at: u64) -> Result<(), ContractError> {
        let ttl = expires_at
            .checked_sub(now)
            .ok_or(ContractError::InvalidExpiry)?;
        if ttl > MAX_SIGNATURE_TTL {
            return Err(ContractError::InvalidExpiry);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod webhook_tests;

#[cfg(test)]
mod settlement_tests;
