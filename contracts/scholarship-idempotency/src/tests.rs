extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, KeyStatus, Operation, ScholarshipIdempotencyContract,
    ScholarshipIdempotencyContractClient,
};

fn setup() -> (Env, Address, ScholarshipIdempotencyContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipIdempotencyContract, ());
    let client = ScholarshipIdempotencyContractClient::new(&env, &id);
    (env, id, client)
}

fn key(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn zero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

// ── initialization ────────────────────────────────────────────────────────

#[test]
fn test_double_initialize_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, ContractError::AlreadyInitialized);
}

// ── key creation ──────────────────────────────────────────────────────────

#[test]
fn test_create_key_produces_pending_record() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 1);
    let record = client.create_key(&actor, &Operation::Submission, &k, &0);
    assert_eq!(record.status, KeyStatus::Pending);
    assert_eq!(record.actor, actor);
}

#[test]
fn test_create_key_duplicate_returns_in_flight() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 1);
    client.create_key(&actor, &Operation::Payout, &k, &0);
    let err = client
        .try_create_key(&actor, &Operation::Payout, &k, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::KeyInFlight);
}

#[test]
fn test_ttl_too_long_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 9);
    // 90 days + 1 second.
    let too_long = 7_776_001u64;
    let err = client
        .try_create_key(&actor, &Operation::Decision, &k, &too_long)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::TtlTooLong);
}

// ── completion ────────────────────────────────────────────────────────────

#[test]
fn test_complete_success_marks_succeeded() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 2);
    client.create_key(&actor, &Operation::Decision, &k, &0);
    client.complete_success(&actor, &Operation::Decision, &k, &zero_hash(&env));
    assert!(client.is_succeeded(&actor, &Operation::Decision, &k));
}

#[test]
fn test_complete_failure_marks_failed() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 3);
    client.create_key(&actor, &Operation::Refund, &k, &0);
    client.complete_failure(&actor, &Operation::Refund, &k, &zero_hash(&env));
    let record = client.get_record(&actor, &Operation::Refund, &k);
    assert_eq!(record.status, KeyStatus::Failed);
}

#[test]
fn test_completed_key_is_immutable() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 4);
    client.create_key(&actor, &Operation::Acceptance, &k, &0);
    client.complete_success(&actor, &Operation::Acceptance, &k, &zero_hash(&env));
    // Second complete attempt should be rejected.
    let err = client
        .try_complete_success(&actor, &Operation::Acceptance, &k, &zero_hash(&env))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::KeyAlreadyCompleted);
}

#[test]
fn test_completed_key_create_returns_already_completed() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 5);
    client.create_key(&actor, &Operation::Milestone, &k, &0);
    client.complete_success(&actor, &Operation::Milestone, &k, &zero_hash(&env));
    let err = client
        .try_create_key(&actor, &Operation::Milestone, &k, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::KeyAlreadyCompleted);
}

// ── cross-actor isolation ─────────────────────────────────────────────────

#[test]
fn test_different_actors_same_key_bytes_independent() {
    // Two different actors using the same key bytes are completely independent.
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor_a = Address::generate(&env);
    let actor_b = Address::generate(&env);
    let k = key(&env, 6);

    client.create_key(&actor_a, &Operation::Notification, &k, &0);
    client.create_key(&actor_b, &Operation::Notification, &k, &0);

    // Completing A's key has no effect on B's.
    client.complete_success(&actor_a, &Operation::Notification, &k, &zero_hash(&env));
    assert!(client.is_in_flight(&actor_b, &Operation::Notification, &k));
}

#[test]
fn test_actor_cannot_complete_another_actors_key() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor_a = Address::generate(&env);
    let actor_b = Address::generate(&env);
    let k = key(&env, 7);

    client.create_key(&actor_a, &Operation::Submission, &k, &0);
    // actor_b tries to complete actor_a's key. Storage key includes actor_a
    // so it creates a new (not-found) record for actor_b.
    let err = client
        .try_complete_success(&actor_b, &Operation::Submission, &k, &zero_hash(&env))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::KeyNotFound);
}

// ── concurrent retry → one outcome ───────────────────────────────────────

#[test]
fn test_concurrent_retry_one_outcome() {
    // Soroban serializes invocations: the second complete_success is
    // rejected as KeyAlreadyCompleted, giving exactly one outcome.
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 8);
    client.create_key(&actor, &Operation::Payout, &k, &0);

    // First completion succeeds.
    client.complete_success(&actor, &Operation::Payout, &k, &zero_hash(&env));
    // Second (concurrent) completion is rejected.
    let err = client
        .try_complete_success(&actor, &Operation::Payout, &k, &zero_hash(&env))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::KeyAlreadyCompleted);
    // Record reflects exactly one outcome.
    let record = client.get_record(&actor, &Operation::Payout, &k);
    assert_eq!(record.status, KeyStatus::Succeeded);
}

// ── is_in_flight / is_succeeded lifecycle ────────────────────────────────

#[test]
fn test_lifecycle_states() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let actor = Address::generate(&env);
    let k = key(&env, 10);

    assert!(!client.is_in_flight(&actor, &Operation::Decision, &k));
    assert!(!client.is_succeeded(&actor, &Operation::Decision, &k));

    client.create_key(&actor, &Operation::Decision, &k, &0);
    assert!(client.is_in_flight(&actor, &Operation::Decision, &k));
    assert!(!client.is_succeeded(&actor, &Operation::Decision, &k));

    client.complete_success(&actor, &Operation::Decision, &k, &zero_hash(&env));
    assert!(!client.is_in_flight(&actor, &Operation::Decision, &k));
    assert!(client.is_succeeded(&actor, &Operation::Decision, &k));
}
