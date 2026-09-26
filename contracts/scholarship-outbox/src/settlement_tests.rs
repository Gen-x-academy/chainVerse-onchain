//! Adversarial tests for settlement reconciliation (issue #1142).
//!
//! The acceptance criteria name four properties: explicit finality rules,
//! idempotent reprocessing, mismatches that alert operators, and frontend
//! status drawn from reconciled state. Each is tested here against the way it
//! would actually be broken -- by a retry, a swapped transaction, a
//! premature read, or a UI that renders whatever it is given.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::vec;

const AMOUNT: i128 = 1_000_000;

struct SettlementFixture {
    env: Env,
    contract: Address,
    operator: Address,
    backend: Address,
}

impl SettlementFixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        // Ledger 0 is not a real ledger, and the contract refuses a
        // transaction claimed to be in it, so the fixture starts somewhere
        // plausible.
        env.ledger().set_sequence_number(100);
        let contract = env.register(ScholarshipOutboxContract, ());
        let operator = Address::generate(&env);
        let backend = Address::generate(&env);
        ScholarshipOutboxContractClient::new(&env, &contract).initialize(&operator);
        Self {
            env,
            contract,
            operator,
            backend,
        }
    }

    fn client(&self) -> ScholarshipOutboxContractClient<'_> {
        ScholarshipOutboxContractClient::new(&self.env, &self.contract)
    }

    fn hash(&self, fill: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[fill; 32])
    }

    fn open(&self, id: u8, amount: i128) -> BytesN<32> {
        let intent_id = self.hash(id);
        self.client().open_settlement(
            &self.operator,
            &intent_id,
            &self.contract,
            &self.hash(99),
            &amount,
            &2,
        );
        intent_id
    }

    /// Open, observe a transaction in the current ledger, and advance far
    /// enough for it to be final. Returns the intent id.
    fn settled_path(&self, id: u8, amount: i128) -> BytesN<32> {
        let intent_id = self.open(id, amount);
        let ledger = self.env.ledger().sequence();
        self.client()
            .observe_settlement(&self.operator, &intent_id, &self.hash(7), &ledger);
        self.advance_ledgers(3);
        intent_id
    }

    fn advance_ledgers(&self, count: u32) {
        self.env
            .ledger()
            .set_sequence_number(self.env.ledger().sequence() + count);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Finality
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reconciling_before_the_finality_window_elapses_is_refused() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let ledger = f.env.ledger().sequence();
    f.client()
        .observe_settlement(&f.operator, &id, &f.hash(7), &ledger);

    // The transaction is observed but only one ledger has passed, and two
    // were required. Reconciling now is precisely the bug #1142 names: a
    // transaction reported as successful can still be rolled back.
    f.advance_ledgers(1);
    assert_eq!(
        f.client()
            .try_reconcile_settlement(&f.operator, &id, &AMOUNT),
        Err(Ok(ContractError::NotFinal))
    );
    assert_eq!(f.client().get_intent(&id).state, SettlementState::Pending);
}

#[test]
fn reconciling_exactly_at_the_finality_boundary_succeeds() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let ledger = f.env.ledger().sequence();
    f.client()
        .observe_settlement(&f.operator, &id, &f.hash(7), &ledger);
    f.advance_ledgers(2);
    // Off-by-one here would either reject a settled payment or accept an
    // unsettled one, so the boundary itself is the test.
    assert_eq!(
        f.client().reconcile_settlement(&f.operator, &id, &AMOUNT),
        SettlementState::Reconciled
    );
}

#[test]
fn a_zero_finality_threshold_is_refused() {
    let f = SettlementFixture::new();
    // "Final immediately" is the whole failure this contract exists to
    // prevent, so it must not be expressible.
    assert_eq!(
        f.client().try_open_settlement(
            &f.operator,
            &f.hash(1),
            &f.contract,
            &f.hash(99),
            &AMOUNT,
            &0,
        ),
        Err(Ok(ContractError::InvalidFinality))
    );
}

#[test]
fn an_unbounded_finality_threshold_is_refused() {
    let f = SettlementFixture::new();
    // Otherwise an intent could be parked in Pending forever, and the
    // failure mode of an unbounded window is silence.
    assert_eq!(
        f.client().try_open_settlement(
            &f.operator,
            &f.hash(1),
            &f.contract,
            &f.hash(99),
            &AMOUNT,
            &(MAX_FINALITY + 1),
        ),
        Err(Ok(ContractError::InvalidFinality))
    );
}

#[test]
fn the_maximum_finality_threshold_is_accepted() {
    let f = SettlementFixture::new();
    f.client().open_settlement(
        &f.operator,
        &f.hash(1),
        &f.contract,
        &f.hash(99),
        &AMOUNT,
        &MAX_FINALITY,
    );
    assert_eq!(
        f.client().get_intent(&f.hash(1)).required_finality,
        MAX_FINALITY
    );
}

#[test]
fn an_unobserved_intent_cannot_be_reconciled() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    // No transaction has been reported, so there is nothing to compare
    // against and no amount the caller could assert is authoritative.
    assert_eq!(
        f.client()
            .try_reconcile_settlement(&f.operator, &id, &AMOUNT),
        Err(Ok(ContractError::NotFinal))
    );
}

#[test]
fn a_transaction_from_a_future_ledger_is_refused() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let ahead = f.env.ledger().sequence() + 100;
    // Accepting a ledger that has not happened would make the finality
    // check pass for a transaction that does not exist.
    assert_eq!(
        f.client()
            .try_observe_settlement(&f.operator, &id, &f.hash(7), &ahead),
        Err(Ok(ContractError::NotFinal))
    );
}

#[test]
fn a_transaction_in_ledger_zero_is_refused() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    assert_eq!(
        f.client()
            .try_observe_settlement(&f.operator, &id, &f.hash(7), &0),
        Err(Ok(ContractError::NotFinal))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Idempotent reprocessing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn reprocessing_a_settled_intent_changes_nothing() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    assert_eq!(
        f.client().reconcile_settlement(&f.operator, &id, &AMOUNT),
        SettlementState::Reconciled
    );
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    let before = f.client().get_intent(&id);

    // A backend retry loop, or the same event delivered twice, must not be
    // able to move a settled verdict. The assertion is whole-record
    // equality, not field-by-field spot checks: reprocessing is a complete
    // no-op, `attempts` included.
    for _ in 0..3 {
        assert_eq!(
            f.client().reconcile_settlement(&f.operator, &id, &AMOUNT),
            SettlementState::Reconciled
        );
    }
    assert_eq!(f.client().get_intent(&id), before);
}

#[test]
fn reprocessing_cannot_flip_a_settled_intent_to_a_mismatch() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    // A late-arriving, wrong observation must not be able to unsettle a
    // payment that already reconciled.
    assert_eq!(
        f.client()
            .reconcile_settlement(&f.operator, &id, &(AMOUNT * 2)),
        SettlementState::Reconciled
    );
    assert_eq!(f.client().get_intent(&id).observed_amount, AMOUNT);
    assert_eq!(f.client().alert_count(), 0);
}

#[test]
fn reprocessing_cannot_clear_a_mismatch() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT + 1));
    assert_eq!(
        f.client().get_intent(&id).state,
        SettlementState::Mismatched
    );

    // The observed transaction is immutable, so the disagreement is too. If
    // a re-run with the *correct* amount could clear it, the alert would be
    // suppressible by the very backend that caused it.
    assert_eq!(
        f.client().reconcile_settlement(&f.operator, &id, &AMOUNT),
        SettlementState::Mismatched
    );
    assert_eq!(
        f.client().get_intent(&id).state,
        SettlementState::Mismatched
    );
    assert_eq!(f.client().alert_count(), 1);
}

#[test]
fn re_reporting_the_same_observation_is_a_no_op() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let ledger = f.env.ledger().sequence();
    f.client()
        .observe_settlement(&f.operator, &id, &f.hash(7), &ledger);
    f.client()
        .observe_settlement(&f.operator, &id, &f.hash(7), &ledger);
    assert_eq!(f.client().get_intent(&id).observed_ledger, ledger);
}

#[test]
fn a_second_transaction_cannot_replace_the_first() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let ledger = f.env.ledger().sequence();
    f.client()
        .observe_settlement(&f.operator, &id, &f.hash(7), &ledger);
    // If this were allowed, a backend could swap in a transaction that
    // happens to agree with its intent and make a real mismatch vanish.
    assert_eq!(
        f.client()
            .try_observe_settlement(&f.operator, &id, &f.hash(8), &ledger),
        Err(Ok(ContractError::AlreadyObserved))
    );
    assert_eq!(f.client().get_intent(&id).observed_tx_hash, f.hash(7));
}

#[test]
fn opening_the_same_intent_twice_is_refused() {
    let f = SettlementFixture::new();
    f.open(1, AMOUNT);
    // Content-derived ids mean a repeat is a producer bug, not an update.
    assert_eq!(
        f.client().try_open_settlement(
            &f.operator,
            &f.hash(1),
            &f.contract,
            &f.hash(99),
            &AMOUNT,
            &2,
        ),
        Err(Ok(ContractError::IntentExists))
    );
    assert_eq!(f.client().tracked_intents(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Mismatches alert
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_mismatch_is_recorded_and_counted() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    assert_eq!(
        f.client()
            .reconcile_settlement(&f.operator, &id, &(AMOUNT - 1)),
        SettlementState::Mismatched
    );
    // A running count means a monitor can alert without tailing every
    // reconciliation topic, and it only falls when somebody resolves it.
    assert_eq!(f.client().alert_count(), 1);
}

#[test]
fn a_missing_transaction_alerts() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    f.client().expire_settlement(&f.operator, &id);
    // "No transaction" needs a different response from "the wrong amount",
    // but it is equally a case where the backend and the chain disagree.
    assert_eq!(f.client().get_intent(&id).state, SettlementState::Expired);
    assert_eq!(f.client().alert_count(), 1);
}

#[test]
fn a_settled_intent_does_not_alert() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    assert_eq!(f.client().alert_count(), 0);
}

#[test]
fn a_mismatch_is_resolved_only_explicitly() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT * 3));
    assert_eq!(
        f.client().get_intent(&id).state,
        SettlementState::Mismatched
    );

    f.client().resolve_settlement(&f.operator, &id, &f.hash(42));
    let intent = f.client().get_intent(&id);
    assert_eq!(intent.state, SettlementState::Reconciled);
    // The reason is committed by hash, so the resolution is auditable
    // without putting free text on chain.
    assert_eq!(intent.resolution_hash, f.hash(42));
}

#[test]
fn resolving_without_a_reason_is_refused() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT * 3));
    assert_eq!(
        f.client().try_resolve_settlement(
            &f.operator,
            &id,
            &BytesN::from_array(&f.env, &[0u8; 32]),
        ),
        Err(Ok(ContractError::EmptyCommitment))
    );
    assert_eq!(
        f.client().get_intent(&id).state,
        SettlementState::Mismatched
    );
}

#[test]
fn a_settled_intent_cannot_be_resolved() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    // Resolving something that is already fine is a caller bug, and
    // surfacing it beats a no-op that hides a confused operator script.
    assert_eq!(
        f.client()
            .try_resolve_settlement(&f.operator, &id, &f.hash(42)),
        Err(Ok(ContractError::InvalidState))
    );
}

#[test]
fn a_pending_intent_cannot_be_resolved() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    assert_eq!(
        f.client()
            .try_resolve_settlement(&f.operator, &id, &f.hash(42)),
        Err(Ok(ContractError::InvalidState))
    );
}

#[test]
fn an_expired_intent_cannot_be_reconciled() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    f.client().expire_settlement(&f.operator, &id);
    f.advance_ledgers(10);
    // Expiry is terminal for the same reason a mismatch is: the observation
    // window closed, and that is not something a later call can undo. The
    // answer is the existing state rather than an error, because reconcile
    // reports state and never fails on a decided intent.
    assert_eq!(
        f.client().reconcile_settlement(&f.operator, &id, &AMOUNT),
        SettlementState::Expired
    );
    assert_eq!(f.client().get_intent(&id).state, SettlementState::Expired);
    assert_eq!(f.client().alert_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Frontend status uses reconciled state
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_pending_intent_reports_no_settled_amount() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    let view = f.client().settlement_status(&id);
    // The point of the whole contract: a UI reading this cannot render a
    // pending intent as a settled payment, because the number is not there.
    assert_eq!(view.state, SettlementState::Pending);
    assert_eq!(view.settled_amount, 0);
}

#[test]
fn a_mismatched_intent_reports_no_settled_amount() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT * 5));
    let view = f.client().settlement_status(&id);
    // A disputed amount is exactly what a UI must not show as money the
    // sponsor received.
    assert_eq!(view.state, SettlementState::Mismatched);
    assert_eq!(view.settled_amount, 0);
    // The observed figure is still available, because an operator
    // reconciling by hand needs to see both sides.
    assert_eq!(view.observed_amount, AMOUNT * 5);
}

#[test]
fn a_reconciled_intent_reports_the_settled_amount() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    let view = f.client().settlement_status(&id);
    assert_eq!(view.state, SettlementState::Reconciled);
    assert_eq!(view.settled_amount, AMOUNT);
}

#[test]
fn settled_amount_refuses_to_answer_for_an_unreconciled_intent() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    // A caller that wants a number has to handle the error, rather than
    // receiving a zero it might render as "nothing was paid".
    assert_eq!(
        f.client().try_settled_amount(&id),
        Err(Ok(ContractError::InvalidState))
    );
    let settled = f.settled_path(2, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &settled, &AMOUNT);
    assert_eq!(f.client().settled_amount(&settled), AMOUNT);
}

#[test]
fn a_resolved_mismatch_can_report_the_observed_amount() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT * 5));
    f.client().resolve_settlement(&f.operator, &id, &f.hash(42));
    // Once an operator has signed off, the observed figure becomes the
    // settled one.
    assert_eq!(f.client().settled_amount(&id), AMOUNT * 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// Authorization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_non_operator_cannot_open_an_intent() {
    let f = SettlementFixture::new();
    assert_eq!(
        f.client().try_open_settlement(
            &f.backend,
            &f.hash(1),
            &f.contract,
            &f.hash(99),
            &AMOUNT,
            &2,
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn a_non_operator_cannot_reconcile() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    assert_eq!(
        f.client()
            .try_reconcile_settlement(&f.backend, &id, &AMOUNT),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(f.client().get_intent(&id).state, SettlementState::Pending);
}

#[test]
fn a_non_operator_cannot_archive() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    assert_eq!(
        f.client().try_archive_settlement(&f.backend, &id),
        Err(Ok(ContractError::NotAdmin))
    );
    assert!(f.client().try_get_intent(&id).is_ok());
}

#[test]
fn settlement_operations_require_a_signature() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    f.env.set_auths(&[]);
    assert!(f
        .client()
        .try_reconcile_settlement(&f.operator, &id, &AMOUNT)
        .is_err());
}

#[test]
fn an_unknown_intent_is_reported_as_such() {
    let f = SettlementFixture::new();
    assert_eq!(
        f.client().try_settlement_status(&f.hash(200)),
        Err(Ok(ContractError::IntentNotFound))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Amounts and bounds
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_zero_amount_intent_is_refused() {
    let f = SettlementFixture::new();
    // A settlement of zero is not a settlement, and allowing it would let a
    // zero-valued intent read as reconciled.
    assert_eq!(
        f.client()
            .try_open_settlement(&f.operator, &f.hash(1), &f.contract, &f.hash(99), &0, &2,),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn a_negative_amount_intent_is_refused() {
    let f = SettlementFixture::new();
    assert_eq!(
        f.client()
            .try_open_settlement(&f.operator, &f.hash(1), &f.contract, &f.hash(99), &-1, &2,),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn archiving_frees_a_slot() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client().reconcile_settlement(&f.operator, &id, &AMOUNT);
    assert_eq!(f.client().tracked_intents(), 1);
    f.client().archive_settlement(&f.operator, &id);
    assert_eq!(f.client().tracked_intents(), 0);
    assert_eq!(
        f.client().try_get_intent(&id),
        Err(Ok(ContractError::IntentNotFound))
    );
}

#[test]
fn a_disputed_intent_cannot_be_archived() {
    let f = SettlementFixture::new();
    let id = f.settled_path(1, AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &id, &(AMOUNT * 5));
    // The release valve must not double as a way to erase an open dispute.
    assert_eq!(
        f.client().try_archive_settlement(&f.operator, &id),
        Err(Ok(ContractError::InvalidState))
    );
    assert_eq!(f.client().tracked_intents(), 1);
}

#[test]
fn a_pending_intent_cannot_be_archived() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    assert_eq!(
        f.client().try_archive_settlement(&f.operator, &id),
        Err(Ok(ContractError::InvalidState))
    );
}

#[test]
fn a_pending_intent_cannot_be_expired_twice() {
    let f = SettlementFixture::new();
    let id = f.open(1, AMOUNT);
    f.client().expire_settlement(&f.operator, &id);
    assert_eq!(
        f.client().try_expire_settlement(&f.operator, &id),
        Err(Ok(ContractError::InvalidState))
    );
    // Expiry is not re-counted as a fresh alert on every failed retry.
    assert_eq!(f.client().alert_count(), 1);
}

#[test]
fn alerting_states_are_the_two_that_need_a_human() {
    assert!(SettlementState::Mismatched.is_alerting());
    assert!(SettlementState::Expired.is_alerting());
    assert!(!SettlementState::Pending.is_alerting());
    assert!(!SettlementState::Reconciled.is_alerting());
}

#[test]
fn many_intents_stay_independent() {
    let f = SettlementFixture::new();
    let mut ids = vec![&f.env];
    for n in 0..5u8 {
        ids.push_back(f.settled_path(n, AMOUNT + n as i128));
    }
    // Settle one, mismatch another, leave the rest pending; no intent may
    // notice another's outcome.
    f.client()
        .reconcile_settlement(&f.operator, &ids.get(0).unwrap(), &AMOUNT);
    f.client()
        .reconcile_settlement(&f.operator, &ids.get(1).unwrap(), &(AMOUNT + 999));

    assert_eq!(
        f.client().get_intent(&ids.get(0).unwrap()).state,
        SettlementState::Reconciled
    );
    assert_eq!(
        f.client().get_intent(&ids.get(1).unwrap()).state,
        SettlementState::Mismatched
    );
    assert_eq!(
        f.client().get_intent(&ids.get(2).unwrap()).state,
        SettlementState::Pending
    );
    assert_eq!(f.client().alert_count(), 1);
    assert_eq!(f.client().tracked_intents(), 5);
}
