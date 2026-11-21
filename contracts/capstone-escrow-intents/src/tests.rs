#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    symbol_short, Address, BytesN, Env, Symbol,
};

use crate::{
    CapstoneEscrowIntentsContract, CapstoneEscrowIntentsContractClient, ContractError,
    CreateIntentRequest, IntentState, Role,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

macro_rules! create_intent {
    ($client:expr, $officer:expr, $payer:expr, $tenant:expr, $project:expr, $id:expr, $intent_hash:expr, $charter:expr, $conditions:expr, $asset:expr, $amount:expr, $expiry:expr, $version:expr) => {
        $create_intent!(client,
            $officer,
            $payer,
            $tenant,
            $project,
            &CreateIntentRequest {
                intent_id: (*$id).clone(),
                intent_hash: (*$intent_hash).clone(),
                charter_hash: (*$charter).clone(),
                conditions_hash: (*$conditions).clone(),
                asset: (*$asset).clone(),
                amount: *$amount,
                expires_at: *$expiry,
                expected_version: *$version,
            },
        )
    };
}

macro_rules! try_create_intent {
    ($client:expr, $officer:expr, $payer:expr, $tenant:expr, $project:expr, $id:expr, $intent_hash:expr, $charter:expr, $conditions:expr, $asset:expr, $amount:expr, $expiry:expr, $version:expr) => {
        $try_create_intent!(client,
            $officer,
            $payer,
            $tenant,
            $project,
            &CreateIntentRequest {
                intent_id: (*$id).clone(),
                intent_hash: (*$intent_hash).clone(),
                charter_hash: (*$charter).clone(),
                conditions_hash: (*$conditions).clone(),
                asset: (*$asset).clone(),
                amount: *$amount,
                expires_at: *$expiry,
                expected_version: *$version,
            },
        )
    };
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneEscrowIntentsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneEscrowIntentsContract, ());
    let client = CapstoneEscrowIntentsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let officer = Address::generate(&env);
    let payer = Address::generate(&env);
    let auditor = Address::generate(&env);
    let tenant = hash(&env, 20);
    let project = hash(&env, 21);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &officer, &Role::FinanceOfficer);
    client.grant_role(&admin, &tenant, &payer, &Role::Payer);
    client.grant_role(&admin, &tenant, &auditor, &Role::Auditor);
    (
        env, contract_id, admin, officer, payer, auditor, tenant, project, client,
    )
}

#[test]
fn intent_creation_requires_officer_and_payer_roles() {
    let (env, _, _, officer, payer, auditor, tenant, project, client) = setup();
    let outsider = Address::generate(&env);
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        try_create_intent!(client,
            &outsider,
            &payer,
            &tenant,
            &project,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &hash(&env, 33),
            &hash(&env, 34),
            &1_000,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        try_create_intent!(client,
            &officer,
            &auditor,
            &tenant,
            &project,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &hash(&env, 33),
            &hash(&env, 34),
            &1_000,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );

    let intent = create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 30),
        &hash(&env, 31),
        &hash(&env, 32),
        &hash(&env, 33),
        &hash(&env, 34),
        &1_000,
        &expiry,
        &0u32,
    );
    assert_eq!(intent.version, 1);
    assert_eq!(intent.payer, payer);
    assert_eq!(intent.state, IntentState::Pending);
    let auths = env.auths();
    assert_eq!(auths.len(), 2);
    assert!(
        (auths[0].0 == officer && auths[1].0 == payer)
            || (auths[0].0 == payer && auths[1].0 == officer)
    );

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBEINT"));
    let (event_tenant, event_project, event_intent, amount, asset):
        (BytesN<32>, BytesN<32>, BytesN<32>, i128, BytesN<32>) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_project, project);
    assert_eq!(event_intent, hash(&env, 30));
    assert_eq!(amount, 1_000);
    assert_eq!(asset, hash(&env, 34));
}

#[test]
fn intent_creation_is_replay_safe_and_binds_expected_version() {
    let (env, _, _, officer, payer, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;

    let first = create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 40),
        &hash(&env, 41),
        &hash(&env, 42),
        &hash(&env, 43),
        &hash(&env, 44),
        &500,
        &expiry,
        &0u32,
    );
    let retry = create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 40),
        &hash(&env, 41),
        &hash(&env, 42),
        &hash(&env, 43),
        &hash(&env, 44),
        &500,
        &expiry,
        &0u32,
    );
    assert_eq!(retry, first);
    assert_eq!(env.events().all().len(), 0);

    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, 40),
            &hash(&env, 45),
            &hash(&env, 42),
            &hash(&env, 43),
            &hash(&env, 44),
            &500,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::RequestConflict))
    );
    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, 46),
            &hash(&env, 47),
            &hash(&env, 42),
            &hash(&env, 43),
            &hash(&env, 44),
            &500,
            &expiry,
            &1u32,
        ),
        Err(Ok(ContractError::VersionConflict))
    );
}

#[test]
fn cancellation_and_expiry_are_idempotent_transitions() {
    let (env, _, _, officer, payer, auditor, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 30;
    create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 50),
        &hash(&env, 51),
        &hash(&env, 52),
        &hash(&env, 53),
        &hash(&env, 54),
        &700,
        &expiry,
        &0u32,
    );

    client.cancel_intent(
        &officer,
        &tenant,
        &project,
        &hash(&env, 50),
        &hash(&env, 55),
        &hash(&env, 56),
        &1u32,
    );
    assert_eq!(
        client
            .get_intent(&tenant, &project, &hash(&env, 50))
            .state,
        IntentState::Cancelled
    );
    let events_before = env.events().all().len();
    client.cancel_intent(
        &officer,
        &tenant,
        &project,
        &hash(&env, 50),
        &hash(&env, 55),
        &hash(&env, 56),
        &1u32,
    );
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(events_before, 1);
    assert_eq!(
        client.try_cancel_intent(
            &officer,
            &tenant,
            &project,
            &hash(&env, 50),
            &hash(&env, 57),
            &hash(&env, 58),
            &1u32,
        ),
        Err(Ok(ContractError::InvalidState))
    );

    create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 60),
        &hash(&env, 61),
        &hash(&env, 62),
        &hash(&env, 63),
        &hash(&env, 64),
        &800,
        &expiry,
        &0u32,
    );
    assert_eq!(
        client.try_mark_expired(
            &auditor,
            &tenant,
            &project,
            &hash(&env, 60),
            &hash(&env, 65),
            &1u32,
        ),
        Err(Ok(ContractError::InvalidExpiry))
    );
    env.ledger().set_timestamp(expiry);
    client.mark_expired(
        &auditor,
        &tenant,
        &project,
        &hash(&env, 60),
        &hash(&env, 65),
        &1u32,
    );
    assert_eq!(
        client
            .get_intent(&tenant, &project, &hash(&env, 60))
            .state,
        IntentState::Expired
    );
    client.mark_expired(
        &auditor,
        &tenant,
        &project,
        &hash(&env, 60),
        &hash(&env, 65),
        &1u32,
    );
    assert_eq!(env.events().all().len(), 0);
}

#[test]
fn tenant_amount_expiry_and_index_limits_are_enforced() {
    let (env, _, _, officer, payer, _, tenant, project, client) = setup();
    let other_tenant = hash(&env, 90);
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &other_tenant,
            &project,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &hash(&env, 73),
            &hash(&env, 74),
            &100,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &hash(&env, 73),
            &hash(&env, 74),
            &0,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::InvalidAmount))
    );
    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &hash(&env, 73),
            &hash(&env, 74),
            &100,
            &env.ledger().timestamp(),
            &0u32,
        ),
        Err(Ok(ContractError::InvalidExpiry))
    );

    let mut seed = 100u8;
    for _ in 0..16 {
        create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, seed),
            &hash(&env, seed + 1),
            &hash(&env, seed + 2),
            &hash(&env, seed + 3),
            &hash(&env, seed + 4),
            &10,
            &expiry,
            &0u32,
        );
        seed += 5;
    }
    assert_eq!(
        try_create_intent!(client,
            &officer,
            &payer,
            &tenant,
            &project,
            &hash(&env, 200),
            &hash(&env, 201),
            &hash(&env, 202),
            &hash(&env, 203),
            &hash(&env, 204),
            &10,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::CapacityExceeded))
    );
    assert_eq!(
        client.try_get_intent(&other_tenant, &project, &hash(&env, 100)),
        Err(Ok(ContractError::NotFound))
    );
}

#[test]
fn records_remain_readable_through_a_fresh_client() {
    let (env, contract_id, _, officer, payer, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let intent = create_intent!(client,
        &officer,
        &payer,
        &tenant,
        &project,
        &hash(&env, 210),
        &hash(&env, 211),
        &hash(&env, 212),
        &hash(&env, 213),
        &hash(&env, 214),
        &900,
        &expiry,
        &0u32,
    );

    let fresh = CapstoneEscrowIntentsContractClient::new(&env, &contract_id);
    assert_eq!(fresh.get_intent(&tenant, &project, &hash(&env, 210)), intent);
}
