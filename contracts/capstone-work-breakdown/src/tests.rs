use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec,
};

use crate::{
    CapstoneWorkBreakdownContract, CapstoneWorkBreakdownContractClient, ContractError,
    CreateTaskInput, Role, StateTransitionInput, TaskState,
};

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    CapstoneWorkBreakdownContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let id = env.register(CapstoneWorkBreakdownContract, ());
    let client = CapstoneWorkBreakdownContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let planner = Address::generate(&env);
    let participant = Address::generate(&env);
    client.initialize(&admin);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.set_role(&admin, &planner, &tenant, &project, &Role::Planner, &true);
    client.set_role(
        &admin,
        &participant,
        &tenant,
        &project,
        &Role::Participant,
        &true,
    );
    (env, admin, planner, participant, client)
}

fn task_input(env: &Env, order: u32, assignee: &Address, estimate: u64) -> CreateTaskInput {
    CreateTaskInput {
        order,
        spec_hash: hash(env, 0x40),
        dependencies: Vec::new(env),
        estimate_seconds: estimate,
        acceptance_hash: hash(env, 0x41),
        assignee: assignee.clone(),
        planned_weights: Vec::from_array(env, [6_000u32, 4_000u32]),
        expected_version: 0,
    }
}

#[test]
fn mutations_are_auth_and_tenant_scoped() {
    let (env, admin, planner, participant, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let other_tenant = hash(&env, 0x11);
    let weights = Vec::from_array(&env, [6_000u32, 4_000u32]);
    let outsider = Address::generate(&env);
    let err = client
        .try_create_package(&outsider, &tenant, &project, &hash(&env, 3), &weights, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = client
        .try_create_package(
            &planner,
            &other_tenant,
            &project,
            &hash(&env, 3),
            &weights,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = client
        .try_set_role(
            &outsider,
            &participant,
            &tenant,
            &project,
            &Role::Participant,
            &true,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let package = client
        .create_package(&planner, &tenant, &project, &hash(&env, 3), &weights, &0)
        ;
    assert_eq!(package.expected_version, 1);
    assert_eq!(package.planned_weights, weights);
    assert_eq!(admin, admin);
}

#[test]
fn package_and_task_replay_are_idempotent_and_stale_versions_rejected() {
    let (env, _admin, planner, participant, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let package_id = hash(&env, 3);
    let weights = Vec::from_array(&env, [6_000u32, 4_000u32]);
    client
        .create_package(&planner, &tenant, &project, &package_id, &weights, &0)
        ;
    let replay = client
        .create_package(&planner, &tenant, &project, &package_id, &weights, &0)
        ;
    assert_eq!(replay.expected_version, 1);

    let task_id = hash(&env, 4);
    let input = task_input(&env, 1, &participant, 600);
    let task = client
        .create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &input,
        )
        ;
    assert_eq!(task.progress_weight_bps, 6_000);
    assert_eq!(task.state, TaskState::Planned);
    let task_replay = client
        .create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &input,
        )
        ;
    assert_eq!(task_replay, task);

    let mut stale = task_input(&env, 2, &participant, 300);
    stale.expected_version = 1;
    let err = client
        .try_create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &hash(&env, 5),
            &stale,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);

    let second = client
        .create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &hash(&env, 5),
            &task_input(&env, 2, &participant, 300),
        )
        ;
    assert_eq!(second.progress_weight_bps, 4_000);
    let package = client
        .get_package(&tenant, &project, &package_id)
        ;
    assert_eq!(package.task_count, 2);
}

#[test]
fn weights_orders_estimates_and_dependencies_are_bounded() {
    let (env, _admin, planner, participant, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let package_id = hash(&env, 3);
    let bad_weights = Vec::from_array(&env, [6_000u32, 3_999u32]);
    let err = client
        .try_create_package(
            &planner,
            &tenant,
            &project,
            &package_id,
            &bad_weights,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidWeight);
    let mut oversized = Vec::new(&env);
    for _ in 0..65u32 {
        oversized.push_back(1);
    }
    let err = client
        .try_create_package(
            &planner,
            &tenant,
            &project,
            &hash(&env, 0x22),
            &oversized,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidWeight);

    let weights = Vec::from_array(&env, [6_000u32, 4_000u32]);
    client
        .create_package(&planner, &tenant, &project, &package_id, &weights, &0)
        ;
    let mut input = task_input(&env, 2, &participant, 600);
    let err = client
        .try_create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &hash(&env, 4),
            &input,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidOrder);

    input.order = 1;
    input.estimate_seconds = 0;
    let err = client
        .try_create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &hash(&env, 4),
            &input,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidEstimate);

    let mut input = task_input(&env, 1, &participant, 600);
    for byte in 0x50u8..0x61u8 {
        input.dependencies.push_back(hash(&env, byte));
    }
    let err = client
        .try_create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &hash(&env, 4),
            &input,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::DependencyLimitReached);
}

#[test]
fn transitions_enforce_versions_terminal_states_and_emit_events() {
    let (env, _admin, planner, participant, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let package_id = hash(&env, 3);
    let task_id = hash(&env, 4);
    let weights = Vec::from_array(&env, [10_000u32]);
    client
        .create_package(&planner, &tenant, &project, &package_id, &weights, &0)
        ;
    let mut input = task_input(&env, 1, &participant, 600);
    input.planned_weights = weights;
    client
        .create_task(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &input,
        )
        ;

    let stale = StateTransitionInput {
        state: TaskState::InProgress,
        acceptance_hash: hash(&env, 0x42),
        assignee: participant.clone(),
        expected_version: 4,
    };
    let err = client
        .try_set_state(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &stale,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);

    let running = client
        .set_state(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &StateTransitionInput {
                state: TaskState::InProgress,
                acceptance_hash: hash(&env, 0x42),
                assignee: participant.clone(),
                expected_version: 1,
            },
        )
        ;
    assert_eq!(running.expected_version, 2);
    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, _) = event;
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic == symbol_short!("WBK_STATE") {
            found = true;
        }
    }
    assert!(found);

    let done = client
        .set_state(
            &participant,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &StateTransitionInput {
                state: TaskState::Done,
                acceptance_hash: hash(&env, 0x43),
                assignee: participant.clone(),
                expected_version: 2,
            },
        )
        ;
    assert_eq!(done.state, TaskState::Done);
    let replay = client
        .set_state(
            &participant,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &StateTransitionInput {
                state: TaskState::Done,
                acceptance_hash: hash(&env, 0x43),
                assignee: participant.clone(),
                expected_version: 2,
            },
        )
        ;
    assert_eq!(replay.expected_version, 3);
    let err = client
        .try_set_state(
            &planner,
            &tenant,
            &project,
            &package_id,
            &task_id,
            &StateTransitionInput {
                state: TaskState::Cancelled,
                acceptance_hash: hash(&env, 0x44),
                assignee: participant.clone(),
                expected_version: 3,
            },
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::TerminalState);
}

#[test]
fn version_and_missing_records_fail_closed() {
    let (env, _admin, _planner, _participant, client) = setup();
    assert_eq!(client.version(), 1);
    let err = client
        .try_get_package(&hash(&env, 1), &hash(&env, 2), &hash(&env, 0x99))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::PackageNotFound);
    let err = client
        .try_get_task(
            &hash(&env, 1),
            &hash(&env, 2),
            &hash(&env, 3),
            &hash(&env, 0x99),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::TaskNotFound);
}
