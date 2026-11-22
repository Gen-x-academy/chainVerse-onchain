extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CapstoneSourceRefsContract, CapstoneSourceRefsContractClient, ConnectionState, ContractError,
    Provider, RefKind, RefState, Role,
};

struct Ctx {
    env: Env,
    admin: Address,
    maintainer: Address,
    integrator: Address,
    viewer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
    client: CapstoneSourceRefsContractClient<'static>,
}

fn hash_u32(env: &Env, seed: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[28..].copy_from_slice(&seed.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 1_800_000_000);
    let contract_id = env.register(CapstoneSourceRefsContract, ());
    let client = CapstoneSourceRefsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let maintainer = Address::generate(&env);
    let integrator = Address::generate(&env);
    let viewer = Address::generate(&env);
    client.initialize(&admin).unwrap();
    let tenant = hash_u32(&env, 1);
    let project = hash_u32(&env, 2);
    client
        .grant_role(&admin, &tenant, &maintainer, &Role::Maintainer)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &integrator, &Role::Integrator)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &viewer, &Role::Viewer)
        .unwrap();
    Ctx {
        env,
        admin,
        maintainer,
        integrator,
        viewer,
        tenant,
        project,
        client,
    }
}

fn connect(ctx: &Ctx, seed: u32, provider: Provider, expected_version: u32) -> BytesN<32> {
    let connection_id = hash_u32(&ctx.env, seed);
    ctx.client
        .connect_provider(
            &ctx.maintainer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, seed + 1_000),
            &expected_version,
            &connection_id,
            &provider,
            &hash_u32(&ctx.env, seed + 2_000),
            &1,
        )
        .unwrap();
    connection_id
}

fn register(
    ctx: &Ctx,
    seed: u32,
    connection_id: &BytesN<32>,
    provider: Provider,
    expected_version: u32,
) -> BytesN<32> {
    let ref_id = hash_u32(&ctx.env, seed);
    ctx.client
        .register_ref(
            &ctx.maintainer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, seed + 3_000),
            &expected_version,
            &ref_id,
            connection_id,
            &provider,
            &RefKind::Commit,
            &hash_u32(&ctx.env, seed + 4_000),
        )
        .unwrap();
    ref_id
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
            .try_grant_role(&outsider, &ctx.tenant, &outsider, &Role::Viewer)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        ctx.client
            .try_connect_provider(
                &ctx.viewer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 10),
                &0,
                &hash_u32(&ctx.env, 11),
                &Provider::GitHub,
                &hash_u32(&ctx.env, 12),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn connect_register_verify_lifecycle() {
    let ctx = setup();
    let connection_id = connect(&ctx, 20, Provider::GitHub, 0);
    let connection = ctx
        .client
        .get_connection(
            &ctx.viewer,
            &ctx.tenant,
            &ctx.project,
            &connection_id,
        )
        .unwrap();
    assert_eq!(connection.state, ConnectionState::Active);
    assert_eq!(connection.provider, Provider::GitHub);
    assert_eq!(connection.created_at, ctx.env.ledger().timestamp());

    let ref_id = register(&ctx, 30, &connection_id, Provider::GitHub, 1);
    let reference = ctx
        .client
        .get_ref(&ctx.viewer, &ctx.tenant, &ctx.project, &ref_id)
        .unwrap();
    assert_eq!(reference.state, RefState::Proposed);
    assert_eq!(reference.kind, RefKind::Commit);
    assert_eq!(reference.proof_hash, None);

    let version = ctx
        .client
        .verify_ref(
            &ctx.integrator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 40),
            &2,
            &ref_id,
            &hash_u32(&ctx.env, 41),
        )
        .unwrap();
    assert_eq!(version, 3);
    let verified = ctx
        .client
        .get_ref(&ctx.viewer, &ctx.tenant, &ctx.project, &ref_id)
        .unwrap();
    assert_eq!(verified.state, RefState::Verified);
    assert_eq!(verified.proof_hash, Some(hash_u32(&ctx.env, 41)));
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 3);
}

#[test]
fn request_replay_is_idempotent_and_payload_conflicts_fail() {
    let ctx = setup();
    let connection_id = hash_u32(&ctx.env, 50);
    let request_id = hash_u32(&ctx.env, 51);
    ctx.client
        .connect_provider(
            &ctx.maintainer,
            &ctx.tenant,
            &ctx.project,
            &request_id,
            &0,
            &connection_id,
            &Provider::GitHub,
            &hash_u32(&ctx.env, 52),
            &1,
        )
        .unwrap();
    assert_eq!(ctx.env.events().all().len(), 1);
    ctx.client
        .connect_provider(
            &ctx.maintainer,
            &ctx.tenant,
            &ctx.project,
            &request_id,
            &0,
            &connection_id,
            &Provider::GitHub,
            &hash_u32(&ctx.env, 52),
            &1,
        )
        .unwrap();
    assert_eq!(ctx.env.events().all().len(), 0);
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 1);

    assert_eq!(
        ctx.client
            .try_connect_provider(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &request_id,
                &0,
                &connection_id,
                &Provider::GitHub,
                &hash_u32(&ctx.env, 99),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
}

#[test]
fn revoked_or_mismatched_connections_cannot_register_refs() {
    let ctx = setup();
    let connection_id = connect(&ctx, 60, Provider::GitLab, 0);
    assert_eq!(
        ctx.client
            .try_register_ref(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 61),
                &1,
                &hash_u32(&ctx.env, 62),
                &connection_id,
                &Provider::GitHub,
                &RefKind::Branch,
                &hash_u32(&ctx.env, 63),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ProviderMismatch
    );
    ctx.client
        .revoke_connection(
            &ctx.maintainer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 64),
            &1,
            &connection_id,
        )
        .unwrap();
    assert_eq!(
        ctx.client
            .try_register_ref(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 65),
                &2,
                &hash_u32(&ctx.env, 66),
                &connection_id,
                &Provider::GitLab,
                &RefKind::Branch,
                &hash_u32(&ctx.env, 67),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ConnectionRevoked
    );
    let previous_version = ctx.client.project_version(&ctx.tenant, &ctx.project);
    assert_eq!(
        ctx.client
            .try_revoke_connection(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 68),
                &previous_version,
                &connection_id,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
}

#[test]
fn webhook_deliveries_are_monotonic_and_replay_safe() {
    let ctx = setup();
    let connection_id = connect(&ctx, 70, Provider::Bitbucket, 0);
    let hook_id = hash_u32(&ctx.env, 71);
    ctx.client
        .record_webhook(
            &ctx.integrator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 72),
            &1,
            &hook_id,
            &connection_id,
            &hash_u32(&ctx.env, 73),
            &7,
        )
        .unwrap();
    let hook = ctx
        .client
        .get_webhook(&ctx.viewer, &ctx.tenant, &ctx.project, &hook_id)
        .unwrap();
    assert_eq!(hook.last_delivery_seq, 7);
    assert_eq!(hook.provider, Provider::Bitbucket);
    assert_eq!(hook.created_at, ctx.env.ledger().timestamp());

    let request_id = hash_u32(&ctx.env, 74);
    ctx.client
        .record_webhook(
            &ctx.integrator,
            &ctx.tenant,
            &ctx.project,
            &request_id,
            &2,
            &hook_id,
            &connection_id,
            &hash_u32(&ctx.env, 75),
            &8,
        )
        .unwrap();
    let replay_version = ctx
        .client
        .record_webhook(
            &ctx.integrator,
            &ctx.tenant,
            &ctx.project,
            &request_id,
            &2,
            &hook_id,
            &connection_id,
            &hash_u32(&ctx.env, 75),
            &8,
        )
        .unwrap();
    assert_eq!(replay_version, 3);
    assert_eq!(ctx.env.events().all().len(), 0);

    assert_eq!(
        ctx.client
            .try_record_webhook(
                &ctx.integrator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 76),
                &3,
                &hook_id,
                &connection_id,
                &hash_u32(&ctx.env, 77),
                &8,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::DeliveryReplay
    );
    ctx.client
        .record_webhook(
            &ctx.integrator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 78),
            &3,
            &hook_id,
            &connection_id,
            &hash_u32(&ctx.env, 79),
            &9,
        )
        .unwrap();
    assert_eq!(
        ctx.client
            .get_webhook(&ctx.viewer, &ctx.tenant, &ctx.project, &hook_id)
            .unwrap()
            .last_delivery_seq,
        9
    );
}

#[test]
fn reads_are_tenant_scoped_and_revocation_is_immediate() {
    let ctx = setup();
    let connection_id = connect(&ctx, 80, Provider::GitHub, 0);
    let other_tenant = hash_u32(&ctx.env, 9_080);
    assert_eq!(
        ctx.client
            .try_get_connection(
                &ctx.maintainer,
                &other_tenant,
                &ctx.project,
                &connection_id,
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
            .try_get_connection(
                &ctx.viewer,
                &ctx.tenant,
                &ctx.project,
                &connection_id,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn pages_are_bounded_and_ref_index_is_capped() {
    let ctx = setup();
    assert_eq!(
        ctx.client
            .try_list_refs(
                &ctx.viewer,
                &ctx.tenant,
                &ctx.project,
                &0,
                &0,
                &Some(Provider::GitHub),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
    assert_eq!(
        ctx.client
            .try_list_refs(
                &ctx.viewer,
                &ctx.tenant,
                &ctx.project,
                &0,
                &65,
                &None,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
    let connection_id = connect(&ctx, 90, Provider::GitHub, 0);
    let mut version = 1;
    for i in 0..64u32 {
        let ref_id = hash_u32(&ctx.env, 10_000 + i);
        ctx.client
            .register_ref(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 20_000 + i),
                &version,
                &ref_id,
                &connection_id,
                &Provider::GitHub,
                &RefKind::Commit,
                &hash_u32(&ctx.env, 30_000 + i),
            )
            .unwrap();
        version += 1;
    }
    let page = ctx
        .client
        .list_refs(
            &ctx.viewer,
            &ctx.tenant,
            &ctx.project,
            &60,
            &64,
            &Some(Provider::GitHub),
        )
        .unwrap();
    assert_eq!(page.len(), 4);
    assert_eq!(
        ctx.client
            .try_register_ref(
                &ctx.maintainer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 99_000),
                &version,
                &hash_u32(&ctx.env, 99_001),
                &connection_id,
                &Provider::GitHub,
                &RefKind::Commit,
                &hash_u32(&ctx.env, 99_002),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}

#[test]
fn events_use_stable_id_and_code_payloads() {
    let ctx = setup();
    let connection_id = connect(&ctx, 110, Provider::GitHub, 0);
    let events = ctx.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("SRCONN"));
    let payload: (BytesN<32>, BytesN<32>, BytesN<32>, u32, u32) =
        data.try_into_val(&ctx.env).unwrap();
    assert_eq!(payload.0, ctx.tenant);
    assert_eq!(payload.1, ctx.project);
    assert_eq!(payload.2, connection_id);
    assert_eq!(payload.3, Provider::GitHub as u32);
    assert_eq!(payload.4, 1);
}
