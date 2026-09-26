//! Tests for the durable scholarship event outbox (#1140).

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::Bytes;

/// Filler bytes for the two fields a test is not about, so the byte a test
/// names is always the one it means.
const CORRELATION: u8 = 200;
const PAYLOAD: u8 = 201;

struct Harness {
    env: Env,
    contract: Address,
    operator: Address,
    source: Address,
    other_source: Address,
    consumer: Address,
    other_consumer: Address,
}

impl Harness {
    fn new() -> Harness {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);
        let contract = env.register(ScholarshipOutboxContract, ());
        let operator = Address::generate(&env);
        ScholarshipOutboxContractClient::new(&env, &contract).initialize(&operator);
        let source = Address::generate(&env);
        let other_source = Address::generate(&env);
        let consumer = Address::generate(&env);
        let other_consumer = Address::generate(&env);
        Harness {
            env,
            contract,
            operator,
            source,
            other_source,
            consumer,
            other_consumer,
        }
    }

    fn client(&self) -> ScholarshipOutboxContractClient<'_> {
        ScholarshipOutboxContractClient::new(&self.env, &self.contract)
    }

    fn key(&self, byte: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[byte; 32])
    }

    /// Enqueue with default arguments, so each test states only the parts
    /// it is actually about. `dedup` is the caller-chosen idempotency
    /// key, which is the byte a test addresses; the correlation id and
    /// payload hash are fixed filler.
    fn enqueue(&self, dedup: u8) -> u64 {
        self.client().enqueue(
            &self.operator,
            &self.source,
            &Symbol::new(&self.env, "award_reserved"),
            &1,
            &self.key(CORRELATION),
            &self.key(dedup),
            &self.key(PAYLOAD),
        )
    }
}

// ── enqueue and durability ──────────────────────────────────────────────────

#[test]
fn an_event_is_durable_and_readable_by_id() {
    let h = Harness::new();
    let id = h.enqueue(1);

    let event = h.client().get_event(&id);
    assert_eq!(event.id, 1);
    assert_eq!(event.topic, Symbol::new(&h.env, "award_reserved"));
    assert_eq!(event.schema_version, 1);
    assert_eq!(event.correlation_id, h.key(CORRELATION));
    assert_eq!(event.payload_hash, h.key(PAYLOAD));
    assert_eq!(event.source, h.source);
    assert_eq!(event.created_at, 1_000);
    assert_eq!(event.published_at, 0);
    assert_eq!(h.client().pending_count(), 1);
}

#[test]
fn ids_are_monotonic_and_gapless() {
    let h = Harness::new();
    for expected in 1..=5u64 {
        assert_eq!(h.enqueue(expected as u8), expected);
    }
    assert_eq!(h.client().pending_count(), 5);
}

#[test]
fn only_the_operator_may_enqueue() {
    let h = Harness::new();
    assert_eq!(
        h.client().try_enqueue(
            &h.consumer,
            &h.source,
            &Symbol::new(&h.env, "award_reserved"),
            &1,
            &h.key(1),
            &h.key(2),
            &h.key(3),
        ),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(h.client().pending_count(), 0);
}

#[test]
fn a_zero_schema_version_is_refused() {
    let h = Harness::new();
    assert_eq!(
        h.client().try_enqueue(
            &h.operator,
            &h.source,
            &Symbol::new(&h.env, "award_reserved"),
            &0,
            &h.key(1),
            &h.key(2),
            &h.key(3),
        ),
        Err(Ok(ContractError::InvalidSchemaVersion))
    );
    assert_eq!(h.client().pending_count(), 0);
}

#[test]
fn initialize_is_not_repeatable() {
    let h = Harness::new();
    assert_eq!(
        h.client().try_initialize(&h.operator),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

// ── deduplication ───────────────────────────────────────────────────────────

#[test]
fn a_duplicate_dedup_key_returns_the_original_id_and_writes_nothing() {
    let h = Harness::new();
    let first = h.enqueue(1);
    let second = h.enqueue(1);
    assert_eq!(first, second);
    // One entry, not two — and the next id was not consumed.
    assert_eq!(h.client().pending_count(), 1);
    assert_eq!(h.enqueue(2), first + 1);
}

#[test]
fn dedup_is_scoped_to_the_key_not_the_topic() {
    let h = Harness::new();
    let first = h.enqueue(1);
    // Same key, different topic and schema: still the same event. The key
    // is the publisher's idempotency token, so it has to win over
    // everything else or a retry that changed a field would double-emit.
    let second = h.client().enqueue(
        &h.operator,
        &h.other_source,
        &Symbol::new(&h.env, "award_paid"),
        &2,
        &h.key(9),
        &h.key(1),
        &h.key(7),
    );
    // Same key as the first `enqueue(1)`, everything else different.
    assert_eq!(second, first);
    assert_eq!(h.client().pending_count(), 1);
}

#[test]
fn a_dedup_key_resolves_to_its_event() {
    let h = Harness::new();
    let id = h.enqueue(3);
    assert_eq!(h.client().get_by_dedup(&h.key(3)), id);
    assert_eq!(
        h.client().try_get_by_dedup(&h.key(200)),
        Err(Ok(ContractError::EventNotFound))
    );
}

// ── publication ─────────────────────────────────────────────────────────────

#[test]
fn publishing_clears_the_entry_from_the_backlog_exactly_once() {
    let h = Harness::new();
    let id = h.enqueue(1);
    assert_eq!(h.client().pending_count(), 1);

    h.client().mark_published(&h.operator, &id);
    assert_eq!(h.client().pending_count(), 0);
    assert_eq!(h.client().get_event(&id).published_at, 1_000);

    // A relay that retries its acknowledgement is told so, rather than
    // being allowed to decrement the backlog a second time.
    assert_eq!(
        h.client().try_mark_published(&h.operator, &id),
        Err(Ok(ContractError::AlreadyPublished))
    );
    assert_eq!(h.client().pending_count(), 0);
}

#[test]
fn a_non_relay_cannot_publish() {
    let h = Harness::new();
    let id = h.enqueue(1);
    assert_eq!(
        h.client().try_mark_published(&h.consumer, &id),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(h.client().get_event(&id).published_at, 0);
    assert_eq!(h.client().pending_count(), 1);
}

#[test]
fn publishing_an_unknown_event_is_refused() {
    let h = Harness::new();
    assert_eq!(
        h.client().try_mark_published(&h.operator, &999),
        Err(Ok(ContractError::EventNotFound))
    );
}

// ── consumer cursors ────────────────────────────────────────────────────────

#[test]
fn a_cursor_advances_and_is_per_consumer() {
    let h = Harness::new();
    let a = h.enqueue(1);
    let b = h.enqueue(2);
    let c = h.enqueue(3);

    assert_eq!(h.client().get_cursor(&h.consumer), 0);
    h.client().acknowledge(&h.consumer, &b);
    assert_eq!(h.client().get_cursor(&h.consumer), b);
    // Another consumer's position is independent.
    assert_eq!(h.client().get_cursor(&h.other_consumer), 0);

    h.client().acknowledge(&h.consumer, &c);
    assert_eq!(h.client().get_cursor(&h.consumer), c);
    // Everything at or below the cursor is considered consumed.
    assert!(a < b && b < c);
}

#[test]
fn a_cursor_never_moves_backwards() {
    let h = Harness::new();
    let a = h.enqueue(1);
    let b = h.enqueue(2);
    h.client().acknowledge(&h.consumer, &b);
    assert_eq!(
        h.client().try_acknowledge(&h.consumer, &a),
        Err(Ok(ContractError::CursorRegression))
    );
    assert_eq!(h.client().get_cursor(&h.consumer), b);
}

#[test]
fn acknowledging_the_current_position_is_idempotent() {
    let h = Harness::new();
    let a = h.enqueue(1);
    assert_eq!(h.client().acknowledge(&h.consumer, &a), a);
    // A redelivered batch re-acknowledges the same head; the cursor stays
    // put and no error is raised, so a retrying consumer is not wedged.
    assert_eq!(h.client().acknowledge(&h.consumer, &a), a);
    assert_eq!(h.client().get_cursor(&h.consumer), a);
}

#[test]
fn a_consumer_cannot_ack_an_event_that_was_never_enqueued() {
    let h = Harness::new();
    h.enqueue(1);
    assert_eq!(
        h.client().try_acknowledge(&h.consumer, &999),
        Err(Ok(ContractError::EventNotFound))
    );
    // And the cursor did not move past the backlog.
    assert_eq!(h.client().get_cursor(&h.consumer), 0);
}

#[test]
fn a_cursor_only_moves_on_the_consumers_own_signature() {
    let h = Harness::new();
    let a = h.enqueue(1);
    // `acknowledge` has no role check because it does not need one: the
    // consumer's own `require_auth` is the control, and nobody else can
    // satisfy it on their behalf. Turn mocking off to see that hold.
    h.env.set_auths(&[]);
    assert!(h.client().try_acknowledge(&h.consumer, &a).is_err());
    assert!(h.client().try_acknowledge(&h.other_consumer, &a).is_err());
    assert_eq!(h.client().get_cursor(&h.consumer), 0);
}

// ── pruning ─────────────────────────────────────────────────────────────────

#[test]
fn pruning_removes_published_entries_older_than_the_horizon() {
    let h = Harness::new();
    let a = h.enqueue(1);
    let b = h.enqueue(2);
    h.client().mark_published(&h.operator, &a);
    h.client().mark_published(&h.operator, &b);
    h.env.ledger().set_timestamp(5_000);

    // Everything published before 5_000 goes.
    assert_eq!(h.client().prune(&h.operator, &4_000, &10), 2);
    assert_eq!(
        h.client().try_get_event(&a),
        Err(Ok(ContractError::EventNotFound))
    );
}

#[test]
fn pruning_never_drops_an_undelivered_event() {
    let h = Harness::new();
    let published = h.enqueue(1);
    let pending = h.enqueue(2);
    h.client().mark_published(&h.operator, &published);
    h.env.ledger().set_timestamp(9_000);

    // The sweep stops at the undelivered entry rather than skipping past
    // it, even though the pending one is older than the horizon.
    assert_eq!(h.client().prune(&h.operator, &5_000, &10), 1);
    assert_eq!(h.client().get_event(&pending).published_at, 0);
    assert_eq!(h.client().pending_count(), 1);
}

#[test]
fn pruning_resumes_where_it_stopped() {
    let h = Harness::new();
    let a = h.enqueue(1);
    let b = h.enqueue(2);
    let c = h.enqueue(3);
    h.client().mark_published(&h.operator, &a);
    h.client().mark_published(&h.operator, &b);
    h.client().mark_published(&h.operator, &c);
    h.env.ledger().set_timestamp(9_000);

    // A bounded sweep does part of the work...
    assert_eq!(h.client().prune(&h.operator, &5_000, &2), 2);
    assert!(h.client().get_event(&c).id == c);
    // ...and the next call finishes it, without rescanning `a` and `b`.
    assert_eq!(h.client().prune(&h.operator, &5_000, &10), 1);
    assert_eq!(
        h.client().try_get_event(&c),
        Err(Ok(ContractError::EventNotFound))
    );
    // A third call has nothing left to do.
    assert_eq!(h.client().prune(&h.operator, &5_000, &10), 0);
}

#[test]
fn a_future_retention_horizon_is_refused() {
    let h = Harness::new();
    h.enqueue(1);
    // Pruning to "now" in the future would delete events consumers have
    // not had a chance to read.
    assert_eq!(
        h.client().try_prune(&h.operator, &2_000, &10),
        Err(Ok(ContractError::InvalidRetentionHorizon))
    );
}

#[test]
fn a_prune_limit_outside_the_allowed_range_is_refused() {
    let h = Harness::new();
    h.enqueue(1);
    assert_eq!(
        h.client().try_prune(&h.operator, &500, &0),
        Err(Ok(ContractError::InvalidPruneLimit))
    );
    assert_eq!(
        h.client().try_prune(&h.operator, &500, &10_000),
        Err(Ok(ContractError::InvalidPruneLimit))
    );
}

#[test]
fn only_the_operator_may_prune() {
    let h = Harness::new();
    h.enqueue(1);
    assert_eq!(
        h.client().try_prune(&h.consumer, &500, &10),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn a_dedup_key_survives_its_event_being_pruned() {
    let h = Harness::new();
    let id = h.enqueue(1);
    h.client().mark_published(&h.operator, &id);
    h.env.ledger().set_timestamp(9_000);
    h.client().prune(&h.operator, &5_000, &10);

    // The event is gone, but the idempotency key is not: an orchestrator
    // retrying that enqueue much later must not mint a second event for a
    // state change that already happened.
    assert_eq!(h.client().get_by_dedup(&h.key(1)), id);
    assert_eq!(h.enqueue(1), id);
    assert_eq!(h.client().pending_count(), 0);
}

// ── privacy ─────────────────────────────────────────────────────────────────

#[test]
fn nothing_but_a_commitment_is_stored() {
    let h = Harness::new();
    let payload = Bytes::from_slice(&h.env, &[7u8; 64]);
    let hash: BytesN<32> = h.env.crypto().sha256(&payload).into();
    let id = h.client().enqueue(
        &h.operator,
        &h.source,
        &Symbol::new(&h.env, "application_submitted"),
        &1,
        &h.key(1),
        &h.key(2),
        &hash,
    );

    let event = h.client().get_event(&id);
    assert_eq!(event.payload_hash, hash);
    // The event type has no field a name, an email, or a free-text answer
    // could be smuggled into: an id, a topic, a version, two ids, a hash,
    // an address, and two timestamps. The 64-byte payload is not among
    // them, which is the point.
    assert_ne!(event.payload_hash, BytesN::from_array(&h.env, &[0u8; 32]));
}

#[test]
fn schema_versions_let_a_consumer_branch_without_guessing() {
    let h = Harness::new();
    let v1 = h.enqueue(1);
    let v2 = h.client().enqueue(
        &h.operator,
        &h.source,
        &Symbol::new(&h.env, "award_reserved"),
        &2,
        &h.key(3),
        &h.key(4),
        &h.key(5),
    );

    assert_eq!(h.client().get_event(&v1).schema_version, 1);
    assert_eq!(h.client().get_event(&v2).schema_version, 2);
    // Same topic, two versions: a consumer sees the topic it knows and
    // can branch on the version, rather than a silent shape change.
    assert_eq!(
        h.client().get_event(&v1).topic,
        h.client().get_event(&v2).topic
    );
}

#[test]
fn version_is_reported() {
    let h = Harness::new();
    assert_eq!(h.client().version(), CONTRACT_VERSION);
}
