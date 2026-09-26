use crate::{
    AwardStatus, ContractError, ScholarshipAwardsContract, ScholarshipAwardsContractClient,
};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, BytesN, Env, Symbol};

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAwardsContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipAwardsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> ScholarshipAwardsContractClient<'a> {
    ScholarshipAwardsContractClient::new(env, contract_id)
}

/// Create a standard offer expiring at `deadline`.
#[allow(clippy::too_many_arguments)]
fn offer(
    env: &Env,
    c: &ScholarshipAwardsContractClient,
    admin: &Address,
    award_id: &BytesN<32>,
    program_id: &BytesN<32>,
    recipient: &Address,
    amount: i128,
    deadline: u64,
) {
    c.create_award(
        admin,
        award_id,
        program_id,
        recipient,
        &amount,
        &Symbol::new(env, "USD"),
        &1u32,
        &hash(env, 7),
        &deadline,
        &None,
    );
}

// ── initialization ──────────────────────────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let result = c.try_initialize(&admin);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_non_admin_cannot_create_award() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let result = c.try_create_award(
        &attacker,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &1u32,
        &hash(&env, 7),
        &1_000u64,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
    // Silence the unused import in the no-op binding above.
    let _ = admin;
}

// ── #1090 — award records, deadlines, conflicts, reservations ──────────────

#[test]
fn test_create_award_records_fields_and_holds_reservation() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);

    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );

    let award = c.get_award(&hash(&env, 1));
    assert_eq!(award.amount, 5_000);
    assert_eq!(award.status, AwardStatus::Offered);
    assert!(award.reserved);
    assert_eq!(award.paid_amount, 0);
    assert_eq!(c.get_reservation(&pid), 1);
    assert!(c.has_active_award(&recipient, &pid));
}

#[test]
fn test_create_award_rejects_duplicate_id() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );

    let result = c.try_create_award(
        &admin,
        &hash(&env, 1),
        &hash(&env, 3),
        &Address::generate(&env),
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &1u32,
        &hash(&env, 7),
        &1_000u64,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::AwardAlreadyExists)));
}

#[test]
fn test_create_award_rejects_non_positive_amount() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let result = c.try_create_award(
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &Address::generate(&env),
        &0i128,
        &Symbol::new(&env, "USD"),
        &1u32,
        &hash(&env, 7),
        &1_000u64,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn test_create_award_rejects_past_deadline() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    env.ledger().set_timestamp(500);
    let result = c.try_create_award(
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &Address::generate(&env),
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &1u32,
        &hash(&env, 7),
        &500u64,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidDeadline)));
}

// #1090 — "an applicant cannot hold conflicting awards."
#[test]
fn test_conflicting_active_award_rejected() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );

    let result = c.try_create_award(
        &admin,
        &hash(&env, 9),
        &pid,
        &recipient,
        &1_000i128,
        &Symbol::new(&env, "USD"),
        &1u32,
        &hash(&env, 7),
        &2_000u64,
        &None,
    );
    assert_eq!(result, Err(Ok(ContractError::ConflictingAward)));
}

#[test]
fn test_same_recipient_can_hold_awards_for_different_programs() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 9),
        &hash(&env, 3),
        &recipient,
        5_000,
        1_000,
    );
    assert_eq!(c.get_reservation(&hash(&env, 2)), 1);
    assert_eq!(c.get_reservation(&hash(&env, 3)), 1);
}

#[test]
fn test_expired_offer_releases_reservation_and_pointer() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );

    // Deadline passes.
    env.ledger().set_timestamp(1_001);
    assert!(c.is_acceptance_expired(&hash(&env, 1)));

    c.expire_award(&hash(&env, 1));
    assert_eq!(c.get_award(&hash(&env, 1)).status, AwardStatus::Expired);
    assert_eq!(c.get_reservation(&pid), 0);
    assert!(!c.has_active_award(&recipient, &pid));
}

#[test]
fn test_expire_before_deadline_fails() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );

    let result = c.try_expire_award(&hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::OfferNotExpired)));
}

// ── #1091 — signed acceptance ───────────────────────────────────────────────

#[test]
fn test_accept_award_captures_signer_timestamp_and_terms() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );

    env.ledger().set_timestamp(400);
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));

    let acceptance = c.get_acceptance(&hash(&env, 1));
    assert_eq!(acceptance.signer, recipient);
    assert_eq!(acceptance.accepted_at, 400);
    assert_eq!(acceptance.terms_version, 1);
    assert_eq!(acceptance.terms_hash, hash(&env, 7));
    assert_eq!(acceptance.declarations_hash, hash(&env, 55));
    assert_eq!(c.get_award(&hash(&env, 1)).status, AwardStatus::Accepted);
}

#[test]
fn test_non_recipient_cannot_accept() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );

    let attacker = Address::generate(&env);
    let result = c.try_accept_award(&attacker, &hash(&env, 1), &hash(&env, 55));
    assert_eq!(result, Err(Ok(ContractError::NotRecipient)));
}

#[test]
fn test_accept_after_deadline_fails() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );

    env.ledger().set_timestamp(1_001);
    let result = c.try_accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));
    assert_eq!(result, Err(Ok(ContractError::OfferExpired)));
}

#[test]
fn test_declined_offer_cannot_disburse() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );

    c.decline_award(&recipient, &hash(&env, 1));
    assert_eq!(c.get_award(&hash(&env, 1)).status, AwardStatus::Declined);
    assert!(!c.can_disburse(&hash(&env, 1)));
    assert_eq!(c.get_reservation(&pid), 0);

    let result = c.try_record_disbursement(&admin, &hash(&env, 1), &100i128);
    assert_eq!(result, Err(Ok(ContractError::AwardNotDisburseable)));
}

// ── #1092 — cancellation and termination ────────────────────────────────────

#[test]
fn test_cancel_before_payment_succeeds_and_stops_disbursement() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));

    c.cancel_award(&admin, &hash(&env, 1), &7u32);

    let award = c.get_award(&hash(&env, 1));
    assert_eq!(award.status, AwardStatus::Cancelled);
    assert_eq!(award.reason_code, 7);
    assert!(!award.reserved);
    assert!(!c.can_disburse(&hash(&env, 1)));
    assert_eq!(c.get_reservation(&pid), 0);
    assert_eq!(
        c.try_record_disbursement(&admin, &hash(&env, 1), &100i128),
        Err(Ok(ContractError::AwardNotDisburseable))
    );
}

#[test]
fn test_cancel_after_payment_is_rejected() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));
    c.record_disbursement(&admin, &hash(&env, 1), &1_000i128);

    let result = c.try_cancel_award(&admin, &hash(&env, 1), &7u32);
    assert_eq!(result, Err(Ok(ContractError::PaymentAlreadyMade)));
}

#[test]
fn test_terminate_requires_a_payment() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));

    let result = c.try_terminate_award(&admin, &hash(&env, 1), &3u32, &0i128);
    assert_eq!(result, Err(Ok(ContractError::NoPaymentMade)));
}

#[test]
fn test_terminate_tracks_recovery_separately() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let pid = hash(&env, 2);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &pid,
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));
    c.record_disbursement(&admin, &hash(&env, 1), &2_000i128);

    c.terminate_award(&admin, &hash(&env, 1), &9u32, &1_500i128);

    let award = c.get_award(&hash(&env, 1));
    assert_eq!(award.status, AwardStatus::Terminated);
    assert_eq!(award.paid_amount, 2_000); // paid total is untouched
    assert_eq!(c.get_recovery(&hash(&env, 1)), 1_500); // recovery is separate
    assert!(!c.can_disburse(&hash(&env, 1)));
    assert_eq!(c.get_reservation(&pid), 0);
}

#[test]
fn test_terminate_rejects_recovery_over_paid() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));
    c.record_disbursement(&admin, &hash(&env, 1), &1_000i128);

    let result = c.try_terminate_award(&admin, &hash(&env, 1), &3u32, &1_001i128);
    assert_eq!(result, Err(Ok(ContractError::RecoveryExceedsPaid)));
}

#[test]
fn test_disbursement_cannot_exceed_award() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));

    let result = c.try_record_disbursement(&admin, &hash(&env, 1), &5_001i128);
    assert_eq!(result, Err(Ok(ContractError::DisbursementExceedsAward)));
}

#[test]
fn test_fully_paid_award_cannot_disburse_again() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));
    c.record_disbursement(&admin, &hash(&env, 1), &5_000i128);

    assert!(!c.can_disburse(&hash(&env, 1)));
}

#[test]
fn test_illegal_transition_declined_then_cancel() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.decline_award(&recipient, &hash(&env, 1));

    // Declined is terminal — cancel is not a legal transition from it.
    let result = c.try_cancel_award(&admin, &hash(&env, 1), &1u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidTransition)));
}

#[test]
fn test_cannot_accept_twice() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );
    c.accept_award(&recipient, &hash(&env, 1), &hash(&env, 55));

    let result = c.try_accept_award(&recipient, &hash(&env, 1), &hash(&env, 56));
    assert_eq!(result, Err(Ok(ContractError::OfferNotActive)));
}

#[test]
fn test_acceptance_not_found() {
    let (env, contract_id, admin) = setup();
    let c = client(&env, &contract_id);
    let recipient = Address::generate(&env);
    offer(
        &env,
        &c,
        &admin,
        &hash(&env, 1),
        &hash(&env, 2),
        &recipient,
        5_000,
        1_000,
    );

    let result = c.try_get_acceptance(&hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::AcceptanceNotFound)));
}

#[test]
fn test_get_award_not_found() {
    let (env, contract_id, _admin) = setup();
    let c = client(&env, &contract_id);
    let result = c.try_get_award(&hash(&env, 42));
    assert_eq!(result, Err(Ok(ContractError::AwardNotFound)));
}

#[test]
fn test_version() {
    let (env, contract_id, _admin) = setup();
    let c = client(&env, &contract_id);
    assert_eq!(c.version(), 1);
}
