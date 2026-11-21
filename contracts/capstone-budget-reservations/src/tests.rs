#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    symbol_short, Address, BytesN, Env, Symbol,
};

use crate::{
    CapstoneBudgetReservationsContract, CapstoneBudgetReservationsContractClient, ContractError,
    ReservationState, ReserveRequest, Role,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

macro_rules! reserve {
    ($client:expr, $sponsor:expr, $tenant:expr, $project:expr, $cap:expr, $request:expr, $request_hash:expr, $plan:expr, $asset:expr, $amount:expr, $expiry:expr, $version:expr) => {
        $reserve!(client,
            $sponsor,
            $tenant,
            $project,
            &ReserveRequest {
                cap_id: (*$cap).clone(),
                request_id: (*$request).clone(),
                request_hash: (*$request_hash).clone(),
                plan_hash: (*$plan).clone(),
                asset: (*$asset).clone(),
                amount: *$amount,
                expires_at: *$expiry,
                expected_version: *$version,
            },
        )
    };
}

macro_rules! try_reserve {
    ($client:expr, $sponsor:expr, $tenant:expr, $project:expr, $cap:expr, $request:expr, $request_hash:expr, $plan:expr, $asset:expr, $amount:expr, $expiry:expr, $version:expr) => {
        $try_reserve!(client,
            $sponsor,
            $tenant,
            $project,
            &ReserveRequest {
                cap_id: (*$cap).clone(),
                request_id: (*$request).clone(),
                request_hash: (*$request_hash).clone(),
                plan_hash: (*$plan).clone(),
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
    BytesN<32>,
    CapstoneBudgetReservationsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneBudgetReservationsContract, ());
    let client = CapstoneBudgetReservationsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let sponsor = Address::generate(&env);
    let auditor = Address::generate(&env);
    let tenant = hash(&env, 20);
    let cap_id = hash(&env, 21);
    let project = hash(&env, 22);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &manager, &Role::ProgramManager);
    client.grant_role(&admin, &tenant, &sponsor, &Role::Sponsor);
    client.grant_role(&admin, &tenant, &auditor, &Role::Auditor);
    (
        env, contract_id, admin, manager, sponsor, auditor, tenant, cap_id, project, client,
    )
}

#[test]
fn cap_configuration_requires_stored_role_and_is_idempotent() {
    let (env, _, _, manager, sponsor, _, tenant, cap_id, _, client) = setup();
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        client.try_configure_cap(
            &sponsor,
            &tenant,
            &cap_id,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &1_000,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );

    assert_eq!(
        client.configure_cap(
            &manager,
            &tenant,
            &cap_id,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &1_000,
            &expiry,
            &0u32,
        ),
        1
    );
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBRGCAP"));
    let (event_tenant, event_cap, version, limit): (BytesN<32>, BytesN<32>, u32, i128) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_cap, cap_id);
    assert_eq!(version, 1);
    assert_eq!(limit, 1_000);

    assert_eq!(
        client.configure_cap(
            &manager,
            &tenant,
            &cap_id,
            &hash(&env, 30),
            &hash(&env, 31),
            &hash(&env, 32),
            &1_000,
            &expiry,
            &0u32,
        ),
        1
    );
    assert_eq!(env.events().all().len(), 0);

    assert_eq!(
        client.try_configure_cap(
            &manager,
            &tenant,
            &cap_id,
            &hash(&env, 30),
            &hash(&env, 40),
            &hash(&env, 32),
            &1_000,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::RequestConflict))
    );
}

#[test]
fn reservation_is_idempotent_and_enforces_the_cap() {
    let (env, _, _, manager, sponsor, _, tenant, cap_id, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let asset = hash(&env, 32);
    client.configure_cap(
        &manager,
        &tenant,
        &cap_id,
        &hash(&env, 30),
        &hash(&env, 31),
        &asset,
        &1_000,
        &expiry,
        &0u32,
    );

    let first = reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 50),
        &hash(&env, 51),
        &hash(&env, 52),
        &asset,
        &300,
        &expiry,
        &0u32,
    );
    assert_eq!(first.amount, 300);
    assert_eq!(first.state, ReservationState::Active);
    assert_eq!(client.get_cap(&tenant, &cap_id).reserved, 300);
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, sponsor);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBRRES"));
    let (event_tenant, event_project, event_request, amount, event_asset):
        (BytesN<32>, BytesN<32>, BytesN<32>, i128, BytesN<32>) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_project, project);
    assert_eq!(event_request, hash(&env, 50));
    assert_eq!(amount, 300);
    assert_eq!(event_asset, asset);

    let retry = reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 50),
        &hash(&env, 51),
        &hash(&env, 52),
        &asset,
        &300,
        &expiry,
        &0u32,
    );
    assert_eq!(retry, first);
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(client.get_cap(&tenant, &cap_id).reserved, 300);

    reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 53),
        &hash(&env, 54),
        &hash(&env, 55),
        &asset,
        &400,
        &expiry,
        &0u32,
    );
    assert_eq!(client.get_cap(&tenant, &cap_id).reserved, 700);

    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 56),
            &hash(&env, 57),
            &hash(&env, 58),
            &asset,
            &400,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::CapExceeded))
    );
    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 56),
            &hash(&env, 57),
            &hash(&env, 58),
            &asset,
            &400,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::AlreadyReserved))
    );
}

#[test]
fn release_and_expiry_restore_capacity_and_cannot_be_duplicated() {
    let (env, _, _, manager, sponsor, auditor, tenant, cap_id, project, client) = setup();
    let expiry = env.ledger().timestamp() + 30;
    let asset = hash(&env, 32);
    client.configure_cap(
        &manager,
        &tenant,
        &cap_id,
        &hash(&env, 30),
        &hash(&env, 31),
        &asset,
        &1_000,
        &expiry,
        &0u32,
    );
    reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 60),
        &hash(&env, 61),
        &hash(&env, 62),
        &asset,
        &300,
        &expiry,
        &0u32,
    );

    assert_eq!(
        client.try_release(
            &sponsor,
            &tenant,
            &project,
            &hash(&env, 60),
            &hash(&env, 63),
            &2u32,
        ),
        Err(Ok(ContractError::VersionConflict))
    );
    client.release(
        &sponsor,
        &tenant,
        &project,
        &hash(&env, 60),
        &hash(&env, 63),
        &1u32,
    );
    assert_eq!(client.get_cap(&tenant, &cap_id).reserved, 0);
    assert_eq!(
        client
            .get_reservation(&tenant, &project, &hash(&env, 60))
            .state,
        ReservationState::Released
    );
    let events_before = env.events().all().len();
    client.release(
        &sponsor,
        &tenant,
        &project,
        &hash(&env, 60),
        &hash(&env, 63),
        &1u32,
    );
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(events_before, 1);
    assert_eq!(
        client.try_release(
            &sponsor,
            &tenant,
            &project,
            &hash(&env, 60),
            &hash(&env, 64),
            &1u32,
        ),
        Err(Ok(ContractError::InvalidState))
    );

    reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 65),
        &hash(&env, 66),
        &hash(&env, 67),
        &asset,
        &200,
        &expiry,
        &0u32,
    );
    assert_eq!(
        client.try_mark_expired(
            &auditor,
            &tenant,
            &project,
            &hash(&env, 65),
            &1u32,
        ),
        Err(Ok(ContractError::InvalidExpiry))
    );
    env.ledger().set_timestamp(expiry);
    client.mark_expired(&auditor, &tenant, &project, &hash(&env, 65), &1u32);
    assert_eq!(client.get_cap(&tenant, &cap_id).reserved, 0);
    assert_eq!(
        client
            .get_reservation(&tenant, &project, &hash(&env, 65))
            .state,
        ReservationState::Expired
    );
    client.mark_expired(&auditor, &tenant, &project, &hash(&env, 65), &1u32);
    assert_eq!(env.events().all().len(), 0);
}

#[test]
fn tenant_asset_and_expiry_boundaries_are_enforced() {
    let (env, _, _, manager, sponsor, auditor, tenant, cap_id, project, client) = setup();
    let other_tenant = hash(&env, 90);
    let expiry = env.ledger().timestamp() + 500;
    let asset = hash(&env, 32);
    client.configure_cap(
        &manager,
        &tenant,
        &cap_id,
        &hash(&env, 30),
        &hash(&env, 31),
        &asset,
        &1_000,
        &expiry,
        &0u32,
    );

    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &other_tenant,
            &project,
            &cap_id,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &asset,
            &100,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &hash(&env, 73),
            &100,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::AssetMismatch))
    );
    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 70),
            &hash(&env, 71),
            &hash(&env, 72),
            &asset,
            &100,
            &env.ledger().timestamp(),
            &0u32,
        ),
        Err(Ok(ContractError::InvalidExpiry))
    );
    assert_eq!(
        client.try_mark_expired(
            &auditor,
            &tenant,
            &project,
            &hash(&env, 70),
            &0u32,
        ),
        Err(Ok(ContractError::NotFound))
    );
    assert_eq!(
        client.try_get_cap(&other_tenant, &cap_id),
        Err(Ok(ContractError::NotFound))
    );
}

#[test]
fn index_bounds_and_checked_arithmetic_are_enforced() {
    let (env, _, _, manager, sponsor, _, tenant, cap_id, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let asset = hash(&env, 32);
    client.configure_cap(
        &manager,
        &tenant,
        &cap_id,
        &hash(&env, 30),
        &hash(&env, 31),
        &asset,
        &i128::MAX,
        &expiry,
        &0u32,
    );
    reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 80),
        &hash(&env, 81),
        &hash(&env, 82),
        &asset,
        &1,
        &expiry,
        &0u32,
    );
    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 83),
            &hash(&env, 84),
            &hash(&env, 85),
            &asset,
            &i128::MAX,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Overflow))
    );
    client.release(
        &sponsor,
        &tenant,
        &project,
        &hash(&env, 80),
        &hash(&env, 86),
        &1u32,
    );

    let mut seed = 100u8;
    for _ in 0..7 {
        reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, seed),
            &hash(&env, seed + 1),
            &hash(&env, seed + 2),
            &asset,
            &10,
            &expiry,
            &0u32,
        );
        client.release(
            &sponsor,
            &tenant,
            &project,
            &hash(&env, seed),
            &hash(&env, seed + 3),
            &1u32,
        );
        seed += 4;
    }
    assert_eq!(
        try_reserve!(client,
            &sponsor,
            &tenant,
            &project,
            &cap_id,
            &hash(&env, 200),
            &hash(&env, 201),
            &hash(&env, 202),
            &asset,
            &10,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::CapacityExceeded))
    );
}

#[test]
fn records_remain_readable_through_a_fresh_client() {
    let (env, contract_id, _, manager, sponsor, _, tenant, cap_id, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let asset = hash(&env, 32);
    client.configure_cap(
        &manager,
        &tenant,
        &cap_id,
        &hash(&env, 30),
        &hash(&env, 31),
        &asset,
        &1_000,
        &expiry,
        &0u32,
    );
    reserve!(client,
        &sponsor,
        &tenant,
        &project,
        &cap_id,
        &hash(&env, 210),
        &hash(&env, 211),
        &hash(&env, 212),
        &asset,
        &100,
        &expiry,
        &0u32,
    );

    let fresh = CapstoneBudgetReservationsContractClient::new(&env, &contract_id);
    assert_eq!(fresh.get_cap(&tenant, &cap_id), client.get_cap(&tenant, &cap_id));
    assert_eq!(
        fresh.get_reservation(&tenant, &project, &hash(&env, 210)),
        client.get_reservation(&tenant, &project, &hash(&env, 210))
    );
}
