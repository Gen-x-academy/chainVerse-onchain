use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CapstoneProjectClosuresContract, CapstoneProjectClosuresContractClient, ClosureOutcome,
    ClosureState, ContractError, Role,
};

const BASE: u64 = 1_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    requester: Address,
    coordinator: Address,
    coordinator_two: Address,
    outsider: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(CapstoneProjectClosuresContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    let requester = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let coordinator_two = Address::generate(&env);
    let outsider = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    c.initialize(&admin);
    c.set_role(&admin, &requester, &tenant, &project, &Role::Participant, &true);
    c.set_role(&admin, &coordinator, &tenant, &project, &Role::Coordinator, &true);
    c.set_role(&admin, &coordinator_two, &tenant, &project, &Role::Coordinator, &true);
    Fixture {
        env,
        contract_id,
        admin,
        requester,
        coordinator,
        coordinator_two,
        outsider,
        tenant,
        project,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> CapstoneProjectClosuresContractClient<'a> {
    CapstoneProjectClosuresContractClient::new(env, contract_id)
}

#[test]
fn initialize_and_roles_are_tenant_scoped() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
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
            &Role::Coordinator,
            &true
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    let other_tenant = hash(&f.env, 9);
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &other_tenant,
            &f.project,
            &hash(&f.env, 20),
            &ClosureOutcome::Completed,
            &hash(&f.env, 21),
            &2_000,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        c.try_request_closure(
            &f.outsider,
            &f.tenant,
            &f.project,
            &hash(&f.env, 22),
            &ClosureOutcome::Completed,
            &hash(&f.env, 23),
            &2_000,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(c.version(), 1);
}

#[test]
fn request_closure_replay_zero_hash_and_open_limit() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let id = hash(&f.env, 30);
    let reason = hash(&f.env, 31);
    let zero = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &id,
            &ClosureOutcome::Completed,
            &zero,
            &2_000,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ZeroHash
    );
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &id,
            &ClosureOutcome::Completed,
            &reason,
            &BASE,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidEffectiveAt
    );
    let first = c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &id,
        &ClosureOutcome::Completed,
        &reason,
        &2_000,
        &0,
    );
    let replay = c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &id,
        &ClosureOutcome::Completed,
        &reason,
        &2_000,
        &0,
    );
    assert_eq!(first, replay);
    assert_eq!(first.state, ClosureState::Requested);
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &id,
            &ClosureOutcome::Inactivity,
            &reason,
            &2_000,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ClosureAlreadyExists
    );
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &hash(&f.env, 32),
            &ClosureOutcome::SponsorWithdrawal,
            &reason,
            &2_000,
            &0
        )
        .unwrap_err()
        .unwrap(),
        ContractError::OpenClosureExists
    );
    assert_eq!(
        c.try_request_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &hash(&f.env, 34),
            &ClosureOutcome::MutualTermination,
            &reason,
            &2_000,
            &7
        )
        .unwrap_err()
        .unwrap(),
        ContractError::VersionMismatch
    );
}

#[test]
fn approval_requires_distinct_bounded_coordinators() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let id = hash(&f.env, 40);
    let reason = hash(&f.env, 41);
    c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &id,
        &ClosureOutcome::Completed,
        &reason,
        &2_000,
        &0,
    );
    c.set_role(
        &f.admin,
        &f.requester,
        &f.tenant,
        &f.project,
        &Role::Coordinator,
        &true,
    );
    assert_eq!(
        c.try_approve_closure(
            &f.requester,
            &f.tenant,
            &f.project,
            &id,
            &f.requester,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::SelfApproval
    );
    assert_eq!(
        c.try_approve_closure(
            &f.coordinator,
            &f.tenant,
            &f.project,
            &id,
            &f.outsider,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        c.try_approve_closure(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id,
            &f.outsider,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidRole
    );
    let approved = c.approve_closure(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id,
        &f.coordinator,
        &1,
    );
    assert_eq!(approved.state, ClosureState::Approved);
    let replay = c.approve_closure(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id,
        &f.coordinator,
        &1,
    );
    assert_eq!(replay, approved);
    assert_eq!(
        c.try_approve_closure(
            &f.coordinator_two,
            &f.tenant,
            &f.project,
            &id,
            &f.coordinator_two,
            &1
        )
        .unwrap_err()
        .unwrap(),
        ContractError::VersionMismatch
    );
    let second = c.approve_closure(
        &f.coordinator_two,
        &f.tenant,
        &f.project,
        &id,
        &f.coordinator_two,
        &2,
    );
    assert_eq!(second.approvers.len(), 2);
    let mut version = 3u32;
    for _byte in 50u8..=55u8 {
        let approver = Address::generate(&f.env);
        c.set_role(
            &f.admin,
            &approver,
            &f.tenant,
            &f.project,
            &Role::Coordinator,
            &true,
        );
        let updated = c.approve_closure(
            &approver,
            &f.tenant,
            &f.project,
            &id,
            &approver,
            &version,
        );
        assert_eq!(updated.state, ClosureState::Approved);
        version += 1;
    }
    let extra = Address::generate(&f.env);
    c.set_role(
        &f.admin,
        &extra,
        &f.tenant,
        &f.project,
        &Role::Coordinator,
        &true,
    );
    assert_eq!(
        c.try_approve_closure(
            &extra,
            &f.tenant,
            &f.project,
            &id,
            &extra,
            &version
        )
        .unwrap_err()
        .unwrap(),
        ContractError::ApprovalLimitReached
    );
}

#[test]
fn close_requires_approval_effective_time_and_fresh_version() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let id = hash(&f.env, 60);
    let reason = hash(&f.env, 61);
    c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &id,
        &ClosureOutcome::PolicyAction,
        &reason,
        &2_000,
        &0,
    );
    assert_eq!(
        c.try_close_closure(&f.requester, &f.tenant, &f.project, &id, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    c.approve_closure(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id,
        &f.coordinator,
        &1,
    );
    assert_eq!(
        c.try_close_closure(&f.requester, &f.tenant, &f.project, &id, &2)
            .unwrap_err()
            .unwrap(),
        ContractError::EffectiveAtNotReached
    );
    assert_eq!(
        c.try_close_closure(&f.requester, &f.tenant, &f.project, &id, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::VersionMismatch
    );
    f.env.ledger().set_timestamp(2_000);
    let closed = c.close_closure(&f.requester, &f.tenant, &f.project, &id, &2);
    assert_eq!(closed.state, ClosureState::Closed);
    let replay = c.close_closure(&f.requester, &f.tenant, &f.project, &id, &2);
    assert_eq!(replay, closed);
    assert_eq!(
        c.try_approve_closure(
            &f.coordinator_two,
            &f.tenant,
            &f.project,
            &id,
            &f.coordinator_two,
            &3
        )
        .unwrap_err()
        .unwrap(),
        ContractError::InvalidState
    );
    let next = c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &hash(&f.env, 62),
        &ClosureOutcome::Completed,
        &hash(&f.env, 63),
        &3_000,
        &0,
    );
    assert_eq!(next.state, ClosureState::Requested);
}

#[test]
fn rejection_and_cancellation_release_the_project_slot() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let first = hash(&f.env, 70);
    c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &first,
        &ClosureOutcome::Completed,
        &hash(&f.env, 71),
        &2_000,
        &0,
    );
    let rejected = c.reject_closure(&f.coordinator, &f.tenant, &f.project, &first, &1);
    assert_eq!(rejected.state, ClosureState::Rejected);
    let id = hash(&f.env, 72);
    c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &id,
        &ClosureOutcome::MutualTermination,
        &hash(&f.env, 73),
        &2_000,
        &0,
    );
    let cancelled = c.cancel_closure(&f.requester, &f.tenant, &f.project, &id, &1);
    assert_eq!(cancelled.state, ClosureState::Cancelled);
    assert_eq!(
        c.try_cancel_closure(&f.coordinator, &f.tenant, &f.project, &id, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    let reject_replay = c.reject_closure(&f.requester, &f.tenant, &f.project, &first, &1);
    assert_eq!(reject_replay, rejected);
    let replay = c.cancel_closure(&f.requester, &f.tenant, &f.project, &id, &1);
    assert_eq!(replay, cancelled);
    c.request_closure(
        &f.requester,
        &f.tenant,
        &f.project,
        &hash(&f.env, 74),
        &ClosureOutcome::Inactivity,
        &hash(&f.env, 75),
        &2_000,
        &0,
    );
    assert_eq!(
        c.try_reject_closure(&f.requester, &f.tenant, &f.project, &hash(&f.env, 74), &7)
            .unwrap_err()
            .unwrap(),
        ContractError::VersionMismatch
    );
}

#[test]
fn lifecycle_events_and_explicit_auth() {
    let env = Env::default();
    let contract_id = env.register(CapstoneProjectClosuresContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    assert!(c.try_initialize(&admin).is_err());
    assert_eq!(c.try_admin().unwrap_err().unwrap(), ContractError::NotInitialized);
    env.mock_all_auths();
    c.initialize(&admin);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    let requester = Address::generate(&env);
    let coordinator = Address::generate(&env);
    c.set_role(&admin, &requester, &tenant, &project, &Role::Participant, &true);
    c.set_role(&admin, &coordinator, &tenant, &project, &Role::Coordinator, &true);
    let id = hash(&env, 80);
    c.request_closure(
        &requester,
        &tenant,
        &project,
        &id,
        &ClosureOutcome::Completed,
        &hash(&env, 81),
        &2_000,
        &0,
    );
    let topic = |index: u32| -> Symbol {
        let (_, topics, _) = env.events().all().get(index).unwrap();
        Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap()
    };
    assert_eq!(topic(0), symbol_short!("CLREQ"));
    c.approve_closure(&coordinator, &tenant, &project, &id, &coordinator, &1);
    assert_eq!(topic(0), symbol_short!("CLAPP"));
    env.ledger().set_timestamp(2_000);
    c.close_closure(&requester, &tenant, &project, &id, &2);
    assert_eq!(topic(0), symbol_short!("CLCLS"));
    assert_eq!(c.version(), 1);
}
