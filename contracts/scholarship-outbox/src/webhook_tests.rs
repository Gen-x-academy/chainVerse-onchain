//! Adversarial tests for the signed sponsor webhook surface (issue #1141).
//!
//! The acceptance criteria for #1141 name six properties — signed,
//! timestamped, retryable, replay-resistant, secret rotation, visible
//! delivery history. These tests are organised by the *attack* each property
//! is supposed to stop, so a regression shows up as a named failure rather
//! than a quietly weaker guarantee.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};

use soroban_sdk::vec;

struct WebhookFixture {
    env: Env,
    contract: Address,
    operator: Address,
    sponsor: Address,
    relay: Address,
    other: Address,
}

impl WebhookFixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let contract = env.register(ScholarshipOutboxContract, ());
        let operator = Address::generate(&env);
        let sponsor = Address::generate(&env);
        let relay = Address::generate(&env);
        let other = Address::generate(&env);
        ScholarshipOutboxContractClient::new(&env, &contract).initialize(&operator);
        Self {
            env,
            contract,
            operator,
            sponsor,
            relay,
            other,
        }
    }

    fn client(&self) -> ScholarshipOutboxContractClient<'_> {
        ScholarshipOutboxContractClient::new(&self.env, &self.contract)
    }

    fn hash(&self, fill: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[fill; 32])
    }

    fn topic(&self, name: &str) -> Symbol {
        Symbol::new(&self.env, name)
    }

    /// An endpoint subscribed to `award_reserved` and `award_paid`.
    fn endpoint(&self) -> u64 {
        self.client().register_endpoint(
            &self.operator,
            &self.sponsor,
            &self.hash(10),
            &self.hash(11),
            &vec![
                &self.env,
                self.topic("award_reserved"),
                self.topic("award_paid"),
            ],
        )
    }

    /// Enqueue an `award_reserved` event, the only topic an enrolled
    /// endpoint subscribes to.
    fn event(&self) -> u64 {
        self.client().enqueue(
            &self.operator,
            &self.contract,
            &self.topic("award_reserved"),
            &1,
            &self.hash(1),
            &self.hash(2),
            &self.hash(3),
        )
    }

    /// A far-future ledger time, so signature windows are not expired by
    /// accident in tests that are not about expiry.
    fn advance(&self, seconds: u64) {
        self.env
            .ledger()
            .set_timestamp(self.env.ledger().timestamp() + seconds);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Enrollment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn an_endpoint_enrolls_with_its_owner_and_a_fresh_secret() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let e = f.client().get_endpoint(&id);

    assert_eq!(e.id, 1);
    assert_eq!(e.owner, f.sponsor);
    assert_eq!(e.url_hash, f.hash(10));
    assert_eq!(e.secret_hash, f.hash(11));
    assert!(e.active);
    // Epoch starts at 1, so zero is never a live generation and a
    // signature can never be attributed to "no secret".
    assert_eq!(e.secret_epoch, 1);
    assert_eq!(e.created_at, f.env.ledger().timestamp());
}

#[test]
fn the_operator_alone_cannot_enroll_a_sponsor() {
    let f = WebhookFixture::new();
    // No auths at all: the owner's signature is genuinely required, so a
    // compromised platform key cannot start shipping one sponsor's events
    // to an attacker's URL.
    f.env.set_auths(&[]);
    assert!(f
        .client()
        .try_register_endpoint(
            &f.operator,
            &f.sponsor,
            &f.hash(10),
            &f.hash(11),
            &vec![&f.env, f.topic("award_reserved")],
        )
        .is_err());
}

#[test]
fn a_random_caller_cannot_enroll_an_endpoint() {
    let f = WebhookFixture::new();
    f.env.set_auths(&[]);
    assert!(f
        .client()
        .try_register_endpoint(
            &f.relay,
            &f.sponsor,
            &f.hash(10),
            &f.hash(11),
            &vec![&f.env, f.topic("award_reserved")],
        )
        .is_err());
}

#[test]
fn a_zero_secret_commitment_is_refused() {
    let f = WebhookFixture::new();
    // Otherwise "no secret configured" and "secret of all zeroes" would be
    // the same state, and the second is not a value anybody chose.
    assert_eq!(
        f.client().try_register_endpoint(
            &f.operator,
            &f.sponsor,
            &f.hash(10),
            &BytesN::from_array(&f.env, &[0u8; 32]),
            &vec![&f.env, f.topic("award_reserved")],
        ),
        Err(Ok(ContractError::EmptyCommitment))
    );
}

#[test]
fn a_zero_url_commitment_is_refused() {
    let f = WebhookFixture::new();
    assert_eq!(
        f.client().try_register_endpoint(
            &f.operator,
            &f.sponsor,
            &BytesN::from_array(&f.env, &[0u8; 32]),
            &f.hash(11),
            &vec![&f.env, f.topic("award_reserved")],
        ),
        Err(Ok(ContractError::EmptyCommitment))
    );
}

#[test]
fn an_endpoint_subscribing_to_nothing_is_refused() {
    let f = WebhookFixture::new();
    // It could never be sent anything, so enrolling it is a config mistake.
    assert_eq!(
        f.client().try_register_endpoint(
            &f.operator,
            &f.sponsor,
            &f.hash(10),
            &f.hash(11),
            &vec![&f.env],
        ),
        Err(Ok(ContractError::NoTopics))
    );
}

#[test]
fn an_oversized_subscription_is_refused() {
    let f = WebhookFixture::new();
    // One more than the cap, spelled out because the crate is `no_std` and
    // cannot build symbol names at runtime.
    let topics = vec![
        &f.env,
        Symbol::new(&f.env, "t00"),
        Symbol::new(&f.env, "t01"),
        Symbol::new(&f.env, "t02"),
        Symbol::new(&f.env, "t03"),
        Symbol::new(&f.env, "t04"),
        Symbol::new(&f.env, "t05"),
        Symbol::new(&f.env, "t06"),
        Symbol::new(&f.env, "t07"),
        Symbol::new(&f.env, "t08"),
        Symbol::new(&f.env, "t09"),
        Symbol::new(&f.env, "t10"),
        Symbol::new(&f.env, "t11"),
        Symbol::new(&f.env, "t12"),
        Symbol::new(&f.env, "t13"),
        Symbol::new(&f.env, "t14"),
        Symbol::new(&f.env, "t15"),
        Symbol::new(&f.env, "t16"),
    ];
    assert_eq!(topics.len(), MAX_ENDPOINT_TOPICS + 1);
    assert_eq!(
        f.client().try_register_endpoint(
            &f.operator,
            &f.sponsor,
            &f.hash(10),
            &f.hash(11),
            &topics,
        ),
        Err(Ok(ContractError::TooManyTopics))
    );
}

#[test]
fn endpoint_ids_are_distinct() {
    let f = WebhookFixture::new();
    let first = f.endpoint();
    let second = f.client().register_endpoint(
        &f.operator,
        &f.other,
        &f.hash(20),
        &f.hash(21),
        &vec![&f.env, f.topic("award_reserved")],
    );
    assert_ne!(first, second);
}

// ═══════════════════════════════════════════════════════════════════════════
// Secret rotation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn rotation_bumps_the_epoch_and_replaces_the_commitment() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let epoch = f.client().rotate_secret(&f.operator, &id, &f.hash(77));
    assert_eq!(epoch, 2);

    let e = f.client().get_endpoint(&id);
    assert_eq!(e.secret_epoch, 2);
    assert_eq!(e.secret_hash, f.hash(77));
    // A rotation must not disturb delivery ordering, or it would hand a
    // relay a sequence gap in the middle of a retry storm.
    assert_eq!(e.last_sequence, 0);
}

#[test]
fn rotation_invalidates_a_signature_captured_under_the_old_secret() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    let before = f.client().delivery_payload(&id, &event, &1, &expiry);
    f.client().rotate_secret(&f.operator, &id, &f.hash(77));
    let after = f.client().delivery_payload(&id, &event, &1, &expiry);

    // The whole point of the epoch being in the preimage: a signature
    // captured before an emergency rotation stops verifying, with no
    // revocation list.
    assert_ne!(before, after);
}

#[test]
fn a_non_operator_cannot_rotate_a_secret() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    assert_eq!(
        f.client().try_rotate_secret(&f.relay, &id, &f.hash(77)),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(f.client().get_endpoint(&id).secret_hash, f.hash(11));
}

#[test]
fn rotation_of_an_unknown_endpoint_is_refused() {
    let f = WebhookFixture::new();
    assert_eq!(
        f.client().try_rotate_secret(&f.operator, &99, &f.hash(77)),
        Err(Ok(ContractError::EndpointNotFound))
    );
}

#[test]
fn rotation_to_a_zero_secret_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    assert_eq!(
        f.client()
            .try_rotate_secret(&f.operator, &id, &BytesN::from_array(&f.env, &[0u8; 32])),
        Err(Ok(ContractError::EmptyCommitment))
    );
    // A refused rotation must leave the live secret alone.
    assert_eq!(f.client().get_endpoint(&id).secret_hash, f.hash(11));
}

// ═══════════════════════════════════════════════════════════════════════════
// Selection — "selected program and award events"
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_subscribed_event_is_deliverable() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    assert!(f.client().deliverable(&id, &f.event()));
}

#[test]
fn an_unsubscribed_event_is_not_deliverable() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let other = f.client().enqueue(
        &f.operator,
        &f.contract,
        &f.topic("application_submitted"),
        &1,
        &f.hash(4),
        &f.hash(5),
        &f.hash(6),
    );
    // If the relay ignored this, an applicant submission would reach a
    // sponsor that never asked for it, and nothing downstream would catch
    // the leak.
    assert!(!f.client().deliverable(&id, &other));
}

#[test]
fn an_inactive_endpoint_is_deliverable_to_nobody() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    f.client().set_endpoint_active(&f.operator, &id, &false);
    assert!(!f.client().deliverable(&id, &event));
}

#[test]
fn deactivation_keeps_the_delivery_history() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.client().set_endpoint_active(&f.operator, &id, &false);

    // A sponsor muted for a week still needs to see what it missed.
    assert_eq!(f.client().delivery_history_count(&id), 1);
    assert_eq!(f.client().get_delivery(&id, &1).event_id, event);
}

#[test]
fn a_non_operator_cannot_deactivate_an_endpoint() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    assert_eq!(
        f.client().try_set_endpoint_active(&f.relay, &id, &false),
        Err(Ok(ContractError::NotAdmin))
    );
    assert!(f.client().get_endpoint(&id).active);
}

// ═══════════════════════════════════════════════════════════════════════════
// Signature windows and expiry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn an_expiry_beyond_the_maximum_window_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    // Otherwise the operator could mint a signature that stays useful
    // indefinitely, which is what the expiry exists to prevent.
    assert_eq!(
        f.client()
            .try_delivery_payload(&id, &event, &1, &(now + MAX_SIGNATURE_TTL + 1)),
        Err(Ok(ContractError::InvalidExpiry))
    );
}

#[test]
fn an_expiry_in_the_past_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.advance(100);
    assert_eq!(
        f.client().try_delivery_payload(&id, &event, &1, &now),
        Err(Ok(ContractError::InvalidExpiry))
    );
}

#[test]
fn the_maximum_window_is_accepted() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    assert!(f
        .client()
        .try_delivery_payload(&id, &event, &1, &(now + MAX_SIGNATURE_TTL))
        .is_ok());
}

#[test]
fn a_payload_for_an_unknown_event_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let now = f.env.ledger().timestamp();
    assert_eq!(
        f.client().try_delivery_payload(&id, &404, &1, &(now + 600)),
        Err(Ok(ContractError::EventNotFound))
    );
}

#[test]
fn a_payload_for_an_inactive_endpoint_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().set_endpoint_active(&f.operator, &id, &false);
    assert_eq!(
        f.client()
            .try_delivery_payload(&id, &event, &1, &(now + 600)),
        Err(Ok(ContractError::EndpointInactive))
    );
}

#[test]
fn the_payload_is_derived_not_supplied() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    // Unauthenticated on purpose: a sponsor recomputing this to check an
    // incoming delivery is the whole reason it is public.
    f.env.set_auths(&[]);
    let a = f.client().delivery_payload(&id, &event, &1, &expiry);
    let b = f.client().delivery_payload(&id, &event, &1, &expiry);
    assert_eq!(a, b, "a sponsor must be able to recompute the digest");
}

// ═══════════════════════════════════════════════════════════════════════════
// Ordering and replay
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_signature_cannot_be_minted_for_a_future_sequence() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    // Pre-signing sequence 5 lets the operator skip ahead of deliveries a
    // sponsor never saw, leaving a hole the sponsor cannot detect.
    assert_eq!(
        f.client()
            .try_delivery_payload(&id, &event, &5, &(now + 600)),
        Err(Ok(ContractError::SequenceRegression))
    );
}

#[test]
fn a_signature_cannot_be_minted_for_a_replayed_sequence() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    assert_eq!(
        f.client()
            .try_delivery_payload(&id, &event, &1, &(now + 600)),
        Err(Ok(ContractError::SequenceRegression))
    );
}

#[test]
fn a_replayed_delivery_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &expiry,
        &DeliveryOutcome::Delivered,
    );
    // The same signed delivery presented twice must not become two records.
    assert_eq!(
        f.client().try_record_delivery(
            &f.operator,
            &id,
            &event,
            &1,
            &expiry,
            &DeliveryOutcome::Delivered
        ),
        Err(Ok(ContractError::SequenceRegression))
    );
    assert_eq!(f.client().delivery_history_count(&id), 1);
}

#[test]
fn sequences_are_gap_free() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();

    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    // Skipping 2 would leave the sponsor unable to tell "nothing happened"
    // from "a delivery was lost".
    assert_eq!(
        f.client().try_record_delivery(
            &f.operator,
            &id,
            &event,
            &3,
            &(now + 600),
            &DeliveryOutcome::Delivered
        ),
        Err(Ok(ContractError::SequenceRegression))
    );
}

#[test]
fn an_expired_delivery_cannot_be_recorded() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    // Signing happens now, the window closes, and only then is the
    // delivery recorded. This is what a replay looks like, so it is
    // refused rather than laundered into ordinary history.
    f.advance(601);
    assert_eq!(
        f.client().try_record_delivery(
            &f.operator,
            &id,
            &event,
            &1,
            &expiry,
            &DeliveryOutcome::Delivered
        ),
        Err(Ok(ContractError::ExpiredDelivery))
    );
    assert_eq!(f.client().delivery_history_count(&id), 0);
}

#[test]
fn a_non_operator_cannot_record_a_delivery() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    assert_eq!(
        f.client().try_record_delivery(
            &f.relay,
            &id,
            &event,
            &1,
            &(now + 600),
            &DeliveryOutcome::Delivered
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn an_unsubscribed_event_cannot_be_delivered_even_by_the_operator() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let other = f.client().enqueue(
        &f.operator,
        &f.contract,
        &f.topic("application_submitted"),
        &1,
        &f.hash(4),
        &f.hash(5),
        &f.hash(6),
    );
    let now = f.env.ledger().timestamp();
    // The operator is trusted to be honest about *what* it sent, but not
    // to be trusted with the subscription filter — that is the one control
    // standing between a sponsor and events it never asked for.
    for outcome in [DeliveryOutcome::Delivered, DeliveryOutcome::Failed] {
        assert_eq!(
            f.client()
                .try_record_delivery(&f.operator, &id, &other, &1, &(now + 600), &outcome),
            Err(Ok(ContractError::NotSubscribed))
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Retries
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_retry_gets_a_new_sequence_and_an_incrementing_attempt() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    assert_eq!(
        f.client().record_delivery(
            &f.operator,
            &id,
            &event,
            &1,
            &expiry,
            &DeliveryOutcome::Failed
        ),
        1
    );
    assert_eq!(
        f.client().record_delivery(
            &f.operator,
            &id,
            &event,
            &2,
            &expiry,
            &DeliveryOutcome::Delivered
        ),
        2
    );

    // A retry is a distinct, countable attempt on the same event rather
    // than an indistinguishable repeat.
    assert_eq!(f.client().get_attempts(&id, &event), 2);
    assert_eq!(f.client().get_delivery(&id, &2).attempt, 2);
    assert_eq!(
        f.client().get_delivery(&id, &2).outcome,
        DeliveryOutcome::Delivered
    );
    assert_eq!(
        f.client().get_delivery(&id, &1).outcome,
        DeliveryOutcome::Failed
    );
}

#[test]
fn a_skipped_delivery_is_recorded_as_such() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let other = f.client().enqueue(
        &f.operator,
        &f.contract,
        &f.topic("application_submitted"),
        &1,
        &f.hash(4),
        &f.hash(5),
        &f.hash(6),
    );
    let now = f.env.ledger().timestamp();
    // A relay that chooses to skip still advances the sequence, so the
    // history shows a deliberate decision instead of a silent gap. Only
    // `Skipped` may name an unsubscribed event — a relay cannot dress an
    // event it actually sent up as a skip.
    f.client().record_delivery(
        &f.operator,
        &id,
        &other,
        &1,
        &(now + 600),
        &DeliveryOutcome::Skipped,
    );
    assert_eq!(
        f.client().get_delivery(&id, &1).outcome,
        DeliveryOutcome::Skipped
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Sponsor acknowledgement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn the_sponsor_acknowledges_its_own_delivery() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.advance(30);
    f.client().acknowledge_delivery(&f.sponsor, &id, &1);

    assert_eq!(f.client().get_acked_sequence(&id), 1);
    // Only the sponsor's own signature records that something arrived; the
    // relay's claim that it sent is not the same fact.
    assert_eq!(f.client().get_delivery(&id, &1).acked_at, now + 30);
}

#[test]
fn a_sponsor_cannot_acknowledge_without_its_own_signature() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.env.set_auths(&[]);
    assert!(f
        .client()
        .try_acknowledge_delivery(&f.sponsor, &id, &1)
        .is_err());
}

#[test]
fn one_sponsor_cannot_acknowledge_anothers_endpoint() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    // Even with a valid signature from `other`, it is not this endpoint's
    // owner, so the check on stored ownership is what stops it.
    assert_eq!(
        f.client().try_acknowledge_delivery(&f.other, &id, &1),
        Err(Ok(ContractError::NotEndpointOwner))
    );
}

#[test]
fn a_replayed_acknowledgement_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.client().acknowledge_delivery(&f.sponsor, &id, &1);
    assert_eq!(
        f.client().try_acknowledge_delivery(&f.sponsor, &id, &1),
        Err(Ok(ContractError::ReplayDetected))
    );
}

#[test]
fn an_acknowledgement_cannot_move_backwards() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &1,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &2,
        &(now + 600),
        &DeliveryOutcome::Delivered,
    );
    f.client().acknowledge_delivery(&f.sponsor, &id, &2);
    // Going back to 1 would let a sponsor appear to have a confirmed
    // delivery it has moved past.
    assert_eq!(
        f.client().try_acknowledge_delivery(&f.sponsor, &id, &1),
        Err(Ok(ContractError::ReplayDetected))
    );
}

#[test]
fn acknowledging_a_delivery_that_was_never_made_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    // Sequence 1 has not been delivered, so there is nothing to confirm.
    assert_eq!(
        f.client().try_acknowledge_delivery(&f.sponsor, &id, &1),
        Err(Ok(ContractError::DeliveryNotFound))
    );
}

#[test]
fn acknowledging_sequence_zero_is_refused() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    // 0 is not a valid sequence, and treating it as "ack nothing" would
    // silently succeed and hide a relay bug.
    assert_eq!(
        f.client().try_acknowledge_delivery(&f.sponsor, &id, &0),
        Err(Ok(ContractError::DeliveryNotFound))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Bounded history
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn history_is_bounded_and_evicts_the_oldest() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + MAX_SIGNATURE_TTL;

    for seq in 1u64..=MAX_DELIVERY_HISTORY as u64 {
        f.client().record_delivery(
            &f.operator,
            &id,
            &event,
            &seq,
            &expiry,
            &DeliveryOutcome::Delivered,
        );
    }
    assert_eq!(f.client().delivery_history_count(&id), MAX_DELIVERY_HISTORY);
    assert_eq!(f.client().delivery_history_floor(&id), 1);

    // One more than the window holds, so the oldest record is evicted and
    // the floor advances.
    f.client().record_delivery(
        &f.operator,
        &id,
        &event,
        &(MAX_DELIVERY_HISTORY as u64 + 1),
        &expiry,
        &DeliveryOutcome::Delivered,
    );
    assert_eq!(f.client().delivery_history_count(&id), MAX_DELIVERY_HISTORY);
    assert_eq!(f.client().delivery_history_floor(&id), 2);
    assert!(f.client().try_get_delivery(&id, &1).is_err());
    assert!(f.client().try_get_delivery(&id, &2).is_ok());
}

#[test]
fn attempt_counts_survive_history_eviction() {
    let f = WebhookFixture::new();
    let id = f.endpoint();
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + MAX_SIGNATURE_TTL;

    for seq in 1u64..=MAX_DELIVERY_HISTORY as u64 + 1 {
        f.client().record_delivery(
            &f.operator,
            &id,
            &event,
            &seq,
            &expiry,
            &DeliveryOutcome::Delivered,
        );
    }
    // The oldest *record* is gone, but the sponsor can still see that the
    // event was attempted 257 times, because the counter is a separate
    // key that eviction does not touch.
    assert!(f.client().try_get_delivery(&id, &1).is_err());
    assert_eq!(
        f.client().get_attempts(&id, &event),
        MAX_DELIVERY_HISTORY + 1
    );
}

#[test]
fn endpoints_are_capped() {
    let f = WebhookFixture::new();
    for _ in 0..MAX_ENDPOINTS {
        f.endpoint();
    }
    assert_eq!(
        f.client().try_register_endpoint(
            &f.operator,
            &f.other,
            &f.hash(30),
            &f.hash(31),
            &vec![&f.env, f.topic("award_reserved")],
        ),
        Err(Ok(ContractError::TooManyEndpoints))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Isolation between endpoints
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn endpoints_have_independent_sequences_and_acks() {
    let f = WebhookFixture::new();
    let first = f.endpoint();
    let second = f.client().register_endpoint(
        &f.operator,
        &f.other,
        &f.hash(20),
        &f.hash(21),
        &vec![&f.env, f.topic("award_reserved")],
    );
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    f.client().record_delivery(
        &f.operator,
        &first,
        &event,
        &1,
        &expiry,
        &DeliveryOutcome::Delivered,
    );
    f.client().record_delivery(
        &f.operator,
        &second,
        &event,
        &1,
        &expiry,
        &DeliveryOutcome::Delivered,
    );
    f.client().acknowledge_delivery(&f.sponsor, &first, &1);

    // One sponsor acking must not advance or imply anything for the other,
    // or a shared view would be able to forge a delivery confirmation.
    assert_eq!(f.client().get_acked_sequence(&first), 1);
    assert_eq!(f.client().get_acked_sequence(&second), 0);
    assert_eq!(f.client().get_delivery(&second, &1).acked_at, 0);
}

#[test]
fn a_signature_for_one_endpoint_does_not_verify_for_another() {
    let f = WebhookFixture::new();
    let first = f.endpoint();
    let second = f.client().register_endpoint(
        &f.operator,
        &f.other,
        &f.hash(20),
        &f.hash(21),
        &vec![&f.env, f.topic("award_reserved")],
    );
    let event = f.event();
    let now = f.env.ledger().timestamp();
    let expiry = now + 600;

    // Both endpoints are at sequence 1 for the same event with the same
    // expiry, so every other preimage field matches.
    let a = f.client().delivery_payload(&first, &event, &1, &expiry);
    let b = f.client().delivery_payload(&second, &event, &1, &expiry);
    // The endpoint id is in the preimage, so a sponsor A cannot be handed
    // a signature that sponsor B would accept.
    assert_ne!(a, b);
}
