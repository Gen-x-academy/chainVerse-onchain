extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    AttributionState, CapstoneContributorAttribution, CapstoneContributorAttributionClient,
    ContractError, Role, Taxonomy,
};

const NOW: u64 = 1_000;

struct Fixture {
    env: Env,
    client: CapstoneContributorAttributionClient<'static>,
    admin: Address,
    coordinator: Address,
    contributor: Address,
    reviewer: Address,
    reviewer2: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn id(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneContributorAttribution, ());
    let client = CapstoneContributorAttributionClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    let coordinator = Address::generate(&env);
    let contributor = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let reviewer2 = Address::generate(&env);
    client.grant_role(&admin, &tenant, &project, &coordinator, &Role::Coordinator);
    client.grant_role(&admin, &tenant, &project, &contributor, &Role::Contributor);
    client.grant_role(&admin, &tenant, &project, &reviewer, &Role::Reviewer);
    client.grant_role(&admin, &tenant, &project, &reviewer2, &Role::Reviewer);
    Fixture {
        env,
        client,
        admin,
        coordinator,
        contributor,
        reviewer,
        reviewer2,
        tenant,
        project,
    }
}

fn default_policy(f: &Fixture) {
    f.client.configure_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 80),
        &0,
        &1,
        &16,
    );
}

#[test]
fn attribution_lifecycle_links_output_with_typed_event() {
    let f = setup();
    default_policy(&f);
    let record_id = id(&f.env, 10);
    let contribution_hash = id(&f.env, 40);
    let record = f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 81),
        &0,
        &Taxonomy::Writing,
        &contribution_hash,
    );
    assert_eq!(record.state, AttributionState::Declared);
    assert_eq!(record.version, 1);

    let approved = f.client.approve_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 82),
        &1,
    );
    assert_eq!(approved.state, AttributionState::Approved);

    let output_hash = id(&f.env, 41);
    let linked = f.client.link_output(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 83),
        &2,
        &output_hash,
    );
    assert_eq!(linked.state, AttributionState::Linked);
    assert_eq!(linked.output_hash, output_hash);
    assert_eq!(linked.version, 3);

    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("ATTRLINK"));
    let (event_record, event_hash, state): (BytesN<32>, BytesN<32>, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(event_record, record_id);
    assert_eq!(event_hash, output_hash);
    assert_eq!(state, 5);
}

#[test]
fn contributor_cannot_approve_own_attribution() {
    let f = setup();
    default_policy(&f);
    f.client.grant_role(
        &f.admin,
        &f.tenant,
        &f.project,
        &f.contributor,
        &Role::Reviewer,
    );
    let record_id = id(&f.env, 11);
    f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 84),
        &0,
        &Taxonomy::Software,
        &id(&f.env, 42),
    );
    assert_eq!(
        f.client
            .try_approve_attribution(
                &f.contributor,
                &f.tenant,
                &f.project,
                &record_id,
                &id(&f.env, 85),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::SelfApprovalNotAllowed
    );
}

#[test]
fn dispute_blocks_link_until_resolved() {
    let f = setup();
    default_policy(&f);
    let record_id = id(&f.env, 12);
    f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 90),
        &0,
        &Taxonomy::DataCuration,
        &id(&f.env, 43),
    );
    f.client.approve_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 91),
        &1,
    );
    let disputed = f.client.dispute_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 92),
        &2,
        &id(&f.env, 44),
    );
    assert_eq!(disputed.state, AttributionState::Disputed);
    assert_eq!(
        f.client
            .try_link_output(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &record_id,
                &id(&f.env, 93),
                &3,
                &id(&f.env, 45),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    let resolved = f.client.resolve_dispute(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 94),
        &3,
        &false,
        &id(&f.env, 46),
    );
    assert_eq!(resolved.state, AttributionState::Approved);
    let linked = f.client.link_output(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 95),
        &4,
        &id(&f.env, 47),
    );
    assert_eq!(linked.state, AttributionState::Linked);

    let second = id(&f.env, 13);
    f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 96),
        &0,
        &Taxonomy::Validation,
        &id(&f.env, 48),
    );
    f.client.approve_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 97),
        &1,
    );
    f.client.dispute_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 98),
        &2,
        &id(&f.env, 49),
    );
    let rejected = f.client.resolve_dispute(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 99),
        &3,
        &true,
        &id(&f.env, 50),
    );
    assert_eq!(rejected.state, AttributionState::Rejected);
}

#[test]
fn two_approvals_are_independent_and_non_duplicable() {
    let f = setup();
    f.client.configure_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 100),
        &0,
        &2,
        &16,
    );
    let record_id = id(&f.env, 14);
    f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 101),
        &0,
        &Taxonomy::Methodology,
        &id(&f.env, 51),
    );
    let first = f.client.approve_attribution(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 102),
        &1,
    );
    assert_eq!(first.state, AttributionState::Declared);
    assert_eq!(first.approvals, 1);
    assert_eq!(
        f.client
            .try_approve_attribution(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &record_id,
                &id(&f.env, 103),
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyApproved
    );
    let second = f.client.approve_attribution(
        &f.reviewer2,
        &f.tenant,
        &f.project,
        &record_id,
        &id(&f.env, 104),
        &2,
    );
    assert_eq!(second.state, AttributionState::Approved);
    assert_eq!(second.approvals, 2);
    assert!(f.client.has_approved(&f.tenant, &f.project, &record_id, &f.reviewer));
    assert!(!f.client.has_approved(&f.tenant, &f.project, &record_id, &f.contributor));
}

#[test]
fn replay_is_cached_and_stale_writes_fail() {
    let f = setup();
    default_policy(&f);
    let record_id = id(&f.env, 15);
    let request_id = id(&f.env, 110);
    let first = f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &request_id,
        &0,
        &Taxonomy::Supervision,
        &id(&f.env, 52),
    );
    let replay = f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &record_id,
        &request_id,
        &0,
        &Taxonomy::Supervision,
        &id(&f.env, 52),
    );
    assert_eq!(first, replay);
    assert_eq!(f.client.record_count(&f.tenant, &f.project), 1);
    assert_eq!(f.env.events().all().len(), 0);

    assert_eq!(
        f.client
            .try_declare_contribution(
                &f.contributor,
                &f.tenant,
                &f.project,
                &record_id,
                &request_id,
                &8,
                &Taxonomy::Supervision,
                &id(&f.env, 52),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(
        f.client
            .try_approve_attribution(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &record_id,
                &id(&f.env, 111),
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
}

#[test]
fn invalid_hashes_and_policy_values_are_rejected() {
    let f = setup();
    default_policy(&f);
    assert_eq!(
        f.client
            .try_declare_contribution(
                &f.contributor,
                &f.tenant,
                &f.project,
                &id(&f.env, 16),
                &id(&f.env, 120),
                &0,
                &Taxonomy::FundingAcquisition,
                &id(&f.env, 0),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidHash
    );

    let g = setup();
    assert_eq!(
        g.client
            .try_configure_policy(
                &g.coordinator,
                &g.tenant,
                &g.project,
                &id(&g.env, 121),
                &0,
                &0,
                &16,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPolicy
    );
    assert_eq!(
        g.client
            .try_configure_policy(
                &g.coordinator,
                &g.tenant,
                &g.project,
                &id(&g.env, 122),
                &0,
                &1,
                &17,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPolicy
    );
}

#[test]
fn record_index_respects_policy_limit() {
    let f = setup();
    f.client.configure_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 130),
        &0,
        &1,
        &4,
    );
    for n in 1u8..=4u8 {
        f.client.declare_contribution(
            &f.contributor,
            &f.tenant,
            &f.project,
            &id(&f.env, n),
            &id(&f.env, 140 + n),
            &0,
            &Taxonomy::Software,
            &id(&f.env, 160 + n),
        );
    }
    assert_eq!(f.client.record_count(&f.tenant, &f.project), 4);
    assert_eq!(
        f.client
            .try_declare_contribution(
                &f.contributor,
                &f.tenant,
                &f.project,
                &id(&f.env, 5),
                &id(&f.env, 150),
                &0,
                &Taxonomy::Software,
                &id(&f.env, 170),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::IndexFull
    );
}

#[test]
fn cross_tenant_contribution_is_denied() {
    let f = setup();
    default_policy(&f);
    let other_tenant = id(&f.env, 93);
    assert_eq!(
        f.client
            .try_declare_contribution(
                &f.contributor,
                &other_tenant,
                &f.project,
                &id(&f.env, 17),
                &id(&f.env, 180),
                &0,
                &Taxonomy::Writing,
                &id(&f.env, 53),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(!f.client.has_role(&other_tenant, &f.project, &f.contributor, &Role::Contributor));
}

#[test]
fn resource_budget_stays_within_transaction_limits() {
    let f = setup();
    default_policy(&f);
    f.env.cost_estimate().budget().reset_default();
    f.client.declare_contribution(
        &f.contributor,
        &f.tenant,
        &f.project,
        &id(&f.env, 18),
        &id(&f.env, 190),
        &0,
        &Taxonomy::Conceptualization,
        &id(&f.env, 54),
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 100_000_000);
    assert!(budget.memory_bytes_cost() < 41_943_040);
}

#[test]
fn version_and_initialization_are_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}
