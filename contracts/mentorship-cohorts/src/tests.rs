extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

use crate::{
    ContractError, EnrollmentStatus, MentorshipCohortsContract,
    MentorshipCohortsContractClient,
};

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    MentorshipCohortsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(100);
    let contract_id = env.register(MentorshipCohortsContract, ());
    let client = MentorshipCohortsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    client.initialize(&admin);
    (env, contract_id, admin, coordinator, client)
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn configure(
    env: &Env,
    client: &MentorshipCohortsContractClient,
    admin: &Address,
    coordinator: &Address,
    capacity: u32,
) -> BytesN<32> {
    let cohort_id = hash(env, 1);
    let mut coordinators = Vec::new(env);
    coordinators.push_back(coordinator.clone());
    client.create_cohort(
        admin,
        &hash(env, 2),
        &cohort_id,
        &50,
        &150,
        &160,
        &200,
        &capacity,
        &coordinators,
        &hash(env, 3),
        &hash(env, 4),
    );
    cohort_id
}

#[test]
fn create_and_enroll_respects_capacity_and_records_ledger_time() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 1);
    let participant = hash(&env, 10);

    let enrollment = client.enroll(&coordinator, &cohort_id, &participant);
    assert_eq!(enrollment.enrolled_at, 100);
    assert_eq!(enrollment.status, EnrollmentStatus::Active);
    assert_eq!(client.get_cohort(&cohort_id).active_count, 1);
    assert_eq!(
        client.try_enroll(&coordinator, &cohort_id, &hash(&env, 11)),
        Err(Ok(ContractError::CapacityReached))
    );
}

#[test]
fn only_configured_coordinators_can_manage_enrollment() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 2);
    let other = Address::generate(&env);

    assert_eq!(
        client.try_enroll(&other, &cohort_id, &hash(&env, 12)),
        Err(Ok(ContractError::NotCoordinator))
    );
}

#[test]
fn duplicate_enrollment_and_empty_commitments_are_rejected() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 2);
    let participant = hash(&env, 13);
    client.enroll(&coordinator, &cohort_id, &participant);

    assert_eq!(
        client.try_enroll(&coordinator, &cohort_id, &participant),
        Err(Ok(ContractError::AlreadyEnrolled))
    );
    assert_eq!(
        client.try_enroll(&coordinator, &cohort_id, &hash(&env, 0)),
        Err(Ok(ContractError::EmptyReference))
    );
}

#[test]
fn enrollment_is_limited_to_configured_window() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 2);
    env.ledger().set_timestamp(151);

    assert_eq!(
        client.try_enroll(&coordinator, &cohort_id, &hash(&env, 14)),
        Err(Ok(ContractError::EnrollmentClosed))
    );
}

#[test]
fn withdrawal_preserves_history_and_releases_capacity() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 1);
    let participant = hash(&env, 15);
    client.enroll(&coordinator, &cohort_id, &participant);
    env.ledger().set_timestamp(120);

    let withdrawn = client.withdraw(&coordinator, &cohort_id, &participant);
    assert_eq!(withdrawn.status, EnrollmentStatus::Withdrawn);
    assert_eq!(withdrawn.withdrawn_at, 120);
    assert_eq!(client.get_cohort(&cohort_id).active_count, 0);
    assert_eq!(
        client.try_enroll(&coordinator, &cohort_id, &participant),
        Err(Ok(ContractError::AlreadyEnrolled))
    );
}

#[test]
fn capacity_cannot_be_reduced_below_active_enrollment() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = configure(&env, &client, &admin, &coordinator, 2);
    client.enroll(&coordinator, &cohort_id, &hash(&env, 16));
    let coordinators = Vec::from_array(&env, [coordinator.clone()]);

    assert_eq!(
        client.try_update_cohort(
            &admin,
            &cohort_id,
            &50,
            &150,
            &160,
            &200,
            &0,
            &coordinators,
            &hash(&env, 3),
            &hash(&env, 4),
        ),
        Err(Ok(ContractError::InvalidCapacity))
    );
    let coordinators = Vec::from_array(&env, [coordinator]);
    assert_eq!(
        client.try_update_cohort(
            &admin,
            &cohort_id,
            &50,
            &150,
            &160,
            &200,
            &1,
            &coordinators,
            &hash(&env, 3),
            &hash(&env, 4),
        ),
        Err(Ok(ContractError::CapacityBelowEnrollment))
    );
}

#[test]
fn config_rejects_invalid_dates_and_duplicate_coordinators() {
    let (env, _, admin, coordinator, client) = setup();
    let cohort_id = hash(&env, 20);
    let mut coordinators = Vec::new(&env);
    coordinators.push_back(coordinator.clone());
    assert_eq!(
        client.try_create_cohort(
            &admin,
            &hash(&env, 2),
            &cohort_id,
            &100,
            &90,
            &110,
            &120,
            &2,
            &coordinators,
            &hash(&env, 3),
            &hash(&env, 4),
        ),
        Err(Ok(ContractError::InvalidDates))
    );

    coordinators.push_back(coordinator.clone());
    assert_eq!(
        client.try_create_cohort(
            &admin,
            &hash(&env, 2),
            &cohort_id,
            &50,
            &90,
            &100,
            &120,
            &2,
            &coordinators,
            &hash(&env, 3),
            &hash(&env, 4),
        ),
        Err(Ok(ContractError::InvalidCoordinators))
    );
}
