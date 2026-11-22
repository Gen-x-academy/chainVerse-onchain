use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec,
};

use crate::{
    CapstoneScheduleRiskContract, CapstoneScheduleRiskContractClient, ContractError, PlanInput,
    Role,
};

const NOW: u64 = 1_700_000_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    CapstoneScheduleRiskContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let id = env.register(CapstoneScheduleRiskContract, ());
    let client = CapstoneScheduleRiskContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let planner = Address::generate(&env);
    client.initialize(&admin);
    client.set_role(
        &admin,
        &planner,
        &hash(&env, 1),
        &hash(&env, 2),
        &Role::Planner,
        &true,
    );
    (env, admin, planner, client)
}

fn plan(env: &Env, deadline: u64, durations: [u64; 3]) -> PlanInput {
    PlanInput {
        node_ids: Vec::from_array(env, [hash(env, 10), hash(env, 11), hash(env, 12)]),
        edges: Vec::from_array(env, [
            (hash(env, 10), hash(env, 11)),
            (hash(env, 11), hash(env, 12)),
        ]),
        durations: Vec::from_array(env, durations),
        resource_hashes: Vec::from_array(env, [hash(env, 20), hash(env, 20), hash(env, 21)]),
        deadline,
        expected_version: 0,
    }
}

#[test]
fn submit_plan_requires_planner_role_for_tenant_and_project() {
    let (env, _admin, planner, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let outsider = Address::generate(&env);
    let input = plan(&env, NOW + 100, [10, 20, 30]);
    let err = client
        .try_submit_plan(
            &outsider,
            &tenant,
            &project,
            &hash(&env, 30),
            &input,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = client
        .try_submit_plan(
            &planner,
            &hash(&env, 0x31),
            &project,
            &hash(&env, 30),
            &input,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let analysis = client
        .submit_plan(&planner, &tenant, &project, &hash(&env, 30), &input)
        ;
    assert_eq!(analysis.node_count, 3);
    assert_eq!(analysis.edge_count, 2);
    assert_eq!(analysis.longest_path_seconds, 60);
}

#[test]
fn replay_is_idempotent_and_stale_versions_rejected() {
    let (env, _admin, planner, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let plan_id = hash(&env, 30);
    let input = plan(&env, NOW + 100, [10, 20, 30]);
    let first = client
        .submit_plan(&planner, &tenant, &project, &plan_id, &input)
        ;
    let replay = client
        .submit_plan(&planner, &tenant, &project, &plan_id, &input)
        ;
    assert_eq!(first.plan_hash, replay.plan_hash);
    assert_eq!(replay.expected_version, 1);

    let mut stale = plan(&env, NOW + 100, [10, 20, 31]);
    stale.expected_version = 0;
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &plan_id, &stale)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);
    let mut conflict = plan(&env, NOW + 100, [10, 20, 31]);
    conflict.expected_version = 1;
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &plan_id, &conflict)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::PlanExists);
}

#[test]
fn analysis_is_deterministic_and_flags_risk() {
    let (env, _admin, planner, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let first = client
        .submit_plan(
            &planner,
            &tenant,
            &project,
            &hash(&env, 30),
            &plan(&env, NOW + 100, [10, 20, 30]),
        )
        ;
    assert_eq!(first.blocked_count, 2);
    assert_eq!(first.resource_conflict_count, 1);
    assert!(first.resource_conflict_flag);
    assert!(!first.impossible_flag);
    assert!(!first.deadline_risk);
    assert_eq!(first.critical_path_node_count, 1);

    let risky = client
        .submit_plan(
            &planner,
            &tenant,
            &project,
            &hash(&env, 31),
            &plan(&env, NOW + 50, [10, 20, 30]),
        )
        ;
    assert!(risky.impossible_flag);
    assert!(risky.deadline_risk);
    assert!(risky.impossible_count > 0);
    assert_eq!(risky.available_seconds, 50);
    let stored = client
        .get_analysis(&tenant, &project, &hash(&env, 31))
        ;
    assert_eq!(stored.plan_hash, risky.plan_hash);
}

#[test]
fn invalid_graphs_and_bounds_are_rejected() {
    let (env, _admin, planner, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let mut input = plan(&env, NOW + 100, [10, 20, 30]);
    input.edges.push_back((hash(&env, 10), hash(&env, 10)));
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &hash(&env, 40), &input)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::SelfLoop);

    let mut input = plan(&env, NOW + 100, [10, 20, 30]);
    input.edges = Vec::from_array(&env, [
        (hash(&env, 10), hash(&env, 11)),
        (hash(&env, 11), hash(&env, 12)),
        (hash(&env, 12), hash(&env, 10)),
    ]);
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &hash(&env, 41), &input)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CycleDetected);

    let mut input = plan(&env, NOW + 100, [10, 20, 30]);
    input.durations.set(0, 0);
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &hash(&env, 42), &input)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidDuration);

    let mut input = plan(&env, NOW + 100, [10, 20, 30]);
    input.node_ids = Vec::new(&env);
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &hash(&env, 43), &input)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidBounds);

    let input = plan(&env, NOW, [10, 20, 30]);
    let err = client
        .try_submit_plan(&planner, &tenant, &project, &hash(&env, 44), &input)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidDeadline);
}

#[test]
fn events_version_and_missing_analysis() {
    let (env, _admin, planner, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    assert_eq!(client.version(), 1);
    let err = client
        .try_get_analysis(&tenant, &project, &hash(&env, 0x99))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::PlanNotFound);
    client
        .submit_plan(
            &planner,
            &tenant,
            &project,
            &hash(&env, 30),
            &plan(&env, NOW + 100, [10, 20, 30]),
        )
        ;
    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, _) = event;
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic == symbol_short!("SKR_PLAN") {
            found = true;
        }
    }
    assert!(found);
}
