extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneProjectRisks, CapstoneProjectRisksClient, Category, ContractError, DataKey,
    RequestRecord, RiskInput, RiskRecord, RiskStatus, Role, RoleGrant, Visibility,
};

const NOW: u64 = 1_000_000;
const DAY: u64 = 86_400;

struct Fixture {
    env: Env,
    client: CapstoneProjectRisksClient<'static>,
    admin: Address,
    manager: Address,
    reviewer: Address,
    viewer: Address,
    owner: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn id(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn id2(env: &Env, hi: u8, lo: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = hi;
    bytes[31] = lo;
    BytesN::from_array(env, &bytes)
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneProjectRisks, ());
    let client = CapstoneProjectRisksClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let viewer = Address::generate(&env);
    let owner = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    client.set_role(
        &admin,
        &manager,
        &tenant,
        &project,
        &Role::Manager,
        &true,
        &0,
    );
    client.set_role(
        &admin,
        &reviewer,
        &tenant,
        &project,
        &Role::Reviewer,
        &true,
        &0,
    );
    client.set_role(&admin, &viewer, &tenant, &project, &Role::Viewer, &true, &0);
    Fixture {
        env,
        client,
        admin,
        manager,
        reviewer,
        viewer,
        owner,
        tenant,
        project,
    }
}

fn input(f: &Fixture, risk: u8) -> RiskInput {
    RiskInput {
        risk_id: id(&f.env, risk),
        category: Category::Schedule,
        likelihood: 3,
        impact: 4,
        owner: f.owner.clone(),
        mitigation_hash: id(&f.env, 100),
        contingency_hash: id(&f.env, 101),
        next_review_at: NOW + DAY,
        visibility: Visibility::Team,
    }
}

fn register(f: &Fixture, request: u8, risk: u8) -> u32 {
    f.client.register_risk(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &input(f, risk),
    )
}

fn get(f: &Fixture, risk: u8) -> RiskRecord {
    f.client
        .get_risk(&f.manager, &f.tenant, &f.project, &id(&f.env, risk))
}

fn transition(f: &Fixture, request: u8, risk: u8, version: u32, to: RiskStatus) -> u32 {
    f.client.transition_status(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &id(&f.env, risk),
        &version,
        &to,
    )
}

#[test]
fn initialization_is_one_time_and_version_is_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}

#[test]
fn uninitialized_contract_rejects_role_changes() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneProjectRisks, ());
    let client = CapstoneProjectRisksClient::new(&env, &contract_id);
    let who = Address::generate(&env);
    let err = client
        .try_set_role(
            &who,
            &who,
            &id(&env, 1),
            &id(&env, 2),
            &Role::Manager,
            &true,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::NotInitialized);
}

#[test]
fn register_stores_categorized_bounded_record() {
    let f = setup();
    assert_eq!(register(&f, 10, 1), 1);
    let record = get(&f, 1);
    assert_eq!(record.category, Category::Schedule);
    assert_eq!(record.likelihood, 3);
    assert_eq!(record.impact, 4);
    assert_eq!(record.score, 12);
    assert_eq!(record.owner, f.owner);
    assert_eq!(record.mitigation_hash, id(&f.env, 100));
    assert_eq!(record.contingency_hash, id(&f.env, 101));
    assert_eq!(record.next_review_at, NOW + DAY);
    assert_eq!(record.last_reviewed_at, 0);
    assert_eq!(record.status, RiskStatus::Identified);
    assert_eq!(record.created_at, NOW);
    assert_eq!(record.version, 1);
}

#[test]
fn authorization_tree_is_deny_by_default() {
    let f = setup();
    register(&f, 10, 1);
    let stranger = Address::generate(&f.env);

    // Viewer and reviewer cannot register risks.
    for actor in [&f.viewer, &f.reviewer, &stranger] {
        let err = f
            .client
            .try_register_risk(actor, &f.tenant, &f.project, &id(&f.env, 11), &input(&f, 2))
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }

    // Only managers update, transition or reassign.
    for actor in [&f.viewer, &f.reviewer, &stranger] {
        let err = f
            .client
            .try_update_assessment(
                actor,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 1),
                &1,
                &2,
                &2,
                &id(&f.env, 110),
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
        let err = f
            .client
            .try_transition_status(
                actor,
                &f.tenant,
                &f.project,
                &id(&f.env, 13),
                &id(&f.env, 1),
                &1,
                &RiskStatus::Mitigating,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
        let err = f
            .client
            .try_reassign_owner(
                actor,
                &f.tenant,
                &f.project,
                &id(&f.env, 14),
                &id(&f.env, 1),
                &1,
                &stranger,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
        let err = f
            .client
            .try_update_contingency(
                actor,
                &f.tenant,
                &f.project,
                &id(&f.env, 15),
                &id(&f.env, 1),
                &1,
                &id(&f.env, 111),
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }

    // Reviewers may record reviews, viewers and strangers may not.
    let review = |actor: &Address, request: u8| {
        f.client.try_record_review(
            actor,
            &f.tenant,
            &f.project,
            &id(&f.env, request),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 120),
            &(NOW + 5 * DAY),
        )
    };
    assert_eq!(
        review(&f.viewer, 20).unwrap_err().unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        review(&stranger, 21).unwrap_err().unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(review(&f.reviewer, 22).unwrap().unwrap(), 2);

    // Reads need an active role.
    assert_eq!(
        f.client
            .try_get_risk(&stranger, &f.tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(f
        .client
        .try_get_risk(&f.viewer, &f.tenant, &f.project, &id(&f.env, 1))
        .is_ok());

    // Role administration belongs to the administrator alone.
    let err = f
        .client
        .try_set_role(
            &f.manager,
            &stranger,
            &f.tenant,
            &f.project,
            &Role::Manager,
            &true,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn unauthorized_calls_do_not_reveal_whether_a_risk_exists() {
    let f = setup();
    register(&f, 10, 1);
    let stranger = Address::generate(&f.env);
    let existing = f
        .client
        .try_update_contingency(
            &stranger,
            &f.tenant,
            &f.project,
            &id(&f.env, 30),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 111),
        )
        .unwrap_err()
        .unwrap();
    let missing = f
        .client
        .try_update_contingency(
            &stranger,
            &f.tenant,
            &f.project,
            &id(&f.env, 31),
            &id(&f.env, 99),
            &1,
            &id(&f.env, 111),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(existing, ContractError::Unauthorized);
    assert_eq!(missing, ContractError::Unauthorized);
}

#[test]
fn tenant_and_project_boundaries_are_enforced() {
    let f = setup();
    register(&f, 10, 1);
    let other_tenant = id(&f.env, 50);
    let other_project = id(&f.env, 51);

    let err = f
        .client
        .try_get_risk(&f.manager, &other_tenant, &f.project, &id(&f.env, 1))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
    let err = f
        .client
        .try_get_risk(&f.manager, &f.tenant, &other_project, &id(&f.env, 1))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);

    // A manager of another tenant sees and writes only their own namespace.
    let other_manager = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &other_manager,
        &other_tenant,
        &f.project,
        &Role::Manager,
        &true,
        &0,
    );
    let created = f.client.register_risk(
        &other_manager,
        &other_tenant,
        &f.project,
        &id(&f.env, 10),
        &input(&f, 1),
    );
    assert_eq!(created, 1);
    // Same request id and risk id in the other tenant do not collide.
    assert_eq!(get(&f, 1).tenant_id, f.tenant);
    let theirs = f
        .client
        .get_risk(&other_manager, &other_tenant, &f.project, &id(&f.env, 1));
    assert_eq!(theirs.tenant_id, other_tenant);
}

#[test]
fn replayed_request_returns_recorded_result_without_side_effects() {
    let f = setup();
    let first = register(&f, 10, 1);
    let replay = register(&f, 10, 1);
    assert_eq!(first, replay);
    assert_eq!(f.env.events().all().len(), 0);
    let page = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &0, &32);
    assert_eq!(page.ids.len(), 1);

    // A replay still answers after later mutations moved the version on.
    let v2 = transition(&f, 11, 1, 1, RiskStatus::Mitigating);
    assert_eq!(v2, 2);
    assert_eq!(register(&f, 10, 1), 1);
    assert_eq!(get(&f, 1).version, 2);
    assert_eq!(transition(&f, 11, 1, 1, RiskStatus::Mitigating), 2);
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(get(&f, 1).version, 2);
}

#[test]
fn request_id_reuse_with_other_payload_or_actor_is_rejected() {
    let f = setup();
    register(&f, 10, 1);
    let err = f
        .client
        .try_register_risk(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &input(&f, 2),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);

    let second_manager = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &second_manager,
        &f.tenant,
        &f.project,
        &Role::Manager,
        &true,
        &0,
    );
    let err = f
        .client
        .try_register_risk(
            &second_manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &input(&f, 1),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
}

#[test]
fn duplicate_risk_ids_under_new_requests_cannot_double_register() {
    let f = setup();
    register(&f, 10, 1);
    let err = f
        .client
        .try_register_risk(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 11),
            &input(&f, 1),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyExists);
    let page = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &0, &32);
    assert_eq!(page.ids.len(), 1);
}

#[test]
fn stale_versions_are_rejected_so_concurrent_writers_cannot_both_win() {
    let f = setup();
    register(&f, 10, 1);
    let v2 = f.client.update_assessment(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &1,
        &5,
        &5,
        &id(&f.env, 130),
    );
    assert_eq!(v2, 2);
    // A competing writer that read version 1 loses with an actionable error.
    let err = f
        .client
        .try_update_assessment(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 12),
            &id(&f.env, 1),
            &1,
            &1,
            &1,
            &id(&f.env, 131),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::StaleVersion);
    let record = get(&f, 1);
    assert_eq!(record.score, 25);
    assert_eq!(record.mitigation_hash, id(&f.env, 130));
    // The failed attempt left no request record behind, so a retry with the
    // fresh version under the same request id succeeds.
    let v3 = f.client.update_assessment(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &id(&f.env, 1),
        &2,
        &1,
        &1,
        &id(&f.env, 131),
    );
    assert_eq!(v3, 3);
}

#[test]
fn status_transitions_follow_the_documented_lifecycle() {
    let f = setup();
    register(&f, 10, 1);
    assert_eq!(transition(&f, 11, 1, 1, RiskStatus::Mitigating), 2);
    assert_eq!(transition(&f, 12, 1, 2, RiskStatus::Accepted), 3);
    assert_eq!(transition(&f, 13, 1, 3, RiskStatus::Mitigating), 4);
    assert_eq!(get(&f, 1).status, RiskStatus::Mitigating);

    // Same-state and backwards transitions are rejected.
    let same = f
        .client
        .try_transition_status(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 14),
            &id(&f.env, 1),
            &4,
            &RiskStatus::Mitigating,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(same, ContractError::InvalidTransition);
    let back = f
        .client
        .try_transition_status(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 15),
            &id(&f.env, 1),
            &4,
            &RiskStatus::Identified,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(back, ContractError::InvalidTransition);

    assert_eq!(transition(&f, 16, 1, 4, RiskStatus::Closed), 5);
    let reopen = f
        .client
        .try_transition_status(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 17),
            &id(&f.env, 1),
            &5,
            &RiskStatus::Mitigating,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(reopen, ContractError::InvalidTransition);

    // Materialized is terminal as well.
    register(&f, 20, 2);
    assert_eq!(transition(&f, 21, 2, 1, RiskStatus::Materialized), 2);
    let after = f
        .client
        .try_transition_status(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 22),
            &id(&f.env, 2),
            &2,
            &RiskStatus::Closed,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(after, ContractError::InvalidTransition);
}

#[test]
fn stale_transition_is_reported_before_an_invalid_one() {
    let f = setup();
    register(&f, 10, 1);
    transition(&f, 11, 1, 1, RiskStatus::Mitigating);
    let err = f
        .client
        .try_transition_status(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 12),
            &id(&f.env, 1),
            &1,
            &RiskStatus::Identified,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::StaleVersion);
}

#[test]
fn terminal_risks_reject_edits_reviews_and_reassignment() {
    let f = setup();
    register(&f, 10, 1);
    transition(&f, 11, 1, 1, RiskStatus::Closed);
    let edit = f
        .client
        .try_update_assessment(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 12),
            &id(&f.env, 1),
            &2,
            &2,
            &2,
            &id(&f.env, 140),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(edit, ContractError::TerminalRisk);
    let review = f
        .client
        .try_record_review(
            &f.reviewer,
            &f.tenant,
            &f.project,
            &id(&f.env, 13),
            &id(&f.env, 1),
            &2,
            &id(&f.env, 141),
            &(NOW + 2 * DAY),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(review, ContractError::TerminalRisk);
    let owner = f
        .client
        .try_reassign_owner(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 14),
            &id(&f.env, 1),
            &2,
            &f.reviewer,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(owner, ContractError::TerminalRisk);
}

#[test]
fn review_dates_are_validated_and_overdue_state_is_derived() {
    let f = setup();
    let mut bad = input(&f, 1);
    bad.next_review_at = NOW;
    assert_eq!(
        f.client
            .try_register_risk(&f.manager, &f.tenant, &f.project, &id(&f.env, 10), &bad)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
    bad.next_review_at = NOW + 367 * DAY;
    assert_eq!(
        f.client
            .try_register_risk(&f.manager, &f.tenant, &f.project, &id(&f.env, 10), &bad)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
    register(&f, 11, 1);
    let due = |f: &Fixture| {
        f.client
            .review_due(&f.viewer, &f.tenant, &f.project, &id(&f.env, 1))
    };
    assert!(!due(&f));
    f.env.ledger().set_timestamp(NOW + 2 * DAY);
    assert!(due(&f));
    let v2 = f.client.record_review(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &id(&f.env, 1),
        &1,
        &id(&f.env, 150),
        &(NOW + 9 * DAY),
    );
    assert_eq!(v2, 2);
    let record = get(&f, 1);
    assert_eq!(record.last_reviewed_at, NOW + 2 * DAY);
    assert_eq!(record.next_review_at, NOW + 9 * DAY);
    assert_eq!(record.review_hash, id(&f.env, 150));
    assert!(!due(&f));
    // A review cannot schedule the next one in the past.
    let err = f
        .client
        .try_record_review(
            &f.reviewer,
            &f.tenant,
            &f.project,
            &id(&f.env, 13),
            &id(&f.env, 1),
            &2,
            &id(&f.env, 151),
            &(NOW + DAY),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidInput);
    // Closed risks are never reported as due.
    transition(&f, 14, 1, 2, RiskStatus::Closed);
    f.env.ledger().set_timestamp(NOW + 30 * DAY);
    assert!(!due(&f));
}

#[test]
fn role_expiry_and_revocation_deny_access() {
    let f = setup();
    register(&f, 10, 1);
    let temp = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &temp,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &(NOW + 100),
    );
    assert!(f
        .client
        .try_get_risk(&temp, &f.tenant, &f.project, &id(&f.env, 1))
        .is_ok());
    f.env.ledger().set_timestamp(NOW + 100);
    assert_eq!(
        f.client
            .try_get_risk(&temp, &f.tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::RoleExpired
    );
    // A grant that is already expired cannot be created.
    assert_eq!(
        f.client
            .try_set_role(
                &f.admin,
                &temp,
                &f.tenant,
                &f.project,
                &Role::Viewer,
                &true,
                &(NOW + 50)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
    // Revocation yields the deny-by-default error.
    f.client.set_role(
        &f.admin,
        &f.viewer,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &false,
        &0,
    );
    assert_eq!(
        f.client
            .try_get_risk(&f.viewer, &f.tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn restricted_risks_are_hidden_from_plain_viewers() {
    let f = setup();
    let mut restricted = input(&f, 1);
    restricted.visibility = Visibility::Restricted;
    f.client.register_risk(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 10),
        &restricted,
    );
    register(&f, 11, 2);

    // Viewer cannot tell the restricted risk exists.
    assert_eq!(
        f.client
            .try_get_risk(&f.viewer, &f.tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    let viewer_page = f
        .client
        .list_risks(&f.viewer, &f.tenant, &f.project, &0, &32);
    assert_eq!(viewer_page.ids.len(), 1);
    assert_eq!(viewer_page.ids.get(0).unwrap(), id(&f.env, 2));

    // Managers, reviewers and the named owner can.
    let manager_page = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &0, &32);
    assert_eq!(manager_page.ids.len(), 2);
    assert!(f
        .client
        .try_get_risk(&f.reviewer, &f.tenant, &f.project, &id(&f.env, 1))
        .is_ok());
    f.client.set_role(
        &f.admin,
        &f.owner,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &0,
    );
    assert!(f
        .client
        .try_get_risk(&f.owner, &f.tenant, &f.project, &id(&f.env, 1))
        .is_ok());
}

#[test]
fn contingency_and_owner_updates_bump_versions() {
    let f = setup();
    register(&f, 10, 1);
    let v2 = f.client.update_contingency(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &1,
        &id(&f.env, 160),
    );
    assert_eq!(v2, 2);
    let record = get(&f, 1);
    assert_eq!(record.contingency_hash, id(&f.env, 160));
    assert_eq!(record.score, 12);
    let new_owner = Address::generate(&f.env);
    let v3 = f.client.reassign_owner(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &id(&f.env, 1),
        &2,
        &new_owner,
    );
    assert_eq!(v3, 3);
    assert_eq!(get(&f, 1).owner, new_owner);
}

#[test]
fn inputs_are_validated() {
    let f = setup();
    let mut bad = input(&f, 1);
    bad.likelihood = 0;
    let try_register = |value: &RiskInput, request: u8| {
        f.client
            .try_register_risk(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, request),
                value,
            )
            .unwrap_err()
            .unwrap()
    };
    assert_eq!(try_register(&bad, 10), ContractError::InvalidInput);
    bad.likelihood = 6;
    assert_eq!(try_register(&bad, 11), ContractError::InvalidInput);
    bad.likelihood = 3;
    bad.impact = 0;
    assert_eq!(try_register(&bad, 12), ContractError::InvalidInput);
    bad.impact = 4;
    bad.mitigation_hash = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(try_register(&bad, 13), ContractError::InvalidInput);
    bad.mitigation_hash = id(&f.env, 100);
    bad.contingency_hash = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(try_register(&bad, 14), ContractError::InvalidInput);
    bad.contingency_hash = id(&f.env, 101);
    bad.risk_id = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(try_register(&bad, 15), ContractError::InvalidInput);

    register(&f, 16, 1);
    // An empty update is rejected and an out-of-range score is rejected.
    let err = f
        .client
        .try_update_assessment(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 17),
            &id(&f.env, 1),
            &1,
            &9,
            &1,
            &id(&f.env, 170),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidInput);
    assert_eq!(get(&f, 1).version, 1);
}

#[test]
fn storage_and_page_bounds_are_enforced() {
    let f = setup();
    for n in 0u32..128 {
        let risk = id2(&f.env, 7, n as u8);
        let request = id2(&f.env, 8, n as u8);
        let mut value = input(&f, 1);
        value.risk_id = risk;
        f.client
            .register_risk(&f.manager, &f.tenant, &f.project, &request, &value);
    }
    let mut value = input(&f, 1);
    value.risk_id = id2(&f.env, 7, 200);
    let err = f
        .client
        .try_register_risk(
            &f.manager,
            &f.tenant,
            &f.project,
            &id2(&f.env, 8, 200),
            &value,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::LimitReached);

    let first = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &0, &32);
    assert_eq!(first.ids.len(), 32);
    assert_eq!(first.next, 32);
    assert!(first.has_more);
    let last = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &96, &32);
    assert_eq!(last.ids.len(), 32);
    assert_eq!(last.next, 128);
    assert!(!last.has_more);
    for (from, limit) in [(0u32, 0u32), (0, 33), (129, 1)] {
        let err = f
            .client
            .try_list_risks(&f.manager, &f.tenant, &f.project, &from, &limit)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::InvalidPage);
    }
}

#[test]
fn role_grants_are_stored_with_expiry_metadata_only() {
    let f = setup();
    f.client.set_role(
        &f.admin,
        &f.viewer,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &(NOW + 500),
    );
    let stored: RoleGrant = f.env.as_contract(&f.client.address, || {
        f.env
            .storage()
            .persistent()
            .get(&DataKey::Role(
                f.viewer.clone(),
                f.tenant.clone(),
                f.project.clone(),
                Role::Viewer,
            ))
            .unwrap()
    });
    assert_eq!(
        stored,
        RoleGrant {
            active: true,
            expires_at: NOW + 500
        }
    );
}

#[test]
fn events_follow_the_documented_schema_without_private_payloads() {
    let f = setup();
    register(&f, 10, 1);
    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    assert_eq!(topics.len(), 1);
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_REG"));
    let (tenant, project, risk, category, score, version): (
        BytesN<32>,
        BytesN<32>,
        BytesN<32>,
        u32,
        u32,
        u32,
    ) = data.try_into_val(&f.env).unwrap();
    assert_eq!(tenant, f.tenant);
    assert_eq!(project, f.project);
    assert_eq!(risk, id(&f.env, 1));
    assert_eq!(category, Category::Schedule as u32);
    assert_eq!(score, 12);
    assert_eq!(version, 1);

    transition(&f, 11, 1, 1, RiskStatus::Mitigating);
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_STS"));
    let (_, _, _, from, to, version): (BytesN<32>, BytesN<32>, BytesN<32>, u32, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((from, to, version), (1, 2, 2));

    f.client.update_assessment(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &id(&f.env, 1),
        &2,
        &2,
        &2,
        &id(&f.env, 180),
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_UPD"));
    let (_, _, _, score, version): (BytesN<32>, BytesN<32>, BytesN<32>, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((score, version), (4, 3));

    f.client.record_review(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &id(&f.env, 13),
        &id(&f.env, 1),
        &3,
        &id(&f.env, 181),
        &(NOW + 3 * DAY),
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_REV"));
    let (_, _, _, next, version): (BytesN<32>, BytesN<32>, BytesN<32>, u64, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((next, version), (NOW + 3 * DAY, 4));

    let new_owner = Address::generate(&f.env);
    f.client.reassign_owner(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 14),
        &id(&f.env, 1),
        &4,
        &new_owner,
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_OWN"));
    let (_, _, _, owner, version): (BytesN<32>, BytesN<32>, BytesN<32>, Address, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((owner, version), (new_owner, 5));

    f.client.set_role(
        &f.admin,
        &f.viewer,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &0,
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSK_ROLE"));
    let (_, _, account, role, active, expires): (BytesN<32>, BytesN<32>, Address, u32, bool, u64) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(
        (account, role, active, expires),
        (f.viewer.clone(), 3, true, 0)
    );
}

#[test]
fn failed_calls_emit_no_events_and_change_nothing() {
    let f = setup();
    register(&f, 10, 1);
    let _ = f.client.try_transition_status(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &1,
        &RiskStatus::Identified,
    );
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(get(&f, 1).version, 1);
    assert_eq!(get(&f, 1).status, RiskStatus::Identified);
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::AlreadyInitialized as u32, 1);
    assert_eq!(ContractError::NotInitialized as u32, 2);
    assert_eq!(ContractError::Unauthorized as u32, 3);
    assert_eq!(ContractError::RequestConflict as u32, 4);
    assert_eq!(ContractError::StaleVersion as u32, 5);
    assert_eq!(ContractError::NotFound as u32, 6);
    assert_eq!(ContractError::AlreadyExists as u32, 7);
    assert_eq!(ContractError::InvalidInput as u32, 8);
    assert_eq!(ContractError::InvalidTransition as u32, 9);
    assert_eq!(ContractError::LimitReached as u32, 10);
    assert_eq!(ContractError::InvalidPage as u32, 11);
    assert_eq!(ContractError::ArithmeticError as u32, 12);
    assert_eq!(ContractError::RoleExpired as u32, 13);
    assert_eq!(ContractError::TerminalRisk as u32, 14);
}

#[test]
fn enum_discriminants_are_stable_for_upgrades() {
    assert_eq!(
        [
            Role::Manager as u32,
            Role::Reviewer as u32,
            Role::Viewer as u32
        ],
        [1, 2, 3]
    );
    assert_eq!(
        [
            Category::Schedule as u32,
            Category::Budget as u32,
            Category::Scope as u32,
            Category::Technical as u32,
            Category::People as u32,
            Category::External as u32
        ],
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        [
            RiskStatus::Identified as u32,
            RiskStatus::Mitigating as u32,
            RiskStatus::Accepted as u32,
            RiskStatus::Closed as u32,
            RiskStatus::Materialized as u32
        ],
        [1, 2, 3, 4, 5]
    );
    assert_eq!(
        [Visibility::Team as u32, Visibility::Restricted as u32],
        [1, 2]
    );
}

#[test]
fn records_written_by_an_earlier_deployment_remain_readable() {
    let f = setup();
    let legacy = RiskRecord {
        risk_id: id(&f.env, 77),
        tenant_id: f.tenant.clone(),
        project_id: f.project.clone(),
        category: Category::Budget,
        likelihood: 2,
        impact: 5,
        score: 10,
        owner: f.owner.clone(),
        mitigation_hash: id(&f.env, 78),
        contingency_hash: id(&f.env, 79),
        review_hash: BytesN::from_array(&f.env, &[0u8; 32]),
        next_review_at: NOW + DAY,
        last_reviewed_at: 0,
        status: RiskStatus::Accepted,
        visibility: Visibility::Team,
        created_at: 5,
        updated_at: 6,
        version: 4,
    };
    f.env.as_contract(&f.client.address, || {
        f.env.storage().persistent().set(
            &DataKey::Risk(f.tenant.clone(), f.project.clone(), id(&f.env, 77)),
            &legacy,
        );
        f.env.storage().persistent().set(
            &DataKey::Request(f.tenant.clone(), f.project.clone(), id(&f.env, 90)),
            &RequestRecord {
                input_hash: id(&f.env, 91),
                result: 4,
            },
        );
    });
    assert_eq!(get(&f, 77), legacy);
    // The stored request record is honoured: a different payload is a conflict.
    let err = f
        .client
        .try_register_risk(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 90),
            &input(&f, 3),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
    // And the legacy record continues through the lifecycle at its version.
    assert_eq!(transition(&f, 92, 77, 4, RiskStatus::Closed), 5);
}

#[test]
fn resource_budgets_stay_within_transaction_limits() {
    let f = setup();
    f.env.cost_estimate().budget().reset_default();
    register(&f, 10, 1);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    f.env.cost_estimate().budget().reset_default();
    transition(&f, 11, 1, 1, RiskStatus::Mitigating);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    // The widest read, one full page, stays inside the complex-call class.
    for n in 0u32..31 {
        let mut value = input(&f, 1);
        value.risk_id = id2(&f.env, 9, n as u8);
        f.client.register_risk(
            &f.manager,
            &f.tenant,
            &f.project,
            &id2(&f.env, 10, n as u8),
            &value,
        );
    }
    f.env.cost_estimate().budget().reset_default();
    let page = f
        .client
        .list_risks(&f.manager, &f.tenant, &f.project, &0, &32);
    assert_eq!(page.ids.len(), 32);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 25_000_000);
    assert!(budget.memory_bytes_cost() < 10_485_760);
}
