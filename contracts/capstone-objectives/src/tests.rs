use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, Vec as SdkVec,
};

use crate::{
    CapstoneObjectivesContract, CapstoneObjectivesContractClient, ContractError, ObjectiveSpec,
    ObjectiveState, Role, Visibility,
};

const BASE: u64 = 1_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    owner: Address,
    coordinator: Address,
    outsider: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
    criteria: SdkVec<BytesN<32>>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(CapstoneObjectivesContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let outsider = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    c.initialize(&admin);
    c.set_role(&admin, &owner, &tenant, &project, &Role::Participant, &true);
    c.set_role(
        &admin,
        &coordinator,
        &tenant,
        &project,
        &Role::Coordinator,
        &true,
    );
    let criteria = SdkVec::from_array(&env, [hash(&env, 3), hash(&env, 4)]);
    Fixture {
        env,
        contract_id,
        admin,
        owner,
        coordinator,
        outsider,
        tenant,
        project,
        criteria,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> CapstoneObjectivesContractClient<'a> {
    CapstoneObjectivesContractClient::new(env, contract_id)
}

fn spec(
    env: &Env,
    owner: &Address,
    priority: u32,
    visibility: Visibility,
    baseline: u8,
    target: u8,
    date: u64,
    charter: u8,
    criteria: &SdkVec<BytesN<32>>,
) -> ObjectiveSpec {
    ObjectiveSpec {
        owner: owner.clone(),
        priority,
        visibility,
        baseline_hash: hash(env, baseline),
        target_hash: hash(env, target),
        target_date: date,
        charter_hash: hash(env, charter),
        criteria_hashes: criteria.clone(),
    }
}

#[test]
fn admin_tenant_and_revision_validation_boundaries() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let valid = spec(
        &f.env,
        &f.owner,
        1,
        Visibility::Public,
        11,
        12,
        2_000,
        13,
        &f.criteria,
    );
    assert_eq!(
        c.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        c.try_set_role(
            &f.outsider,
            &f.outsider,
            &f.tenant,
            &f.project,
            &Role::Participant,
            &true
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &hash(&f.env, 9),
            &f.project,
            &hash(&f.env, 10),
            &valid,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    let outsider_owner = ObjectiveSpec {
        owner: f.outsider.clone(),
        ..valid.clone()
    };
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 14),
            &outsider_owner,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    let mut zero_baseline = valid.clone();
    zero_baseline.baseline_hash = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 15),
            &zero_baseline,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    let mut zero_priority = valid.clone();
    zero_priority.priority = 0;
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 16),
            &zero_priority,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidPriority
    );
    let mut no_criteria = valid.clone();
    no_criteria.criteria_hashes = SdkVec::new(&f.env);
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 17),
            &no_criteria,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::NoCriteria
    );
    let mut zero_criterion = valid.clone();
    zero_criterion.criteria_hashes =
        SdkVec::from_array(&f.env, [BytesN::from_array(&f.env, &[0u8; 32])]);
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 18),
            &zero_criterion,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    assert_eq!(c.version(), 1);
}

#[test]
fn amendments_are_versioned_immutable_and_replay_safe() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let id = hash(&f.env, 40);
    let first = spec(
        &f.env,
        &f.owner,
        5,
        Visibility::Public,
        41,
        42,
        2_000,
        43,
        &f.criteria,
    );
    let created = c.create_objective(&f.owner, &f.tenant, &f.project, &id, &first, &0);
    let replay = c.create_objective(&f.owner, &f.tenant, &f.project, &id, &first, &0);
    assert_eq!(created, replay);
    assert_eq!(created.current.version, 1);
    assert_eq!(created.revisions.len(), 1);
    let changed = ObjectiveSpec {
        priority: 6,
        ..first.clone()
    };
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &changed,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ObjectiveAlreadyExists
    );
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 44),
            &first,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::VersionMismatch
    );
    assert_eq!(
        c.try_amend_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &1,
            &changed,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidVersion
    );
    let second = spec(
        &f.env,
        &f.owner,
        6,
        Visibility::Public,
        45,
        46,
        2_000,
        47,
        &f.criteria,
    );
    assert_eq!(
        c.try_amend_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &2,
            &second,
            &2
        )
        .unwrap_err()
        .unwrap(),
        ContractError::VersionMismatch
    );
    let amended =
        c.amend_objective(&f.owner, &f.tenant, &f.project, &id, &2, &second, &1);
    assert_eq!(amended.current.version, 2);
    assert_eq!(amended.revisions.len(), 2);
    assert_eq!(amended.revisions.get(0).unwrap().priority, 5);
    let replay =
        c.amend_objective(&f.owner, &f.tenant, &f.project, &id, &2, &second, &1);
    assert_eq!(replay, amended);
}

#[test]
fn state_transitions_and_closed_objectives_reject_updates() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let id = hash(&f.env, 50);
    let valid = spec(
        &f.env,
        &f.owner,
        1,
        Visibility::Public,
        51,
        52,
        2_000,
        53,
        &f.criteria,
    );
    c.create_objective(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id,
        &valid,
        &0,
    );
    assert_eq!(
        c.try_set_state(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id,
            &ObjectiveState::Active,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    assert_eq!(
        c.try_set_state(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &ObjectiveState::Active,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::VersionMismatch
    );
    let active = c.set_state(
        &f.owner,
        &f.tenant,
        &f.project,
        &id,
        &ObjectiveState::Active,
        &1,
    );
    assert_eq!(active.state, ObjectiveState::Active);
    let replay = c.set_state(
        &f.owner,
        &f.tenant,
        &f.project,
        &id,
        &ObjectiveState::Active,
        &1,
    );
    assert_eq!(replay, active);
    assert_eq!(
        c.try_set_state(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &ObjectiveState::Draft,
            &2
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidState
    );
    let closed = c.set_state(
        &f.owner,
        &f.tenant,
        &f.project,
        &id,
        &ObjectiveState::Closed,
        &2,
    );
    assert_eq!(closed.state, ObjectiveState::Closed);
    assert_eq!(
        c.try_set_state(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &ObjectiveState::Active,
            &3
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidState
    );
    let next = spec(
        &f.env,
        &f.owner,
        2,
        Visibility::Public,
        54,
        55,
        2_000,
        56,
        &f.criteria,
    );
    assert_eq!(
        c.try_amend_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &2,
            &next,
            &3
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidState
    );
}

#[test]
fn visibility_priority_ordering_bounds_and_events() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let create = |id: BytesN<32>, priority: u32, date: u64, visibility: Visibility| {
        c.create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &id,
            &spec(
                &f.env,
                &f.owner,
                priority,
                visibility,
                60,
                61,
                date,
                62,
                &f.criteria,
            ),
            &0,
        )
    };
    let first = create(hash(&f.env, 20), 2, 3_000, Visibility::Public);
    assert_eq!(first.current.version, 1);
    let topic = || -> Symbol {
        let (_, topics, _) = f.env.events().all().get(0).unwrap();
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap()
    };
    assert_eq!(topic(), symbol_short!("OBJCREATE"));
    create(hash(&f.env, 21), 1, 4_000, Visibility::Public);
    create(hash(&f.env, 22), 1, 2_000, Visibility::Restricted);
    assert_eq!(
        c.list_objective_ids(&f.owner, &f.tenant, &f.project),
        SdkVec::from_array(
            &f.env,
            [hash(&f.env, 22), hash(&f.env, 21), hash(&f.env, 20)]
        )
    );
    assert_eq!(
        c.try_get_objective(&f.outsider, &f.tenant, &f.project, &hash(&f.env, 22))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        c.get_objective(&f.coordinator, &f.tenant, &f.project, &hash(&f.env, 22))
            .current
            .visibility,
        Visibility::Restricted
    );
    assert_eq!(
        c.list_objective_ids(&f.outsider, &f.tenant, &f.project),
        SdkVec::from_array(&f.env, [hash(&f.env, 21), hash(&f.env, 20)])
    );
    let mut too_many = SdkVec::new(&f.env);
    for byte in 100u8..=116u8 {
        too_many.push_back(hash(&f.env, byte));
    }
    let mut excess = spec(
        &f.env,
        &f.owner,
        1,
        Visibility::Public,
        60,
        61,
        2_000,
        62,
        &too_many,
    );
    excess.criteria_hashes = too_many;
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 70),
            &excess,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::CriteriaLimitReached
    );
    for byte in 30u8..=90u8 {
        create(hash(&f.env, byte), 3, 5_000, Visibility::Public);
    }
    assert_eq!(
        c.try_create_objective(
            &f.owner,
            &f.tenant,
            &f.project,
            &hash(&f.env, 91),
            &spec(
                &f.env,
                &f.owner,
                1,
                Visibility::Public,
                60,
                61,
                2_000,
                62,
                &f.criteria
            ),
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ObjectiveLimitReached
    );
    assert_eq!(c.version(), 1);
}
