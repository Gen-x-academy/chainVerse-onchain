extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CapstoneActivityFeedContract, CapstoneActivityFeedContractClient, ContractError, FeedKind, Role,
    Visibility,
};

struct Ctx {
    env: Env,
    admin: Address,
    coordinator: Address,
    member: Address,
    observer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
    client: CapstoneActivityFeedContractClient<'static>,
}

fn hash_u32(env: &Env, seed: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[28..].copy_from_slice(&seed.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_900_000_000);
    let contract_id = env.register(CapstoneActivityFeedContract, ());
    let client = CapstoneActivityFeedContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let member = Address::generate(&env);
    let observer = Address::generate(&env);
    client.initialize(&admin).unwrap();
    let tenant = hash_u32(&env, 1);
    let project = hash_u32(&env, 2);
    client
        .grant_role(&admin, &tenant, &coordinator, &Role::Coordinator)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &member, &Role::Member)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &observer, &Role::Observer)
        .unwrap();
    Ctx {
        env,
        admin,
        coordinator,
        member,
        observer,
        tenant,
        project,
        client,
    }
}

fn append(
    ctx: &Ctx,
    entry_seed: u32,
    request_seed: u32,
    expected_version: u32,
    visibility: Visibility,
) -> u32 {
    ctx.client
        .append_entry(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, request_seed),
            &expected_version,
            &hash_u32(&ctx.env, entry_seed),
            &FeedKind::Decision,
            &hash_u32(&ctx.env, entry_seed + 1_000),
            &hash_u32(&ctx.env, entry_seed + 2_000),
            &visibility,
        )
        .unwrap()
}

#[test]
fn initialization_and_roles_are_stored_and_one_time() {
    let ctx = setup();
    assert_eq!(
        ctx.client.try_initialize(&ctx.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client
            .try_grant_role(&outsider, &ctx.tenant, &outsider, &Role::Observer)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        ctx.client
            .try_append_entry(
                &outsider,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 10),
                &0,
                &hash_u32(&ctx.env, 11),
                &FeedKind::Decision,
                &hash_u32(&ctx.env, 12),
                &hash_u32(&ctx.env, 13),
                &Visibility::Public,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn entries_record_hashes_roles_and_timestamps_without_text() {
    let ctx = setup();
    let version = append(&ctx, 20, 21, 0, Visibility::Members);
    assert_eq!(version, 1);
    let entry = ctx
        .client
        .get_entry(&ctx.member, &ctx.tenant, &ctx.project, &1)
        .unwrap();
    assert_eq!(entry.seq, 1);
    assert_eq!(entry.id, hash_u32(&ctx.env, 20));
    assert_eq!(entry.kind, FeedKind::Decision);
    assert_eq!(entry.actor_role, Role::Coordinator);
    assert_eq!(entry.visibility, Visibility::Members);
    assert_eq!(entry.ref_hash, hash_u32(&ctx.env, 1_020));
    assert_eq!(entry.deep_link_hash, hash_u32(&ctx.env, 2_020));
    assert_eq!(entry.at, ctx.env.ledger().timestamp());
    assert_eq!(entry.prev_hash, zero(&ctx.env));
    assert_eq!(ctx.client.head_hash(&ctx.coordinator, &ctx.tenant, &ctx.project).unwrap(), entry.entry_hash);

    assert_eq!(
        ctx.client
            .try_get_entry(&ctx.observer, &ctx.tenant, &ctx.project, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(ctx
        .client
        .get_entry(&ctx.admin, &ctx.tenant, &ctx.project, &1)
        .is_ok());
}

#[test]
fn visibility_filters_pages_by_role() {
    let ctx = setup();
    append(&ctx, 30, 31, 0, Visibility::Public);
    append(&ctx, 40, 41, 1, Visibility::Members);
    append(&ctx, 50, 51, 2, Visibility::Coordinators);

    let observer = ctx
        .client
        .list_entries(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64)
        .unwrap();
    assert_eq!(observer.len(), 1);
    assert_eq!(observer.get(0).unwrap().seq, 1);

    let member = ctx
        .client
        .list_entries(&ctx.member, &ctx.tenant, &ctx.project, &0, &64)
        .unwrap();
    assert_eq!(member.len(), 2);
    assert_eq!(member.get(1).unwrap().seq, 2);

    let coordinator = ctx
        .client
        .list_entries(&ctx.coordinator, &ctx.tenant, &ctx.project, &0, &64)
        .unwrap();
    assert_eq!(coordinator.len(), 3);
    assert_eq!(coordinator.get(2).unwrap().seq, 3);
}

#[test]
fn retention_filters_old_entries_from_reads() {
    let ctx = setup();
    ctx.client
        .set_retention(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 60),
            &0,
            &10,
        )
        .unwrap();
    append(&ctx, 61, 62, 1, Visibility::Public);
    assert_eq!(
        ctx.client
            .list_entries(&ctx.coordinator, &ctx.tenant, &ctx.project, &0, &64)
            .unwrap()
            .len(),
        1
    );
    ctx.env
        .ledger()
        .with_mut(|li| li.timestamp = li.timestamp + 11);
    assert_eq!(
        ctx.client
            .list_entries(&ctx.coordinator, &ctx.tenant, &ctx.project, &0, &64)
            .unwrap()
            .len(),
        0
    );
    assert!(ctx
        .client
        .get_entry(&ctx.coordinator, &ctx.tenant, &ctx.project, &1)
        .is_ok());
}

#[test]
fn read_cursors_accept_replay_and_reject_regression() {
    let ctx = setup();
    append(&ctx, 70, 71, 0, Visibility::Public);
    let request = hash_u32(&ctx.env, 72);
    let version = ctx
        .client
        .mark_read(
            &ctx.member,
            &ctx.tenant,
            &ctx.project,
            &request,
            &1,
            &1,
        )
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(
        ctx.client
            .read_cursor(&ctx.member, &ctx.tenant, &ctx.project)
            .unwrap(),
        1
    );
    let replay = ctx
        .client
        .mark_read(
            &ctx.member,
            &ctx.tenant,
            &ctx.project,
            &request,
            &1,
            &1,
        )
        .unwrap();
    assert_eq!(replay, version);
    assert_eq!(ctx.env.events().all().len(), 0);

    assert_eq!(
        ctx.client
            .try_mark_read(
                &ctx.member,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 73),
                &2,
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidSequence
    );
    assert_eq!(
        ctx.client
            .try_mark_read(
                &ctx.member,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 74),
                &2,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidSequence
    );
}

#[test]
fn append_replay_is_idempotent_and_payload_conflicts_fail() {
    let ctx = setup();
    let request = hash_u32(&ctx.env, 80);
    ctx.client
        .append_entry(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &0,
            &hash_u32(&ctx.env, 81),
            &FeedKind::Decision,
            &hash_u32(&ctx.env, 1_081),
            &hash_u32(&ctx.env, 2_081),
            &Visibility::Public,
        )
        .unwrap();
    assert_eq!(ctx.env.events().all().len(), 1);
    let replay = ctx
        .client
        .append_entry(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &0,
            &hash_u32(&ctx.env, 81),
            &FeedKind::Decision,
            &hash_u32(&ctx.env, 1_081),
            &hash_u32(&ctx.env, 2_081),
            &Visibility::Public,
        )
        .unwrap();
    assert_eq!(replay, 1);
    assert_eq!(ctx.env.events().all().len(), 0);

    assert_eq!(
        ctx.client
            .try_append_entry(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &request,
                &0,
                &hash_u32(&ctx.env, 81),
                &FeedKind::Decision,
                &hash_u32(&ctx.env, 9_999),
                &hash_u32(&ctx.env, 2_081),
                &Visibility::Public,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 1);
}

#[test]
fn stale_versions_and_coordinator_visibility_rules_are_enforced() {
    let ctx = setup();
    append(&ctx, 90, 91, 0, Visibility::Public);
    assert_eq!(
        ctx.client
            .try_append_entry(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 92),
                &0,
                &hash_u32(&ctx.env, 93),
                &FeedKind::Review,
                &hash_u32(&ctx.env, 1_093),
                &hash_u32(&ctx.env, 2_093),
                &Visibility::Public,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::VersionConflict
    );
    assert_eq!(
        ctx.client
            .try_append_entry(
                &ctx.member,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 94),
                &1,
                &hash_u32(&ctx.env, 95),
                &FeedKind::Review,
                &hash_u32(&ctx.env, 1_095),
                &hash_u32(&ctx.env, 2_095),
                &Visibility::Coordinators,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidVisibility
    );
}

#[test]
fn feed_is_a_bounded_hash_chain() {
    let ctx = setup();
    let mut version = 0;
    for i in 0..65u32 {
        version = append(&ctx, 10_000 + i, 20_000 + i, version, Visibility::Public);
    }
    assert_eq!(version, 65);
    let page = ctx
        .client
        .list_entries(&ctx.coordinator, &ctx.tenant, &ctx.project, &0, &64)
        .unwrap();
    assert_eq!(page.len(), 64);
    assert_eq!(page.get(0).unwrap().seq, 2);
    assert_eq!(page.get(63).unwrap().seq, 65);
    let mut prior = page.get(0).unwrap().prev_hash;
    for i in 0..page.len() {
        let entry = page.get(i).unwrap();
        assert_eq!(entry.prev_hash, prior);
        prior = entry.entry_hash;
    }
    assert_eq!(
        ctx.client
            .try_get_entry(&ctx.coordinator, &ctx.tenant, &ctx.project, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    assert_eq!(
        ctx.client
            .try_list_entries(&ctx.coordinator, &ctx.tenant, &ctx.project, &0, &65)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
}

#[test]
fn reads_are_tenant_scoped() {
    let ctx = setup();
    append(&ctx, 110, 111, 0, Visibility::Public);
    let other_tenant = hash_u32(&ctx.env, 9_110);
    assert_eq!(
        ctx.client
            .try_get_entry(&ctx.observer, &other_tenant, &ctx.project, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    ctx.client
        .revoke_role(&ctx.admin, &ctx.tenant, &ctx.observer, &Role::Observer)
        .unwrap();
    assert_eq!(
        ctx.client
            .try_get_entry(&ctx.observer, &ctx.tenant, &ctx.project, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn events_use_stable_id_and_code_payloads() {
    let ctx = setup();
    append(&ctx, 120, 121, 0, Visibility::Public);
    let events = ctx.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("CFAENT"));
    let payload: (BytesN<32>, BytesN<32>, u32, u32, u32) = data.try_into_val(&ctx.env).unwrap();
    assert_eq!(payload.0, ctx.tenant);
    assert_eq!(payload.1, ctx.project);
    assert_eq!(payload.2, 1);
    assert_eq!(payload.3, FeedKind::Decision as u32);
    assert_eq!(payload.4, Visibility::Public as u32);
}
