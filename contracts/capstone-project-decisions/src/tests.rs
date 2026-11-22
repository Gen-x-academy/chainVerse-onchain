extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CapstoneProjectDecisionsContract, CapstoneProjectDecisionsContractClient, ContractError,
    DecisionState, Role,
};

struct Ctx {
    env: Env,
    admin: Address,
    coordinator: Address,
    approver: Address,
    second_approver: Address,
    viewer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
    client: CapstoneProjectDecisionsContractClient<'static>,
}

fn hash_u32(env: &Env, seed: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[28..].copy_from_slice(&seed.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_700_000_000);
    let contract_id = env.register(CapstoneProjectDecisionsContract, ());
    let client = CapstoneProjectDecisionsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let approver = Address::generate(&env);
    let second_approver = Address::generate(&env);
    let viewer = Address::generate(&env);
    client.initialize(&admin).unwrap();
    let tenant = hash_u32(&env, 1);
    let project = hash_u32(&env, 2);
    client
        .grant_role(&admin, &tenant, &coordinator, &Role::Coordinator)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &approver, &Role::Approver)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &second_approver, &Role::Approver)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &viewer, &Role::Viewer)
        .unwrap();
    Ctx {
        env,
        admin,
        coordinator,
        approver,
        second_approver,
        viewer,
        tenant,
        project,
        client,
    }
}

fn propose(
    ctx: &Ctx,
    decision_seed: u32,
    request_seed: u32,
    expected_version: u32,
    supersedes: Option<BytesN<32>>,
) -> u32 {
    ctx.client
        .propose(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, request_seed),
            &expected_version,
            &hash_u32(&ctx.env, decision_seed),
            &hash_u32(&ctx.env, decision_seed + 1_000),
            &hash_u32(&ctx.env, decision_seed + 2_000),
            &(ctx.env.ledger().timestamp() + 100),
            &1,
            &supersedes,
        )
        .unwrap()
}

#[test]
fn initialization_and_role_grants_are_stored_and_one_time() {
    let ctx = setup();
    assert_eq!(
        ctx.client.try_initialize(&ctx.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client
            .try_grant_role(&outsider, &ctx.tenant, &outsider, &Role::Viewer)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    let decision = hash_u32(&ctx.env, 10);
    propose(&ctx, 10, 10, 0, None);
    assert!(ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
        .is_ok());
}

#[test]
fn propose_approve_activate_lifecycle() {
    let ctx = setup();
    let decision = hash_u32(&ctx.env, 20);
    assert_eq!(propose(&ctx, 20, 20, 0, None), 1);
    let record = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
        .unwrap();
    assert_eq!(record.state, DecisionState::Proposed);
    assert_eq!(record.version, 1);
    assert_eq!(record.created_at, ctx.env.ledger().timestamp());

    let approve_version = ctx
        .client
        .approve(
            &ctx.approver,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 21),
            &1,
            &decision,
            &hash_u32(&ctx.env, 22),
        )
        .unwrap();
    assert_eq!(approve_version, 2);
    let record = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
        .unwrap();
    assert_eq!(record.state, DecisionState::Approved);
    assert_eq!(record.approvals.len(), 1);

    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = record.effective_at);
    let activate_version = ctx
        .client
        .activate(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 23),
            &2,
            &decision,
        )
        .unwrap();
    assert_eq!(activate_version, 3);
    let final_record = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
        .unwrap();
    assert_eq!(final_record.state, DecisionState::Effective);
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 3);
}

#[test]
fn stale_versions_and_unstored_roles_are_denied() {
    let ctx = setup();
    propose(&ctx, 30, 30, 0, None);
    assert_eq!(
        ctx.client
            .try_propose(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 31),
                &0,
                &hash_u32(&ctx.env, 32),
                &hash_u32(&ctx.env, 1_032),
                &hash_u32(&ctx.env, 2_032),
                &ctx.env.ledger().timestamp(),
                &1,
                &None,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::VersionConflict
    );
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client
            .try_propose(
                &outsider,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 33),
                &1,
                &hash_u32(&ctx.env, 34),
                &hash_u32(&ctx.env, 1_034),
                &hash_u32(&ctx.env, 2_034),
                &ctx.env.ledger().timestamp(),
                &1,
                &None,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn request_replay_is_idempotent_and_payload_conflicts_are_rejected() {
    let ctx = setup();
    let decision = hash_u32(&ctx.env, 40);
    let request = hash_u32(&ctx.env, 41);
    let version = ctx
        .client
        .propose(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &0,
            &decision,
            &hash_u32(&ctx.env, 1_040),
            &hash_u32(&ctx.env, 2_040),
            &ctx.env.ledger().timestamp(),
            &1,
            &None,
        )
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(ctx.env.events().all().len(), 1);

    let replay = ctx
        .client
        .propose(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &0,
            &decision,
            &hash_u32(&ctx.env, 1_040),
            &hash_u32(&ctx.env, 2_040),
            &ctx.env.ledger().timestamp(),
            &1,
            &None,
        )
        .unwrap();
    assert_eq!(replay, version);
    assert_eq!(ctx.env.events().all().len(), 0);
    assert_eq!(
        ctx.client
            .list_decisions(&ctx.viewer, &ctx.tenant, &ctx.project, &0, &10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 1);

    assert_eq!(
        ctx.client
            .try_propose(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &request,
                &0,
                &decision,
                &hash_u32(&ctx.env, 9_999),
                &hash_u32(&ctx.env, 2_040),
                &ctx.env.ledger().timestamp(),
                &1,
                &None,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
}

#[test]
fn approval_replay_and_duplicate_approval_are_rejected() {
    let ctx = setup();
    let decision = hash_u32(&ctx.env, 50);
    propose(&ctx, 50, 50, 0, None);
    let request = hash_u32(&ctx.env, 51);
    let approval_hash = hash_u32(&ctx.env, 52);
    let version = ctx
        .client
        .approve(
            &ctx.approver,
            &ctx.tenant,
            &ctx.project,
            &request,
            &1,
            &decision,
            &approval_hash,
        )
        .unwrap();
    assert_eq!(version, 2);

    let replay = ctx
        .client
        .approve(
            &ctx.approver,
            &ctx.tenant,
            &ctx.project,
            &request,
            &1,
            &decision,
            &approval_hash,
        )
        .unwrap();
    assert_eq!(replay, version);
    assert_eq!(ctx.env.events().all().len(), 0);

    assert_eq!(
        ctx.client
            .try_approve(
                &ctx.approver,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 53),
                &2,
                &decision,
                &hash_u32(&ctx.env, 54),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyApproved
    );
    let record = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
        .unwrap();
    assert_eq!(record.state, DecisionState::Approved);
}

#[test]
fn supersession_requires_an_effective_predecessor() {
    let ctx = setup();
    let first = hash_u32(&ctx.env, 60);
    propose(&ctx, 60, 60, 0, None);
    ctx.client
        .approve(
            &ctx.approver,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 61),
            &1,
            &first,
            &hash_u32(&ctx.env, 62),
        )
        .unwrap();
    let effective_at = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &first)
        .unwrap()
        .effective_at;
    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = effective_at);
    ctx.client
        .activate(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 63),
            &2,
            &first,
        )
        .unwrap();

    propose(&ctx, 70, 70, 3, Some(first.clone()));
    let prior = ctx
        .client
        .get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &first)
        .unwrap();
    assert_eq!(prior.state, DecisionState::Superseded);

    assert_eq!(
        ctx.client
            .try_propose(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 75),
                &4,
                &hash_u32(&ctx.env, 76),
                &hash_u32(&ctx.env, 1_076),
                &hash_u32(&ctx.env, 2_076),
                &ctx.env.ledger().timestamp(),
                &1,
                &Some(first),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidSupersession
    );
}

#[test]
fn activation_checks_effective_date_and_state() {
    let ctx = setup();
    let decision = hash_u32(&ctx.env, 80);
    let effective_at = ctx.env.ledger().timestamp() + 1_000;
    ctx.client
        .propose(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 81),
            &0,
            &decision,
            &hash_u32(&ctx.env, 1_080),
            &hash_u32(&ctx.env, 2_080),
            &effective_at,
            &1,
            &None,
        )
        .unwrap();
    ctx.client
        .approve(
            &ctx.approver,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 82),
            &1,
            &decision,
            &hash_u32(&ctx.env, 83),
        )
        .unwrap();
    assert_eq!(
        ctx.client
            .try_activate(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 84),
                &2,
                &decision,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotEffective
    );
    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = effective_at - 1);
    assert_eq!(
        ctx.client
            .try_activate(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 85),
                &2,
                &decision,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotEffective
    );
    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = effective_at);
    ctx.client
        .activate(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 86),
            &2,
            &decision,
        )
        .unwrap();
    assert_eq!(
        ctx.client
            .try_activate(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 87),
                &3,
                &decision,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
}

#[test]
fn reads_are_tenant_scoped_and_revocation_is_immediate() {
    let ctx = setup();
    let decision = hash_u32(&ctx.env, 90);
    propose(&ctx, 90, 90, 0, None);
    let other_tenant = hash_u32(&ctx.env, 9_090);
    assert_eq!(
        ctx.client
            .try_get_decision(
                &ctx.coordinator,
                &other_tenant,
                &ctx.project,
                &decision,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    ctx.client
        .revoke_role(&ctx.admin, &ctx.tenant, &ctx.viewer, &Role::Viewer)
        .unwrap();
    assert_eq!(
        ctx.client
            .try_get_decision(&ctx.viewer, &ctx.tenant, &ctx.project, &decision)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(ctx
        .client
        .get_decision(&ctx.admin, &ctx.tenant, &ctx.project, &decision)
        .is_ok());
}

#[test]
fn pages_are_bounded_and_decision_index_is_capped() {
    let ctx = setup();
    assert_eq!(
        ctx.client
            .try_list_decisions(&ctx.viewer, &ctx.tenant, &ctx.project, &0, &0)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
    assert_eq!(
        ctx.client
            .try_list_decisions(&ctx.viewer, &ctx.tenant, &ctx.project, &0, &65)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
    let mut expected_version = 0;
    for i in 0..64u32 {
        let version = propose(&ctx, 10_000 + i, 20_000 + i, expected_version, None);
        expected_version = version;
    }
    let page = ctx
        .client
        .list_decisions(&ctx.viewer, &ctx.tenant, &ctx.project, &60, &64)
        .unwrap();
    assert_eq!(page.len(), 4);
    assert_eq!(
        ctx.client
            .try_propose(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 30_000),
                &expected_version,
                &hash_u32(&ctx.env, 30_001),
                &hash_u32(&ctx.env, 30_002),
                &hash_u32(&ctx.env, 30_003),
                &ctx.env.ledger().timestamp(),
                &1,
                &None,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}

#[test]
fn events_use_stable_id_and_code_payloads() {
    let ctx = setup();
    propose(&ctx, 110, 110, 0, None);
    let events = ctx.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("DCPROP"));
    let payload: (BytesN<32>, BytesN<32>, BytesN<32>, u32) = data.try_into_val(&ctx.env).unwrap();
    assert_eq!(payload.0, ctx.tenant);
    assert_eq!(payload.1, ctx.project);
    assert_eq!(payload.2, hash_u32(&ctx.env, 110));
    assert_eq!(payload.3, 1);
}
