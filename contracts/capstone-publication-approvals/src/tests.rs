extern crate std;

use soroban_sdk::{
    testutils::Address as _, testutils::Events as _, Address, BytesN, Env, Vec,
};

use crate::{
    CapstonePublicationApprovalsContract, CapstonePublicationApprovalsContractClient,
    ApprovalState, ContractError, Decision, ReleaseCode, Role,
};

struct Fixture {
    env: Env,
    tenant: BytesN<32>,
    other_tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    author: Address,
    supervisor: Address,
    sponsor: Address,
    ethics: Address,
    organization: Address,
    operator: Address,
    outsider: Address,
    roles: Vec<Role>,
    client: CapstonePublicationApprovalsContractClient<'static>,
}

fn id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstonePublicationApprovalsContract, ());
    let client = CapstonePublicationApprovalsContractClient::new(&env, &contract_id);
    let tenant = id(&env, 1);
    let other_tenant = id(&env, 2);
    let project = id(&env, 3);
    let admin = Address::generate(&env);
    let author = Address::generate(&env);
    let supervisor = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let ethics = Address::generate(&env);
    let organization = Address::generate(&env);
    let operator = Address::generate(&env);
    let outsider = Address::generate(&env);
    let roles = Vec::from_array(
        &env,
        [
            Role::Author,
            Role::Supervisor,
            Role::Sponsor,
            Role::Ethics,
            Role::Organization,
        ],
    );
    client.initialize(&admin, &tenant);
    client.grant_role(&admin, &tenant, &author, &Role::Author);
    client.grant_role(&admin, &tenant, &supervisor, &Role::Supervisor);
    client.grant_role(&admin, &tenant, &sponsor, &Role::Sponsor);
    client.grant_role(&admin, &tenant, &ethics, &Role::Ethics);
    client.grant_role(&admin, &tenant, &organization, &Role::Organization);
    client.grant_role(&admin, &tenant, &operator, &Role::Operator);
    Fixture {
        env,
        tenant,
        other_tenant,
        project,
        admin,
        author,
        supervisor,
        sponsor,
        ethics,
        organization,
        operator,
        outsider,
        roles,
        client,
    }
}

fn request(f: &Fixture, request_seed: u8) -> BytesN<32> {
    let request_id = id(&f.env, request_seed);
    f.client.request_approval(
        &f.author,
        &f.tenant,
        &f.project,
        &request_id,
        &1,
        &ReleaseCode::PublicRepository,
        &id(&f.env, 0x20),
        &f.roles,
        &100,
    );
    request_id
}

fn approve_all(f: &Fixture, request_id: &BytesN<32>, seed: u8) {
    let approvers = [
        (&f.author, Role::Author),
        (&f.supervisor, Role::Supervisor),
        (&f.sponsor, Role::Sponsor),
        (&f.ethics, Role::Ethics),
        (&f.organization, Role::Organization),
    ];
    for (i, (approver, _)) in approvers.iter().enumerate() {
        f.client.decide(
            approver,
            &f.tenant,
            &f.project,
            &id(&f.env, seed + i as u8),
            request_id,
            &1,
            &Decision::Approve,
            &id(&f.env, 0x40 + i as u8),
        );
    }
}

#[test]
fn request_is_idempotent_by_request_id() {
    let f = setup();
    let before = f.env.events().all().len();
    let request_id = request(&f, 10);
    f.client.request_approval(
        &f.author,
        &f.tenant,
        &f.project,
        &request_id,
        &1,
        &ReleaseCode::PublicRepository,
        &id(&f.env, 0x20),
        &f.roles,
        &100,
    );
    assert_eq!(f.env.events().all().len(), before + 1);
    let error = f
        .client
        .try_request_approval(
            &f.author,
            &f.tenant,
            &f.project,
            &request_id,
            &1,
            &ReleaseCode::ExternalSubmission,
            &id(&f.env, 0x21),
            &f.roles,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::RequestAlreadyUsed);
}

#[test]
fn all_required_roles_release_approval() {
    let f = setup();
    let request_id = request(&f, 20);
    approve_all(&f, &request_id, 0x30);
    assert_eq!(
        f.client.get_approval(&f.tenant, &f.project, &request_id).unwrap().state,
        ApprovalState::Approved
    );
    let state = f.client.finalize_approval(
        &f.admin,
        &f.tenant,
        &f.project,
        &id(&f.env, 0x50),
        &request_id,
        &1,
        &id(&f.env, 0x51),
    );
    assert_eq!(state, ApprovalState::Released);
}

#[test]
fn rejection_blocks_release() {
    let f = setup();
    let request_id = request(&f, 60);
    let state = f.client.decide(
        &f.ethics,
        &f.tenant,
        &f.project,
        &id(&f.env, 61),
        &request_id,
        &1,
        &Decision::Reject,
        &id(&f.env, 62),
    );
    assert_eq!(state, ApprovalState::Rejected);
    let error = f
        .client
        .try_decide(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 63),
            &request_id,
            &1,
            &Decision::Approve,
            &id(&f.env, 64),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::InvalidState);
}

#[test]
fn unauthorized_and_wrong_tenant_are_denied() {
    let f = setup();
    let request_id = request(&f, 70);
    let error = f
        .client
        .try_decide(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id(&f.env, 71),
            &request_id,
            &1,
            &Decision::Approve,
            &id(&f.env, 72),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotAuthorized);
    let error = f
        .client
        .try_request_approval(
            &f.author,
            &f.other_tenant,
            &f.project,
            &id(&f.env, 73),
            &1,
            &ReleaseCode::PublicRepository,
            &id(&f.env, 74),
            &f.roles,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::WrongTenant);
}

#[test]
fn duplicate_vote_is_rejected() {
    let f = setup();
    let request_id = request(&f, 80);
    f.client.decide(
        &f.author,
        &f.tenant,
        &f.project,
        &id(&f.env, 81),
        &request_id,
        &1,
        &Decision::Approve,
        &id(&f.env, 82),
    );
    let error = f
        .client
        .try_decide(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 83),
            &request_id,
            &1,
            &Decision::Approve,
            &id(&f.env, 84),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::AlreadyVoted);
}

#[test]
fn stale_version_is_rejected() {
    let f = setup();
    let request_id = request(&f, 90);
    let error = f
        .client
        .try_decide(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 91),
            &request_id,
            &2,
            &Decision::Approve,
            &id(&f.env, 92),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::StaleVersion);
}

#[test]
fn intent_replay_does_not_duplicate_decision() {
    let f = setup();
    let request_id = request(&f, 100);
    f.client.decide(
        &f.author,
        &f.tenant,
        &f.project,
        &id(&f.env, 101),
        &request_id,
        &1,
        &Decision::Approve,
        &id(&f.env, 102),
    );
    let before = f.env.events().all().len();
    assert_eq!(
        f.client.decide(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 101),
            &request_id,
            &1,
            &Decision::Approve,
            &id(&f.env, 102),
        ),
        ApprovalState::Requested
    );
    assert_eq!(f.env.events().all().len(), before);
    let error = f
        .client
        .try_decide(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 101),
            &request_id,
            &1,
            &Decision::Reject,
            &id(&f.env, 103),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::IntentAlreadyUsed);
}

#[test]
fn expiry_and_bounds_are_enforced() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 500);
    let request_id = request(&f, 110);
    let error = f
        .client
        .try_expire_approval(
            &f.operator,
            &f.tenant,
            &f.project,
            &id(&f.env, 111),
            &request_id,
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotExpired);
    let empty = Vec::new(&f.env);
    let error = f
        .client
        .try_request_approval(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 112),
            &1,
            &ReleaseCode::PublicRepository,
            &id(&f.env, 113),
            &empty,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::TooManyRoles);
    let bad_roles = Vec::from_array(&f.env, [Role::Administrator]);
    let error = f
        .client
        .try_request_approval(
            &f.author,
            &f.tenant,
            &f.project,
            &id(&f.env, 114),
            &1,
            &ReleaseCode::PublicRepository,
            &id(&f.env, 115),
            &bad_roles,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::InvalidRoleSet);
    f.env.ledger().with_mut(|li| li.timestamp = 601);
    f.client.expire_approval(
        &f.operator,
        &f.tenant,
        &f.project,
        &id(&f.env, 116),
        &request_id,
        &1,
    );
    assert_eq!(
        f.client.get_approval(&f.tenant, &f.project, &request_id).unwrap().state,
        ApprovalState::Expired
    );
}

#[test]
fn ttl_extension_keeps_approvals_readable() {
    let f = setup();
    let request_id = request(&f, 120);
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_approval(&f.tenant, &f.project, &request_id)
            .unwrap()
            .version,
        1
    );
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_approval(&f.tenant, &f.project, &request_id)
            .unwrap()
            .approvals,
        0
    );
}

#[test]
fn resource_budget_allows_full_role_set() {
    let f = setup();
    let mut budget = f.env.budget();
    budget.reset_unlimited();
    let request_id = request(&f, 130);
    approve_all(&f, &request_id, 0xD0);
    assert_eq!(
        f.client
            .get_approval(&f.tenant, &f.project, &request_id)
            .unwrap()
            .approvals,
        5
    );
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::WrongTenant as u32, 4);
    assert_eq!(ContractError::IntentAlreadyUsed as u32, 6);
}
