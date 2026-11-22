extern crate std;

use soroban_sdk::{
    testutils::Address as _, testutils::Events as _, Address, BytesN, Env,
};

use crate::{
    CapstoneIpTermsContract, CapstoneIpTermsContractClient, ContractError, IpTermsState, Role,
};

struct Fixture {
    env: Env,
    tenant: BytesN<32>,
    other_tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    manager: Address,
    participants: std::vec::Vec<Address>,
    operator: Address,
    outsider: Address,
    client: CapstoneIpTermsContractClient<'static>,
}

fn id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneIpTermsContract, ());
    let client = CapstoneIpTermsContractClient::new(&env, &contract_id);
    let tenant = id(&env, 1);
    let other_tenant = id(&env, 2);
    let project = id(&env, 3);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let participants = (0..8)
        .map(|_| Address::generate(&env))
        .collect::<std::vec::Vec<_>>();
    let operator = Address::generate(&env);
    let outsider = Address::generate(&env);
    client.initialize(&admin, &tenant);
    client.grant_role(&admin, &tenant, &manager, &Role::IpManager);
    for participant in &participants {
        client.grant_role(&admin, &tenant, participant, &Role::Participant);
    }
    client.grant_role(&admin, &tenant, &operator, &Role::Operator);
    Fixture {
        env,
        tenant,
        other_tenant,
        project,
        admin,
        manager,
        participants,
        operator,
        outsider,
        client,
    }
}

fn propose(f: &Fixture, request_seed: u8, version: u32, required: u32) -> BytesN<32> {
    let request_id = id(&f.env, request_seed);
    f.client.propose_terms(
        &f.manager,
        &f.tenant,
        &f.project,
        &request_id,
        &version,
        &1,
        &2,
        &3,
        &4,
        &5,
        &900,
        &id(&f.env, 0x20),
        &required,
        &100,
    );
    request_id
}

#[test]
fn propose_is_idempotent_by_request_id() {
    let f = setup();
    let before = f.env.events().all().len();
    let request_id = propose(&f, 10, 1, 1);
    f.client.propose_terms(
        &f.manager,
        &f.tenant,
        &f.project,
        &request_id,
        &1,
        &1,
        &2,
        &3,
        &4,
        &5,
        &900,
        &id(&f.env, 0x20),
        &1,
        &100,
    );
    assert_eq!(f.env.events().all().len(), before + 1);
    let error = f
        .client
        .try_propose_terms(
            &f.manager,
            &f.tenant,
            &f.project,
            &request_id,
            &1,
            &1,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 0x21),
            &1,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::RequestAlreadyUsed);
}

#[test]
fn acceptance_threshold_activates_terms() {
    let f = setup();
    let terms_id = propose(&f, 20, 1, 2);
    assert_eq!(
        f.client
            .accept_terms(
                &f.participants[0],
                &f.tenant,
                &f.project,
                &id(&f.env, 21),
                &terms_id,
                &1,
                &id(&f.env, 22),
            ),
        1
    );
    assert_eq!(
        f.client
            .accept_terms(
                &f.participants[1],
                &f.tenant,
                &f.project,
                &id(&f.env, 23),
                &terms_id,
                &1,
                &id(&f.env, 24),
            ),
        2
    );
    assert_eq!(
        f.client.get_terms(&f.tenant, &f.project, &terms_id).unwrap().state,
        IpTermsState::Accepted
    );
    f.client.activate_terms(
        &f.admin,
        &f.tenant,
        &f.project,
        &id(&f.env, 25),
        &terms_id,
        &1,
    );
    assert_eq!(
        f.client.get_latest_terms(&f.tenant, &f.project).unwrap().state,
        IpTermsState::Active
    );
}

#[test]
fn duplicate_and_excess_acceptances_are_rejected() {
    let f = setup();
    let terms_id = propose(&f, 30, 1, 1);
    f.client.accept_terms(
        &f.participants[0],
        &f.tenant,
        &f.project,
        &id(&f.env, 31),
        &terms_id,
        &1,
        &id(&f.env, 32),
    );
    let error = f
        .client
        .try_accept_terms(
            &f.participants[0],
            &f.tenant,
            &f.project,
            &id(&f.env, 33),
            &terms_id,
            &1,
            &id(&f.env, 34),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::AlreadyAccepted);
    let error = f
        .client
        .try_accept_terms(
            &f.participants[1],
            &f.tenant,
            &f.project,
            &id(&f.env, 35),
            &terms_id,
            &1,
            &id(&f.env, 36),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::AcceptanceLimit);
}

#[test]
fn unauthorized_and_wrong_tenant_are_denied() {
    let f = setup();
    let error = f
        .client
        .try_propose_terms(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id(&f.env, 40),
            &1,
            &1,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 41),
            &1,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotAuthorized);
    let error = f
        .client
        .try_propose_terms(
            &f.manager,
            &f.other_tenant,
            &f.project,
            &id(&f.env, 42),
            &1,
            &1,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 43),
            &1,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::WrongTenant);
}

#[test]
fn stale_version_is_rejected() {
    let f = setup();
    let terms_id = propose(&f, 50, 1, 1);
    let error = f
        .client
        .try_accept_terms(
            &f.participants[0],
            &f.tenant,
            &f.project,
            &id(&f.env, 51),
            &terms_id,
            &2,
            &id(&f.env, 52),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::StaleVersion);
}

#[test]
fn intent_replay_does_not_duplicate_acceptance() {
    let f = setup();
    let terms_id = propose(&f, 60, 1, 2);
    f.client.accept_terms(
        &f.participants[0],
        &f.tenant,
        &f.project,
        &id(&f.env, 61),
        &terms_id,
        &1,
        &id(&f.env, 62),
    );
    let before = f.env.events().all().len();
    assert_eq!(
        f.client.accept_terms(
            &f.participants[0],
            &f.tenant,
            &f.project,
            &id(&f.env, 61),
            &terms_id,
            &1,
            &id(&f.env, 62),
        ),
        1
    );
    assert_eq!(f.env.events().all().len(), before);
    let error = f
        .client
        .try_accept_terms(
            &f.participants[0],
            &f.tenant,
            &f.project,
            &id(&f.env, 61),
            &terms_id,
            &1,
            &id(&f.env, 63),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::IntentAlreadyUsed);
}

#[test]
fn embargo_update_and_expiry_use_ledger_time() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 500);
    let terms_id = propose(&f, 70, 1, 1);
    f.client.accept_terms(
        &f.participants[0],
        &f.tenant,
        &f.project,
        &id(&f.env, 71),
        &terms_id,
        &1,
        &id(&f.env, 72),
    );
    let error = f
        .client
        .try_expire_terms(
            &f.operator,
            &f.tenant,
            &f.project,
            &id(&f.env, 73),
            &terms_id,
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotExpired);
    f.client.set_embargo(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 74),
        &terms_id,
        &1,
        &900,
    );
    assert_eq!(
        f.client
            .get_terms(&f.tenant, &f.project, &terms_id)
            .unwrap()
            .embargo_until,
        900
    );
    f.env.ledger().with_mut(|li| li.timestamp = 601);
    f.client.expire_terms(
        &f.operator,
        &f.tenant,
        &f.project,
        &id(&f.env, 75),
        &terms_id,
        &1,
    );
    assert_eq!(
        f.client
            .get_terms(&f.tenant, &f.project, &terms_id)
            .unwrap()
            .state,
        IpTermsState::Expired
    );
}

#[test]
fn policy_bounds_and_checked_arithmetic_are_enforced() {
    let f = setup();
    let error = f
        .client
        .try_propose_terms(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 80),
            &1,
            &1,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 81),
            &9,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::InvalidPolicy);
    let error = f
        .client
        .try_propose_terms(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 82),
            &1,
            &0,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 83),
            &1,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::InvalidCode);
    f.env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    let error = f
        .client
        .try_propose_terms(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 84),
            &1,
            &1,
            &2,
            &3,
            &4,
            &5,
            &900,
            &id(&f.env, 85),
            &1,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::Overflow);
}

#[test]
fn ttl_extension_keeps_terms_readable() {
    let f = setup();
    let terms_id = propose(&f, 90, 1, 1);
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_terms(&f.tenant, &f.project, &terms_id)
            .unwrap()
            .terms_id,
        terms_id
    );
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client.get_latest_terms(&f.tenant, &f.project).unwrap().version,
        1
    );
}

#[test]
fn resource_budget_allows_full_acceptance_set() {
    let f = setup();
    let mut budget = f.env.budget();
    budget.reset_unlimited();
    let terms_id = propose(&f, 100, 1, 8);
    for (i, participant) in f.participants.iter().enumerate() {
        f.client.accept_terms(
            participant,
            &f.tenant,
            &f.project,
            &id(&f.env, 0xA0 + i as u8),
            &terms_id,
            &1,
            &id(&f.env, 0xB0 + i as u8),
        );
    }
    assert_eq!(
        f.client
            .get_terms(&f.tenant, &f.project, &terms_id)
            .unwrap()
            .acceptance_count,
        8
    );
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::WrongTenant as u32, 4);
    assert_eq!(ContractError::IntentAlreadyUsed as u32, 6);
}
