extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    CalibrationPolicy, CapstoneReviewOutcomesContract, CapstoneReviewOutcomesContractClient,
    ContractError, MissingPolicy, OutcomeResult, OutcomeStatus, Role, TieTreatment,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneReviewOutcomesContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(4_000);
    let contract_id = env.register(CapstoneReviewOutcomesContract, ());
    let client = CapstoneReviewOutcomesContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &project, &coordinator, &Role::Coordinator);
    (env, admin, coordinator, tenant, project, client)
}

fn create(
    client: &CapstoneReviewOutcomesContractClient<'_>,
    coordinator: &Address,
    env: &Env,
    outcome: &BytesN<32>,
    request: &BytesN<32>,
    payload: &BytesN<32>,
    quorum: u32,
    missing: MissingPolicy,
    calibration: CalibrationPolicy,
    tie: TieTreatment,
) -> u32 {
    client.create_outcome(
        coordinator,
        &hash(env, 1),
        &hash(env, 2),
        outcome,
        request,
        payload,
        &0,
        &hash(env, 3),
        &1,
        &hash(env, 4),
        &1,
        &quorum,
        &missing,
        &calibration,
        &tie,
        &hash(env, 5),
    )
}

#[test]
fn initialization_is_one_time_and_unauthorized_cannot_create() {
    let (env, admin, coordinator, tenant, project, client) = setup();
    let outsider = Address::generate(&env);

    assert!(client.has_role(&coordinator, &tenant, &project, &Role::Coordinator));
    assert!(!client.has_role(&outsider, &tenant, &project, &Role::Coordinator));
    assert_eq!(
        client.try_initialize(&admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        client
            .try_create_outcome(
                &outsider,
                &tenant,
                &project,
                &hash(&env, 10),
                &hash(&env, 11),
                &hash(&env, 12),
                &0,
                &hash(&env, 13),
                &1,
                &hash(&env, 14),
                &1,
                &2,
                &MissingPolicy::Exclude,
                &CalibrationPolicy::Disabled,
                &TieTreatment::Escalate,
                &hash(&env, 15),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn create_is_idempotent_and_validates_policy_inputs() {
    let (env, _, coordinator, _, _, client) = setup();
    let first = create(
        &client,
        &coordinator,
        &env,
        &hash(&env, 20),
        &hash(&env, 21),
        &hash(&env, 22),
        2,
        MissingPolicy::Exclude,
        CalibrationPolicy::Disabled,
        TieTreatment::Escalate,
    );
    let replay = create(
        &client,
        &coordinator,
        &env,
        &hash(&env, 20),
        &hash(&env, 21),
        &hash(&env, 22),
        2,
        MissingPolicy::Exclude,
        CalibrationPolicy::Disabled,
        TieTreatment::Escalate,
    );
    assert_eq!(first, 1);
    assert_eq!(replay, 1);

    assert_eq!(
        client.try_create_outcome(
            &coordinator,
            &hash(&env, 1),
            &hash(&env, 2),
            &hash(&env, 20),
            &hash(&env, 21),
            &hash(&env, 99),
            &0,
            &hash(&env, 3),
            &1,
            &hash(&env, 4),
            &1,
            &2,
            &MissingPolicy::Exclude,
            &CalibrationPolicy::Disabled,
            &TieTreatment::Escalate,
            &hash(&env, 5),
        ).unwrap_err().unwrap(),
        ContractError::RequestConflict
    );

    assert_eq!(
        client.try_create_outcome(
            &coordinator,
            &hash(&env, 1),
            &hash(&env, 2),
            &hash(&env, 23),
            &hash(&env, 24),
            &hash(&env, 25),
            &0,
            &hash(&env, 3),
            &1,
            &hash(&env, 4),
            &1,
            &0,
            &MissingPolicy::Exclude,
            &CalibrationPolicy::Disabled,
            &TieTreatment::Escalate,
            &hash(&env, 5),
        ).unwrap_err().unwrap(),
        ContractError::InvalidQuorum
    );
}

#[test]
fn scores_are_deduplicated_and_finalization_is_immutable() {
    let (env, _, coordinator, tenant, project, client) = setup();
    let outcome = hash(&env, 30);
    create(
        &client,
        &coordinator,
        &env,
        &outcome,
        &hash(&env, 31),
        &hash(&env, 32),
        2,
        MissingPolicy::Exclude,
        CalibrationPolicy::Disabled,
        TieTreatment::Escalate,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 33),
        &hash(&env, 34),
        &1,
        &hash(&env, 35),
        &80,
        &100,
    );
    let replay = client.record_score(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 33),
        &hash(&env, 34),
        &1,
        &hash(&env, 35),
        &80,
        &100,
    );
    assert_eq!(replay, 2);
    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 36),
                &hash(&env, 37),
                &2,
                &hash(&env, 35),
                &10,
                &100,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateContribution
    );
    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 38),
                &hash(&env, 39),
                &2,
                &hash(&env, 40),
                &101,
                &100,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidScore
    );
    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 41),
                &hash(&env, 42),
                &2,
                &hash(&env, 43),
                &50,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWeight
    );

    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 44),
        &hash(&env, 45),
        &2,
        &hash(&env, 46),
        &40,
        &100,
    );
    env.ledger().set_timestamp(6_000);
    let result = client.finalize(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 47),
        &hash(&env, 48),
        &3,
        &hash(&env, 49),
    );
    assert_eq!(result, OutcomeResult::Accepted);
    let stored = client.get_outcome(&tenant, &project, &outcome).unwrap();
    assert_eq!(stored.status, OutcomeStatus::Finalized);
    assert_eq!(stored.final_score, 60);
    assert_eq!(stored.finalized_at, 6_000);
    assert_eq!(
        client
            .get_contribution(&tenant, &project, &outcome, &hash(&env, 35))
            .unwrap()
            .score,
        80
    );
    assert_eq!(client.get_contribution_ids(&tenant, &project, &outcome).len(), 2);

    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 50),
                &hash(&env, 51),
                &4,
                &hash(&env, 52),
                &10,
                &100,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyFinalized
    );
    assert_eq!(
        client
            .try_finalize(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 53),
                &hash(&env, 54),
                &4,
                &hash(&env, 55),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyFinalized
    );
    let replay_finalize = client.finalize(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 47),
        &hash(&env, 48),
        &3,
        &hash(&env, 49),
    );
    assert_eq!(replay_finalize, OutcomeResult::Accepted);
}

#[test]
fn tie_treatment_is_explicit_and_deterministic() {
    let (env, _, coordinator, tenant, project, client) = setup();
    let outcome = hash(&env, 60);
    create(
        &client,
        &coordinator,
        &env,
        &outcome,
        &hash(&env, 61),
        &hash(&env, 62),
        2,
        MissingPolicy::Exclude,
        CalibrationPolicy::Disabled,
        TieTreatment::Escalate,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 63),
        &hash(&env, 64),
        &1,
        &hash(&env, 65),
        &30,
        &100,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 66),
        &hash(&env, 67),
        &2,
        &hash(&env, 68),
        &70,
        &100,
    );
    let result = client.finalize(
        &coordinator,
        &tenant,
        &project,
        &outcome,
        &hash(&env, 69),
        &hash(&env, 70),
        &3,
        &hash(&env, 71),
    );
    assert_eq!(result, OutcomeResult::Escalated);
    assert_eq!(
        client.get_outcome(&tenant, &project, &outcome).unwrap().final_score,
        50
    );
}

#[test]
fn missing_policies_and_calibration_are_explainable() {
    let (env, _, coordinator, tenant, project, client) = setup();

    let missing = hash(&env, 80);
    create(
        &client,
        &coordinator,
        &env,
        &missing,
        &hash(&env, 81),
        &hash(&env, 82),
        3,
        MissingPolicy::CountAsZero,
        CalibrationPolicy::Disabled,
        TieTreatment::Reject,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &missing,
        &hash(&env, 83),
        &hash(&env, 84),
        &1,
        &hash(&env, 85),
        &90,
        &100,
    );
    assert_eq!(
        client.finalize(
            &coordinator,
            &tenant,
            &project,
            &missing,
            &hash(&env, 86),
            &hash(&env, 87),
            &2,
            &hash(&env, 88),
        ),
        OutcomeResult::Rejected
    );
    assert_eq!(
        client.get_outcome(&tenant, &project, &missing).unwrap().final_score,
        30
    );

    let blocked = hash(&env, 90);
    create(
        &client,
        &coordinator,
        &env,
        &blocked,
        &hash(&env, 91),
        &hash(&env, 92),
        2,
        MissingPolicy::Block,
        CalibrationPolicy::Disabled,
        TieTreatment::Reject,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &blocked,
        &hash(&env, 93),
        &hash(&env, 94),
        &1,
        &hash(&env, 95),
        &100,
        &100,
    );
    assert_eq!(
        client
            .try_finalize(
                &coordinator,
                &tenant,
                &project,
                &blocked,
                &hash(&env, 96),
                &hash(&env, 97),
                &2,
                &hash(&env, 98),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::QuorumNotMet
    );

    let calibrated = hash(&env, 100);
    create(
        &client,
        &coordinator,
        &env,
        &calibrated,
        &hash(&env, 101),
        &hash(&env, 102),
        2,
        MissingPolicy::Exclude,
        CalibrationPolicy::LinearRescale,
        TieTreatment::Reject,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &calibrated,
        &hash(&env, 103),
        &hash(&env, 104),
        &1,
        &hash(&env, 105),
        &60,
        &100,
    );
    client.record_score(
        &coordinator,
        &tenant,
        &project,
        &calibrated,
        &hash(&env, 106),
        &hash(&env, 107),
        &2,
        &hash(&env, 108),
        &100,
        &20,
    );
    assert_eq!(
        client.finalize(
            &coordinator,
            &tenant,
            &project,
            &calibrated,
            &hash(&env, 109),
            &hash(&env, 110),
            &3,
            &hash(&env, 111),
        ),
        OutcomeResult::Rejected
    );
    assert_eq!(
        client
            .get_outcome(&tenant, &project, &calibrated)
            .unwrap()
            .final_score,
        15
    );
}

#[test]
fn contributions_are_bounded_and_versions_are_enforced() {
    let (env, _, coordinator, tenant, project, client) = setup();
    let outcome = hash(&env, 120);
    create(
        &client,
        &coordinator,
        &env,
        &outcome,
        &hash(&env, 121),
        &hash(&env, 122),
        64,
        MissingPolicy::Exclude,
        CalibrationPolicy::PriorAnchor,
        TieTreatment::Accept,
    );
    let mut version = 1u32;
    let mut i = 1u8;
    while i <= 64 {
        let next = client.record_score(
            &coordinator,
            &tenant,
            &project,
            &outcome,
            &hash(&env, 130 + i),
            &hash(&env, 190 + i),
            &version,
            &hash(&env, 100 + i),
            &70,
            &10,
        );
        version = next;
        i += 1;
    }
    assert_eq!(client.get_contribution_ids(&tenant, &project, &outcome).len(), 64);
    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 240),
                &hash(&env, 241),
                &version,
                &hash(&env, 242),
                &70,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
    assert_eq!(
        client
            .try_record_score(
                &coordinator,
                &tenant,
                &project,
                &outcome,
                &hash(&env, 243),
                &hash(&env, 244),
                &1,
                &hash(&env, 245),
                &70,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
}
