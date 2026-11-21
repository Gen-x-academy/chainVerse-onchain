#![cfg(test)]

use crate::{
    CapstoneResourceLibraryContract, CapstoneResourceLibraryContractClient, ContractError,
    AccessPolicy, LicenseKind, LibraryRole, ResourceAction, ResourceDraft, ResourceKind,
    ResourceState, ResourceVersionInput, MAX_VERSIONS, SCHEMA_VERSION,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol,
};

const BASE_VERSION: u32 = 3;

struct Fixture<'a> {
    env: Env,
    tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    curator: Address,
    reviewer: Address,
    member: Address,
    client: CapstoneResourceLibraryContractClient<'a>,
}

fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneResourceLibraryContract, ());
    let client = CapstoneResourceLibraryContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let curator = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let member = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin, &tenant, &project);
    client.grant_role(
        &admin, &tenant, &project, &curator, &LibraryRole::Curator, &hash(&env, 11), &0,
    );
    client.grant_role(
        &admin, &tenant, &project, &reviewer, &LibraryRole::Reviewer, &hash(&env, 12), &1,
    );
    client.grant_role(
        &admin, &tenant, &project, &member, &LibraryRole::Member, &hash(&env, 13), &2,
    );
    Fixture {
        env,
        tenant,
        project,
        admin,
        curator,
        reviewer,
        member,
        client,
    }
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn draft(env: &Env, seed: u8, policy: AccessPolicy) -> ResourceDraft {
    ResourceDraft {
        resource_id: hash(env, seed),
        kind: ResourceKind::Dataset,
        content_hash: hash(env, seed.wrapping_add(20)),
        integrity_hash: hash(env, seed.wrapping_add(40)),
        size_bytes: 1_024,
        license: LicenseKind::CcBy,
        license_ref_hash: hash(env, seed.wrapping_add(60)),
        policy,
        retention_seconds: 100,
    }
}

#[test]
fn role_hierarchy_is_enforced_and_unassigned_actors_are_denied() {
    let f = setup();
    let outsider = Address::generate(&f.env);
    let resource = draft(&f.env, 20, AccessPolicy::ProjectMembers);

    assert_eq!(
        f.client
            .try_propose_resource(
                &f.member,
                &f.tenant,
                &f.project,
                &resource,
                &hash(&f.env, 60),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        f.client
            .try_propose_resource(
                &outsider,
                &f.tenant,
                &f.project,
                &resource,
                &hash(&f.env, 61),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
}

#[test]
fn proposal_replay_is_idempotent_and_integrity_is_checked() {
    let f = setup();
    let resource = draft(&f.env, 30, AccessPolicy::ProjectMembers);
    let request = hash(&f.env, 62);
    let events_before = f.env.events().all().len();

    let first = f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &request,
        &BASE_VERSION,
    );
    let replay = f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &request,
        &BASE_VERSION,
    );
    assert_eq!(first, replay);
    assert_eq!(replay, resource.resource_id);
    assert_eq!(f.env.events().all().len(), events_before + 1);
    assert_eq!(
        f.client
            .get_resource(&f.member, &f.tenant, &f.project, &resource.resource_id)
            .state,
        ResourceState::Proposed
    );
    assert_eq!(
        f.client
            .try_propose_resource(
                &f.curator,
                &f.tenant,
                &f.project,
                &resource,
                &hash(&f.env, 63),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    let mut changed = resource.clone();
    changed.content_hash = hash(&f.env, 64);
    assert_eq!(
        f.client
            .try_propose_resource(
                &f.curator,
                &f.tenant,
                &f.project,
                &changed,
                &request,
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    let mut invalid = draft(&f.env, 31, AccessPolicy::ProjectMembers);
    invalid.integrity_hash = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(
        f.client
            .try_propose_resource(
                &f.curator,
                &f.tenant,
                &f.project,
                &invalid,
                &hash(&f.env, 65),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidIntegrity
    );
}

#[test]
fn approval_and_deprecation_follow_stable_transitions() {
    let f = setup();
    let resource = draft(&f.env, 40, AccessPolicy::ProjectMembers);
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 70),
        &BASE_VERSION,
    );
    f.client.approve_resource(
        &f.admin,
        &f.tenant,
        &f.project,
        &resource.resource_id,
        &hash(&f.env, 71),
        &(BASE_VERSION + 1),
    );
    assert_eq!(
        f.client
            .get_resource(&f.member, &f.tenant, &f.project, &resource.resource_id)
            .state,
        ResourceState::Approved
    );
    assert_eq!(
        f.client
            .try_approve_resource(
                &f.admin,
                &f.tenant,
                &f.project,
                &resource.resource_id,
                &hash(&f.env, 72),
                &(BASE_VERSION + 2)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
    f.client.deprecate_resource(
        &f.admin,
        &f.tenant,
        &f.project,
        &resource.resource_id,
        &hash(&f.env, 73),
        &(BASE_VERSION + 2),
    );
    assert_eq!(
        f.client
            .can_access(&f.member, &f.tenant, &f.project, &resource.resource_id),
        false
    );
}

#[test]
fn access_policies_are_denied_by_default_then_explicitly_granted() {
    let f = setup();
    let resource = draft(&f.env, 50, AccessPolicy::OwnerOnly);
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 80),
        &BASE_VERSION,
    );
    f.client.approve_resource(
        &f.admin,
        &f.tenant,
        &f.project,
        &resource.resource_id,
        &hash(&f.env, 81),
        &(BASE_VERSION + 1),
    );
    assert_eq!(
        f.client
            .can_access(&f.curator, &f.tenant, &f.project, &resource.resource_id),
        true
    );
    assert_eq!(
        f.client
            .can_access(&f.member, &f.tenant, &f.project, &resource.resource_id),
        false
    );
    f.client.set_policy(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource.resource_id,
        AccessPolicy::ProjectMembers,
        &hash(&f.env, 82),
        &(BASE_VERSION + 2),
    );
    assert_eq!(
        f.client
            .can_access(&f.member, &f.tenant, &f.project, &resource.resource_id),
        true
    );
    f.client.set_policy(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource.resource_id,
        AccessPolicy::Reviewers,
        &hash(&f.env, 83),
        &(BASE_VERSION + 3),
    );
    assert_eq!(
        f.client
            .can_access(&f.member, &f.tenant, &f.project, &resource.resource_id),
        false
    );
    assert_eq!(
        f.client
            .can_access(&f.reviewer, &f.tenant, &f.project, &resource.resource_id),
        true
    );
}

#[test]
fn resource_versions_are_bounded_and_auditable() {
    let f = setup();
    let resource = draft(&f.env, 60, AccessPolicy::ProjectMembers);
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 90),
        &BASE_VERSION,
    );
    let mut version = BASE_VERSION + 1;
    for index in 1..MAX_VERSIONS {
        let input = ResourceVersionInput {
            resource_id: resource.resource_id.clone(),
            content_hash: hash(&f.env, 100 + index as u8),
            integrity_hash: hash(&f.env, 120 + index as u8),
            size_bytes: 2_048,
        };
        let result = f.client.add_version(
            &f.curator,
            &f.tenant,
            &f.project,
            &input,
            &hash(&f.env, 140 + index as u8),
            &version,
        );
        assert_eq!(result, input.content_hash);
        version += 1;
    }
    assert_eq!(
        f.client
            .get_version(&f.member, &f.tenant, &f.project, &resource.resource_id, MAX_VERSIONS)
            .version,
        MAX_VERSIONS
    );
    let overflow = ResourceVersionInput {
        resource_id: resource.resource_id,
        content_hash: hash(&f.env, 200),
        integrity_hash: hash(&f.env, 201),
        size_bytes: 2_048,
    };
    assert_eq!(
        f.client
            .try_add_version(
                &f.curator,
                &f.tenant,
                &f.project,
                &overflow,
                &hash(&f.env, 202),
                &version
            )
            .unwrap_err()
            .unwrap(),
        ContractError::VersionsFull
    );
}

#[test]
fn retention_expiry_and_timestamp_overflow_are_deterministic() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 2_000);
    let resource = draft(&f.env, 70, AccessPolicy::ProjectMembers);
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 210),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_resource(&f.member, &f.tenant, &f.project, &resource.resource_id)
            .expires_at,
        2_100
    );
    f.env.ledger().with_mut(|li| li.timestamp = 2_101);
    assert_eq!(
        f.client
            .try_get_resource(&f.member, &f.tenant, &f.project, &resource.resource_id)
            .unwrap_err()
            .unwrap(),
        ContractError::Expired
    );
    let other = setup();
    other
        .env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    assert_eq!(
        other
            .client
            .try_propose_resource(
                &other.curator,
                &other.tenant,
                &other.project,
                &draft(&other.env, 71, AccessPolicy::ProjectMembers),
                &hash(&other.env, 211),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TimestampOverflow
    );
}

#[test]
fn proposal_event_uses_only_ids_enums_hashes_and_versions() {
    let f = setup();
    let resource = draft(&f.env, 80, AccessPolicy::ProjectMembers);
    let events_before = f.env.events().all().len();
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 220),
        &BASE_VERSION,
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(events_before).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&f.env).unwrap();
    assert_eq!(topic, symbol_short!("RSCP"));
    let decoded: (BytesN<32>, ResourceAction, u32, BytesN<32>) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(decoded.0, resource.resource_id);
    assert_eq!(decoded.1, ResourceAction::Propose);
    assert_eq!(decoded.2, 1);
    assert_eq!(decoded.3, resource.content_hash);
}

#[test]
fn schema_and_error_codes_are_stable() {
    let f = setup();
    let resource = draft(&f.env, 90, AccessPolicy::ProjectMembers);
    f.client.propose_resource(
        &f.curator,
        &f.tenant,
        &f.project,
        &resource,
        &hash(&f.env, 230),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_resource(&f.member, &f.tenant, &f.project, &resource.resource_id)
            .schema_version,
        SCHEMA_VERSION
    );
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::InvalidIntegrity as u32, 19);
    assert_eq!(
        f.client
            .try_resources_page(&f.member, &f.tenant, &f.project, 0, 51)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
}
