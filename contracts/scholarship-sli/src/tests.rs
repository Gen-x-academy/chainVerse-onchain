extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env,
};

use crate::{ContractError, ScholarshipSliContract, ScholarshipSliContractClient, SliMetric};

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    ScholarshipSliContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(200);
    let contract_id = env.register(ScholarshipSliContract, ());
    let client = ScholarshipSliContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recorder = Address::generate(&env);
    client.initialize(&admin, &recorder);
    (env, contract_id, admin, recorder, client)
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

#[test]
fn records_daily_operation_latency_and_failures_without_sample_ids_in_result() {
    let (env, _, _, recorder, client) = setup();
    let program = hash(&env, 1);
    let aggregate = client.record_sample(
        &recorder,
        &program,
        &SliMetric::Operation,
        &hash(&env, 2),
        &100,
        &160,
        &false,
        &0,
        &0,
    );
    assert_eq!(aggregate.sample_count, 1);
    assert_eq!(aggregate.failure_count, 1);
    assert_eq!(aggregate.duration_total_seconds, 60);
    assert_eq!(aggregate.duration_max_seconds, 60);
    assert_eq!(aggregate.day, 0);
    assert_eq!(
        client.get_aggregate(&program, &SliMetric::Operation, &0),
        aggregate
    );
}

#[test]
fn reconciliation_records_absolute_amount_drift() {
    let (env, _, _, recorder, client) = setup();
    let aggregate = client.record_sample(
        &recorder,
        &hash(&env, 3),
        &SliMetric::Reconciliation,
        &hash(&env, 4),
        &100,
        &150,
        &true,
        &1_000,
        &975,
    );
    assert_eq!(aggregate.reconciliation_count, 1);
    assert_eq!(aggregate.net_drift_total, -25);
    assert_eq!(aggregate.absolute_drift_total, 25);
    assert_eq!(aggregate.absolute_drift_max, 25);
}

#[test]
fn rejects_duplicate_samples_and_non_recorder_writes() {
    let (env, _, _, recorder, client) = setup();
    let program = hash(&env, 5);
    let sample = hash(&env, 6);
    client.record_sample(
        &recorder,
        &program,
        &SliMetric::QueueAge,
        &sample,
        &100,
        &110,
        &true,
        &0,
        &0,
    );
    assert_eq!(
        client.try_record_sample(
            &recorder,
            &program,
            &SliMetric::QueueAge,
            &sample,
            &100,
            &110,
            &true,
            &0,
            &0,
        ),
        Err(Ok(ContractError::DuplicateSample))
    );
    let other = Address::generate(&env);
    assert_eq!(
        client.try_record_sample(
            &other,
            &program,
            &SliMetric::QueueAge,
            &hash(&env, 7),
            &100,
            &110,
            &true,
            &0,
            &0,
        ),
        Err(Ok(ContractError::NotRecorder))
    );
}

#[test]
fn rejects_invalid_timestamps_and_amounts() {
    let (env, _, _, recorder, client) = setup();
    assert_eq!(
        client.try_record_sample(
            &recorder,
            &hash(&env, 8),
            &SliMetric::NotificationLag,
            &hash(&env, 9),
            &110,
            &100,
            &true,
            &0,
            &0,
        ),
        Err(Ok(ContractError::InvalidTimestamp))
    );
    assert_eq!(
        client.try_record_sample(
            &recorder,
            &hash(&env, 8),
            &SliMetric::Operation,
            &hash(&env, 10),
            &100,
            &110,
            &true,
            &10,
            &0,
        ),
        Err(Ok(ContractError::InvalidAmounts))
    );
}

#[test]
fn admin_can_rotate_recorder() {
    let (env, _, admin, recorder, client) = setup();
    let next = Address::generate(&env);
    client.set_recorder(&admin, &next);

    assert_eq!(
        client.try_record_sample(
            &recorder,
            &hash(&env, 11),
            &SliMetric::Operation,
            &hash(&env, 12),
            &100,
            &110,
            &true,
            &0,
            &0,
        ),
        Err(Ok(ContractError::NotRecorder))
    );
}
