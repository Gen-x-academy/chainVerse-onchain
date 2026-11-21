extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    Aggregation, CapstoneReviewRubricsContract, CapstoneReviewRubricsContractClient, Criterion,
    ContractError, FieldVisibility, Role,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> (
    Env,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneReviewRubricsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let contract_id = env.register(CapstoneReviewRubricsContract, ());
    let client = CapstoneReviewRubricsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let editor = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &project, &editor, &Role::Editor);
    (env, editor, tenant, project, client)
}

fn criterion(env: &Env, seed: u8, weight: u32) -> Criterion {
    Criterion {
        criterion_id: hash(env, seed),
        weight,
        min_score: 0,
        max_score: 100,
        requires_comment: false,
        visibility: FieldVisibility::ReviewerOnly,
        guidance_hash: hash(env, seed + 80),
    }
}

#[test]
fn initialization_is_one_time_and_roles_are_scoped() {
    let (env, editor, tenant, project, client) = setup();
    let admin = Address::generate(&env);
    let other_tenant = hash(&env, 70);

    assert!(client.has_role(&editor, &tenant, &project, &Role::Editor));
    assert!(!client.has_role(&editor, &other_tenant, &project, &Role::Editor));
    assert_eq!(
        client.try_initialize(&admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    let outsider = Address::generate(&env);
    assert_eq!(
        client
            .try_grant_role(&outsider, &tenant, &project, &editor, &Role::Editor)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn unauthorized_caller_cannot_create_rubric() {
    let (env, _, tenant, project, client) = setup();
    let outsider = Address::generate(&env);

    assert_eq!(
        client
            .try_create_rubric(
                &outsider,
                &tenant,
                &project,
                &hash(&env, 3),
                &hash(&env, 4),
                &hash(&env, 5),
                &Aggregation::WeightedMean,
                &FieldVisibility::Public,
                &hash(&env, 6),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn create_is_idempotent_and_rejects_payload_conflicts() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 10);
    let request = hash(&env, 11);
    let payload = hash(&env, 12);
    let content = hash(&env, 13);

    let first = client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &request,
        &payload,
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &content,
    );
    let replay = client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &request,
        &payload,
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &content,
    );
    assert_eq!(first, 1);
    assert_eq!(replay, 1);

    let conflict = client.try_create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &request,
        &hash(&env, 99),
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &content,
    );
    assert_eq!(
        conflict.unwrap_err().unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(client.get_rubric(&tenant, &project, &rubric).unwrap().version, 1);
}

#[test]
fn add_criterion_enforces_versions_weights_and_transitions() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 20);

    client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 21),
        &hash(&env, 22),
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &hash(&env, 23),
    );

    let v2 = client.add_criterion(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 24),
        &hash(&env, 25),
        &1,
        &criterion(&env, 100, 40),
    );
    let v3 = client.add_criterion(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 26),
        &hash(&env, 27),
        &2,
        &criterion(&env, 101, 60),
    );
    assert_eq!(v2, 2);
    assert_eq!(v3, 3);

    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 28),
                &hash(&env, 29),
                &1,
                &criterion(&env, 102, 10),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 30),
                &hash(&env, 31),
                &3,
                &criterion(&env, 103, 10),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWeight
    );

    let published = client.publish_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 32),
        &hash(&env, 33),
        &3,
    );
    assert_eq!(published, 4);
    assert_eq!(
        client.get_rubric(&tenant, &project, &rubric).unwrap().version,
        4
    );
    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 34),
                &hash(&env, 35),
                &4,
                &criterion(&env, 104, 1),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
}

#[test]
fn publish_requires_at_least_one_criterion_and_full_weight() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 40);
    client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 41),
        &hash(&env, 42),
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &hash(&env, 43),
    );
    assert_eq!(
        client
            .try_publish_rubric(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 44),
                &hash(&env, 45),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::MissingPrerequisite
    );
    client.add_criterion(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 46),
        &hash(&env, 47),
        &1,
        &criterion(&env, 110, 40),
    );
    assert_eq!(
        client
            .try_publish_rubric(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 48),
                &hash(&env, 49),
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWeight
    );
}

#[test]
fn criterion_collection_is_bounded() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 50);
    client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 51),
        &hash(&env, 52),
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &hash(&env, 53),
    );
    let mut version = 1;
    let mut seed = 120;
    while version <= 16 {
        let next = client.add_criterion(
            &editor,
            &tenant,
            &project,
            &rubric,
            &hash(&env, 60 + version as u8),
            &hash(&env, 80 + version as u8),
            &version,
            &criterion(&env, seed, 6),
        );
        assert_eq!(next, version + 1);
        version = next;
        seed += 1;
    }
    assert_eq!(client.get_criteria(&tenant, &project, &rubric).len(), 16);
    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 200),
                &hash(&env, 201),
                &17,
                &criterion(&env, 150, 1),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}

#[test]
fn timestamps_are_recorded_and_archive_is_final() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 210);
    env.ledger().set_timestamp(500);
    client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 211),
        &hash(&env, 212),
        &Aggregation::WeightedMean,
        &FieldVisibility::Public,
        &hash(&env, 213),
    );
    env.ledger().set_timestamp(900);
    client.add_criterion(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 214),
        &hash(&env, 215),
        &1,
        &criterion(&env, 160, 100),
    );
    let stored = client.get_rubric(&tenant, &project, &rubric).unwrap();
    assert_eq!(stored.created_at, 500);
    assert_eq!(stored.updated_at, 900);

    client.publish_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 216),
        &hash(&env, 217),
        &2,
    );
    client.archive_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 218),
        &hash(&env, 219),
        &3,
    );
    assert_eq!(
        client
            .try_archive_rubric(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 220),
                &hash(&env, 221),
                &4,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
}

#[test]
fn invalid_criteria_are_rejected() {
    let (env, editor, tenant, project, client) = setup();
    let rubric = hash(&env, 230);
    client.create_rubric(
        &editor,
        &tenant,
        &project,
        &rubric,
        &hash(&env, 231),
        &hash(&env, 232),
        &Aggregation::Median,
        &FieldVisibility::Hidden,
        &hash(&env, 233),
    );
    let mut bad = criterion(&env, 170, 10);
    bad.min_score = 10;
    bad.max_score = 10;
    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 234),
                &hash(&env, 235),
                &1,
                &bad,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidRange
    );
    let mut bad_hash = criterion(&env, 171, 10);
    bad_hash.guidance_hash = BytesN::from_array(&env, &[0u8; 32]);
    assert_eq!(
        client
            .try_add_criterion(
                &editor,
                &tenant,
                &project,
                &rubric,
                &hash(&env, 236),
                &hash(&env, 237),
                &1,
                &bad_hash,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidHash
    );
}
