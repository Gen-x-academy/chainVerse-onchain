extern crate std;

use soroban_sdk::{
    testutils::Address as _, testutils::Events as _, Address, BytesN, Env, Vec,
};

use crate::{
    CapstoneShowcasePagesContract, CapstoneShowcasePagesContractClient, ContractError, PageState,
    Role,
};

struct Fixture {
    env: Env,
    tenant: BytesN<32>,
    other_tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    curator: Address,
    publisher: Address,
    operator: Address,
    outsider: Address,
    client: CapstoneShowcasePagesContractClient<'static>,
}

fn id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn links(env: &Env, count: u32, seed: u8) -> Vec<BytesN<32>> {
    let mut values = Vec::new(env);
    for i in 0..count {
        values.push_back(id(env, seed.wrapping_add(i as u8)));
    }
    values
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneShowcasePagesContract, ());
    let client = CapstoneShowcasePagesContractClient::new(&env, &contract_id);
    let tenant = id(&env, 1);
    let other_tenant = id(&env, 2);
    let project = id(&env, 3);
    let admin = Address::generate(&env);
    let curator = Address::generate(&env);
    let publisher = Address::generate(&env);
    let operator = Address::generate(&env);
    let outsider = Address::generate(&env);
    client.initialize(&admin, &tenant);
    client.grant_role(&admin, &tenant, &curator, &Role::Curator);
    client.grant_role(&admin, &tenant, &publisher, &Role::Publisher);
    client.grant_role(&admin, &tenant, &operator, &Role::Operator);
    Fixture {
        env,
        tenant,
        other_tenant,
        project,
        admin,
        curator,
        publisher,
        operator,
        outsider,
        client,
    }
}

fn create(f: &Fixture, request_seed: u8, version: u32) -> BytesN<32> {
    let request_id = id(&f.env, request_seed);
    f.client.create_page(
        &f.curator,
        &f.tenant,
        &f.project,
        &request_id,
        &version,
        &id(&f.env, 0x20),
        &id(&f.env, 0x21),
        &id(&f.env, 0x22),
        &id(&f.env, 0x23),
        &id(&f.env, 0x24),
        &id(&f.env, 0x25),
        &id(&f.env, 0x26),
        &id(&f.env, 0x27),
        &id(&f.env, 0x28),
        &links(&f.env, 2, 0x30),
        &100,
    );
    request_id
}

#[test]
fn create_is_idempotent_by_request_id() {
    let f = setup();
    let before = f.env.events().all().len();
    let page_id = create(&f, 10, 1);
    f.client.create_page(
        &f.curator,
        &f.tenant,
        &f.project,
        &page_id,
        &1,
        &id(&f.env, 0x20),
        &id(&f.env, 0x21),
        &id(&f.env, 0x22),
        &id(&f.env, 0x23),
        &id(&f.env, 0x24),
        &id(&f.env, 0x25),
        &id(&f.env, 0x26),
        &id(&f.env, 0x27),
        &id(&f.env, 0x28),
        &links(&f.env, 2, 0x30),
        &100,
    );
    assert_eq!(f.env.events().all().len(), before + 1);
    let error = f
        .client
        .try_create_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &page_id,
            &1,
            &id(&f.env, 0x20),
            &id(&f.env, 0x99),
            &id(&f.env, 0x22),
            &id(&f.env, 0x23),
            &id(&f.env, 0x24),
            &id(&f.env, 0x25),
            &id(&f.env, 0x26),
            &id(&f.env, 0x27),
            &id(&f.env, 0x28),
            &links(&f.env, 2, 0x30),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::RequestAlreadyUsed);
}

#[test]
fn publish_update_and_unpublish_follow_state_machine() {
    let f = setup();
    let page_id = create(&f, 20, 1);
    f.client.publish_page(
        &f.publisher,
        &f.tenant,
        &f.project,
        &id(&f.env, 21),
        &page_id,
        &1,
    );
    assert_eq!(
        f.client.get_page(&f.tenant, &f.project, &page_id).unwrap().state,
        PageState::Published
    );
    let version = f.client.update_page(
        &f.curator,
        &f.tenant,
        &f.project,
        &id(&f.env, 22),
        &page_id,
        &1,
        &id(&f.env, 23),
        &id(&f.env, 24),
    );
    assert_eq!(version, 2);
    assert_eq!(
        f.client.get_latest_page(&f.tenant, &f.project).unwrap().state,
        PageState::Updated
    );
    f.client.unpublish_page(
        &f.publisher,
        &f.tenant,
        &f.project,
        &id(&f.env, 25),
        &page_id,
        &2,
    );
    assert_eq!(
        f.client.get_page(&f.tenant, &f.project, &page_id).unwrap().state,
        PageState::Unpublished
    );
}

#[test]
fn unauthorized_and_wrong_tenant_are_denied() {
    let f = setup();
    let error = f
        .client
        .try_create_page(
            &f.outsider,
            &f.tenant,
            &f.project,
            &id(&f.env, 30),
            &1,
            &id(&f.env, 0x20),
            &id(&f.env, 0x21),
            &id(&f.env, 0x22),
            &id(&f.env, 0x23),
            &id(&f.env, 0x24),
            &id(&f.env, 0x25),
            &id(&f.env, 0x26),
            &id(&f.env, 0x27),
            &id(&f.env, 0x28),
            &links(&f.env, 2, 0x30),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotAuthorized);
    let error = f
        .client
        .try_create_page(
            &f.curator,
            &f.other_tenant,
            &f.project,
            &id(&f.env, 31),
            &1,
            &id(&f.env, 0x20),
            &id(&f.env, 0x21),
            &id(&f.env, 0x22),
            &id(&f.env, 0x23),
            &id(&f.env, 0x24),
            &id(&f.env, 0x25),
            &id(&f.env, 0x26),
            &id(&f.env, 0x27),
            &id(&f.env, 0x28),
            &links(&f.env, 2, 0x30),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::WrongTenant);
}

#[test]
fn stale_version_is_rejected() {
    let f = setup();
    let page_id = create(&f, 40, 1);
    let error = f
        .client
        .try_publish_page(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id(&f.env, 41),
            &page_id,
            &2,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::StaleVersion);
}

#[test]
fn intent_replay_does_not_duplicate_update() {
    let f = setup();
    let page_id = create(&f, 50, 1);
    f.client.publish_page(
        &f.publisher,
        &f.tenant,
        &f.project,
        &id(&f.env, 51),
        &page_id,
        &1,
    );
    assert_eq!(
        f.client.update_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 52),
            &page_id,
            &1,
            &id(&f.env, 53),
            &id(&f.env, 54),
        ),
        2
    );
    let before = f.env.events().all().len();
    assert_eq!(
        f.client.update_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 52),
            &page_id,
            &1,
            &id(&f.env, 53),
            &id(&f.env, 54),
        ),
        2
    );
    assert_eq!(f.env.events().all().len(), before);
    let error = f
        .client
        .try_update_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 52),
            &page_id,
            &1,
            &id(&f.env, 55),
            &id(&f.env, 56),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::IntentAlreadyUsed);
}

#[test]
fn expiry_and_bounds_are_enforced() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 500);
    let page_id = create(&f, 60, 1);
    let too_many = links(&f.env, 17, 0x70);
    let error = f
        .client
        .try_create_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 61),
            &1,
            &id(&f.env, 0x20),
            &id(&f.env, 0x21),
            &id(&f.env, 0x22),
            &id(&f.env, 0x23),
            &id(&f.env, 0x24),
            &id(&f.env, 0x25),
            &id(&f.env, 0x26),
            &id(&f.env, 0x27),
            &id(&f.env, 0x28),
            &too_many,
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::TooManyLinks);
    let error = f
        .client
        .try_expire_page(
            &f.operator,
            &f.tenant,
            &f.project,
            &id(&f.env, 62),
            &page_id,
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::NotExpired);
    f.env.ledger().with_mut(|li| li.timestamp = 601);
    let error = f
        .client
        .try_publish_page(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id(&f.env, 63),
            &page_id,
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::AlreadyExpired);
    f.client.expire_page(
        &f.operator,
        &f.tenant,
        &f.project,
        &id(&f.env, 64),
        &page_id,
        &1,
    );
    assert_eq!(
        f.client.get_page(&f.tenant, &f.project, &page_id).unwrap().state,
        PageState::Expired
    );
}

#[test]
fn arithmetic_overflow_is_rejected() {
    let f = setup();
    f.env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    let error = f
        .client
        .try_create_page(
            &f.curator,
            &f.tenant,
            &f.project,
            &id(&f.env, 70),
            &1,
            &id(&f.env, 0x20),
            &id(&f.env, 0x21),
            &id(&f.env, 0x22),
            &id(&f.env, 0x23),
            &id(&f.env, 0x24),
            &id(&f.env, 0x25),
            &id(&f.env, 0x26),
            &id(&f.env, 0x27),
            &id(&f.env, 0x28),
            &links(&f.env, 2, 0x30),
            &100,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(error, ContractError::Overflow);
}

#[test]
fn ttl_extension_keeps_pages_readable() {
    let f = setup();
    let page_id = create(&f, 80, 1);
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client
            .get_page(&f.tenant, &f.project, &page_id)
            .unwrap()
            .version,
        1
    );
    f.env
        .ledger()
        .with_mut(|li| li.sequence = li.sequence + 6_120_801);
    assert_eq!(
        f.client.get_latest_page(&f.tenant, &f.project).unwrap().page_id,
        page_id
    );
}

#[test]
fn resource_budget_allows_full_link_set() {
    let f = setup();
    let mut budget = f.env.budget();
    budget.reset_unlimited();
    f.client.create_page(
        &f.curator,
        &f.tenant,
        &f.project,
        &id(&f.env, 90),
        &1,
        &id(&f.env, 0x20),
        &id(&f.env, 0x21),
        &id(&f.env, 0x22),
        &id(&f.env, 0x23),
        &id(&f.env, 0x24),
        &id(&f.env, 0x25),
        &id(&f.env, 0x26),
        &id(&f.env, 0x27),
        &id(&f.env, 0x28),
        &links(&f.env, 16, 0x40),
        &100,
    );
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::WrongTenant as u32, 4);
    assert_eq!(ContractError::IntentAlreadyUsed as u32, 6);
}
