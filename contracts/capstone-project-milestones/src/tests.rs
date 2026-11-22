use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec,
};

use crate::{
    CapstoneProjectMilestonesContract, CapstoneProjectMilestonesContractClient,
    CompletionRule, ContractError, CreateMilestoneInput, MilestoneState, Role,
};

const NOW: u64 = 1_700_000_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    CapstoneProjectMilestonesContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let id = env.register(CapstoneProjectMilestonesContract, ());
    let client = CapstoneProjectMilestonesContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let planner = Address::generate(&env);
    let reviewer = Address::generate(&env);
    client.initialize(&admin);
    client.set_role(
        &admin,
        &planner,
        &hash(&env, 1),
        &hash(&env, 2),
        &Role::Planner,
        &true,
    );
    (env, admin, planner, reviewer, client)
}

fn input(env: &Env, order: u32, reviewer: &Address, rule: CompletionRule) -> CreateMilestoneInput {
    CreateMilestoneInput {
        order,
        description_hash: hash(env, 0x60),
        due_at: 1,
        dependencies: Vec::new(env),
        reviewer: reviewer.clone(),
        deliverable_hashes: if rule == CompletionRule::AllDeliverables {
            Vec::from_array(env, [hash(env, 0x61)])
        } else {
            Vec::new(env)
        },
        completion_rule: rule,
        funding_ref: None,
        expected_version: 0,
    }
}

#[test]
fn mutations_are_auth_tenant_and_project_scoped() {
    let (env, _admin, planner, reviewer, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let outsider = Address::generate(&env);
    let err = client
        .try_create_milestone(
            &outsider,
            &tenant,
            &project,
            &hash(&env, 10),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = client
        .try_create_milestone(
            &planner,
            &hash(&env, 0x71),
            &project,
            &hash(&env, 10),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let first = client
        .create_milestone(
            &planner,
            &tenant,
            &project,
            &hash(&env, 10),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        ;
    assert_eq!(first.state, MilestoneState::Pending);

    let other_project = hash(&env, 3);
    client.set_role(
        &_admin,
        &planner,
        &tenant,
        &other_project,
        &Role::Planner,
        &true,
    );
    client
        .create_milestone(
            &planner,
            &tenant,
            &other_project,
            &hash(&env, 11),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        ;
    let mut cross = input(&env, 2, &reviewer, CompletionRule::Reviewer);
    cross.dependencies.push_back(hash(&env, 11));
    let err = client
        .try_create_milestone(&planner, &tenant, &project, &hash(&env, 12), &cross)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CrossProjectDependency);
}

#[test]
fn dependencies_gate_creation_and_reviewer_gates_completion() {
    let (env, _admin, planner, reviewer, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client
        .create_milestone(
            &planner,
            &tenant,
            &project,
            &hash(&env, 20),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        ;
    let mut dependent = input(&env, 2, &reviewer, CompletionRule::Reviewer);
    dependent.dependencies.push_back(hash(&env, 20));
    let err = client
        .try_create_milestone(&planner, &tenant, &project, &hash(&env, 21), &dependent)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::IncompleteDependency);

    let wrong_reviewer = Address::generate(&env);
    let err = client
        .try_complete_milestone(
            &wrong_reviewer,
            &tenant,
            &project,
            &hash(&env, 20),
            &Vec::new(&env),
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::WrongReviewer);
    let completed = client
        .complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 20),
            &Vec::new(&env),
            &1,
        )
        ;
    assert_eq!(completed.state, MilestoneState::Completed);
    assert_eq!(completed.due_at, 1);
    let second = client
        .create_milestone(&planner, &tenant, &project, &hash(&env, 21), &dependent)
        ;
    assert_eq!(second.dependencies.len(), 1);

    let deliverable = input(&env, 3, &reviewer, CompletionRule::AllDeliverables);
    client
        .create_milestone(&planner, &tenant, &project, &hash(&env, 22), &deliverable)
        ;
    let err = client
        .try_complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 22),
            &Vec::new(&env),
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::MissingEvidence);
    client
        .complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 22),
            &Vec::from_array(&env, [hash(&env, 0x61)]),
            &1,
        )
        ;
}

#[test]
fn replay_is_idempotent_and_stale_versions_rejected() {
    let (env, _admin, planner, reviewer, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let milestone_id = hash(&env, 30);
    let create = input(&env, 1, &reviewer, CompletionRule::Reviewer);
    let first = client
        .create_milestone(&planner, &tenant, &project, &milestone_id, &create)
        ;
    let replay = client
        .create_milestone(&planner, &tenant, &project, &milestone_id, &create)
        ;
    assert_eq!(first.expected_version, replay.expected_version);

    let err = client
        .try_complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &milestone_id,
            &Vec::new(&env),
            &4,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);
    let done = client
        .complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &milestone_id,
            &Vec::new(&env),
            &1,
        )
        ;
    let done_replay = client
        .complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &milestone_id,
            &Vec::new(&env),
            &1,
        )
        ;
    assert_eq!(done.completed_at, done_replay.completed_at);
    let err = client
        .try_complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &milestone_id,
            &Vec::from_array(&env, [hash(&env, 0x62)]),
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::VersionMismatch);
}

#[test]
fn cycles_bounds_events_and_version() {
    let (env, _admin, planner, reviewer, client) = setup();
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    assert_eq!(client.version(), 1);
    let mut self_cycle = input(&env, 1, &reviewer, CompletionRule::Reviewer);
    self_cycle.dependencies.push_back(hash(&env, 40));
    let err = client
        .try_create_milestone(&planner, &tenant, &project, &hash(&env, 40), &self_cycle)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::CycleDetected);

    client
        .create_milestone(
            &planner,
            &tenant,
            &project,
            &hash(&env, 41),
            &input(&env, 1, &reviewer, CompletionRule::ExternalEvidence),
        )
        ;
    let err = client
        .try_create_milestone(
            &planner,
            &tenant,
            &project,
            &hash(&env, 42),
            &input(&env, 1, &reviewer, CompletionRule::Reviewer),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::DuplicateOrder);

    let mut too_many = input(&env, 2, &reviewer, CompletionRule::Reviewer);
    for byte in 0x80u8..0x91u8 {
        too_many.dependencies.push_back(hash(&env, byte));
    }
    let err = client
        .try_create_milestone(&planner, &tenant, &project, &hash(&env, 43), &too_many)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::DependencyLimitReached);

    client
        .complete_milestone(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 41),
            &Vec::from_array(&env, [hash(&env, 0x63)]),
            &1,
        )
        ;
    let events = env.events().all();
    let mut found = false;
    for event in events.iter() {
        let (_, topics, _) = event;
        let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
        if topic == symbol_short!("CPM_DONE") {
            found = true;
        }
    }
    assert!(found);
    let err = client
        .try_get_milestone(&tenant, &project, &hash(&env, 0x99))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::MilestoneNotFound);
}
