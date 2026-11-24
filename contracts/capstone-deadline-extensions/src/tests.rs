extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneDeadlineExtensions, CapstoneDeadlineExtensionsClient, ContractError, DataKey,
    ExtensionInput, ExtensionRecord, ExtensionState, RequestRecord, Role, RoleGrant,
};

const NOW: u64 = 5_000_000;
const DAY: u64 = 86_400;

struct Fixture {
    env: Env,
    client: CapstoneDeadlineExtensionsClient<'static>,
    admin: Address,
    manager: Address,
    requester: Address,
    approver: Address,
    approver2: Address,
    viewer: Address,
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

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneDeadlineExtensions, ());
    let client = CapstoneDeadlineExtensionsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let requester = Address::generate(&env);
    let approver = Address::generate(&env);
    let approver2 = Address::generate(&env);
    let viewer = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    for (who, role) in [
        (&manager, Role::Manager),
        (&requester, Role::Requester),
        (&approver, Role::Approver),
        (&approver2, Role::Approver),
        (&viewer, Role::Viewer),
    ] {
        client.set_role(&admin, who, &tenant, &project, &role, &true, &0);
    }
    Fixture {
        env,
        client,
        admin,
        manager,
        requester,
        approver,
        approver2,
        viewer,
        tenant,
        project,
    }
}

fn input(f: &Fixture, ext: u8, target: u8) -> ExtensionInput {
    ExtensionInput {
        extension_id: id(&f.env, ext),
        target_id: id(&f.env, target),
        reason_hash: id(&f.env, 100),
        impact_hash: id(&f.env, 101),
        current_deadline: NOW + 20 * DAY,
        proposed_deadline: NOW + 34 * DAY,
        funding_delta: 2_500,
        approvals_required: 2,
        expires_at: NOW + 10 * DAY,
    }
}

fn submit(f: &Fixture, request: u8, ext: u8, target: u8) -> u32 {
    f.client.submit_request(
        &f.requester,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &input(f, ext, target),
    )
}

fn get(f: &Fixture, ext: u8) -> ExtensionRecord {
    f.client
        .get_extension(&f.viewer, &f.tenant, &f.project, &id(&f.env, ext))
}

fn approve(f: &Fixture, who: &Address, request: u8, ext: u8, version: u32) -> u32 {
    f.client.approve(
        who,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &id(&f.env, ext),
        &version,
        &id(&f.env, request.wrapping_add(50)),
    )
}

fn try_approve(f: &Fixture, who: &Address, request: u8, ext: u8, version: u32) -> ContractError {
    f.client
        .try_approve(
            who,
            &f.tenant,
            &f.project,
            &id(&f.env, request),
            &id(&f.env, ext),
            &version,
            &id(&f.env, request.wrapping_add(50)),
        )
        .unwrap_err()
        .unwrap()
}

fn publish(f: &Fixture, request: u8, ext: u8, version: u32) -> u32 {
    f.client.publish_revision(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &id(&f.env, ext),
        &version,
        &id(&f.env, 120),
        &id(&f.env, 121),
    )
}

fn approved(f: &Fixture, ext: u8, target: u8, base: u8) -> u32 {
    submit(f, base, ext, target);
    approve(f, &f.approver, base + 1, ext, 1);
    approve(f, &f.approver2, base + 2, ext, 2)
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
fn submit_records_reasoned_request_with_impact_and_funding() {
    let f = setup();
    assert_eq!(submit(&f, 10, 1, 9), 1);
    let record = get(&f, 1);
    assert_eq!(record.state, ExtensionState::Requested);
    assert_eq!(record.requester, f.requester);
    assert_eq!(record.target_id, id(&f.env, 9));
    assert_eq!(record.reason_hash, id(&f.env, 100));
    assert_eq!(record.impact_hash, id(&f.env, 101));
    assert_eq!(record.current_deadline, NOW + 20 * DAY);
    assert_eq!(record.proposed_deadline, NOW + 34 * DAY);
    assert_eq!(record.funding_delta, 2_500);
    assert_eq!(record.approvals_required, 2);
    assert_eq!(record.approvals, 0);
    assert_eq!(record.decision_hash, zero(&f.env));
    assert_eq!(record.revised_schedule_hash, zero(&f.env));
    assert_eq!(record.created_at, NOW);
    assert_eq!(record.decided_at, 0);
    assert_eq!(record.version, 1);
    assert_eq!(
        f.client
            .open_request_for(&f.viewer, &f.tenant, &f.project, &id(&f.env, 9)),
        Some(id(&f.env, 1))
    );
}

#[test]
fn authorization_tree_is_deny_by_default() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    for (n, who) in [&f.manager, &f.approver, &f.viewer, &stranger]
        .iter()
        .enumerate()
    {
        let err = f
            .client
            .try_submit_request(
                who,
                &f.tenant,
                &f.project,
                &id(&f.env, 10 + n as u8),
                &input(&f, 1, 9),
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }
    submit(&f, 20, 1, 9);

    // Only approvers approve or reject.
    for (n, who) in [&f.manager, &f.requester, &f.viewer, &stranger]
        .iter()
        .enumerate()
    {
        assert_eq!(
            try_approve(&f, who, 30 + n as u8, 1, 1),
            ContractError::Unauthorized
        );
        let err = f
            .client
            .try_reject(
                who,
                &f.tenant,
                &f.project,
                &id(&f.env, 40 + n as u8),
                &id(&f.env, 1),
                &1,
                &id(&f.env, 130),
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }

    // Only the requester role withdraws, and only its own requests.
    for (n, who) in [&f.manager, &f.approver, &f.viewer, &stranger]
        .iter()
        .enumerate()
    {
        let err = f
            .client
            .try_withdraw(
                who,
                &f.tenant,
                &f.project,
                &id(&f.env, 50 + n as u8),
                &id(&f.env, 1),
                &1,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }
    let other_requester = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &other_requester,
        &f.tenant,
        &f.project,
        &Role::Requester,
        &true,
        &0,
    );
    let err = f
        .client
        .try_withdraw(
            &other_requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 60),
            &id(&f.env, 1),
            &1,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);

    // Only managers publish revisions.
    approve(&f, &f.approver, 61, 1, 1);
    approve(&f, &f.approver2, 62, 1, 2);
    for (n, who) in [&f.requester, &f.approver, &f.viewer, &stranger]
        .iter()
        .enumerate()
    {
        let err = f
            .client
            .try_publish_revision(
                who,
                &f.tenant,
                &f.project,
                &id(&f.env, 70 + n as u8),
                &id(&f.env, 1),
                &3,
                &id(&f.env, 120),
                &id(&f.env, 121),
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, ContractError::Unauthorized);
    }

    // Reads and expiry need some scoped role.
    assert_eq!(
        f.client
            .try_get_extension(&stranger, &f.tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_list_extensions(&stranger, &f.tenant, &f.project, &0, &8)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_expire(
                &stranger,
                &f.tenant,
                &f.project,
                &id(&f.env, 80),
                &id(&f.env, 1)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_set_role(
                &f.manager,
                &stranger,
                &f.tenant,
                &f.project,
                &Role::Viewer,
                &true,
                &0
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn tenant_and_project_boundaries_are_enforced() {
    let f = setup();
    submit(&f, 10, 1, 9);
    let other_tenant = id(&f.env, 70);
    let other_project = id(&f.env, 71);
    assert_eq!(
        f.client
            .try_get_extension(&f.viewer, &other_tenant, &f.project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_get_extension(&f.viewer, &f.tenant, &other_project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    // The same target in another tenant is not "already open" there.
    f.client.set_role(
        &f.admin,
        &f.requester,
        &other_tenant,
        &f.project,
        &Role::Requester,
        &true,
        &0,
    );
    let v = f.client.submit_request(
        &f.requester,
        &other_tenant,
        &f.project,
        &id(&f.env, 10),
        &input(&f, 1, 9),
    );
    assert_eq!(v, 1);
    // An approver of tenant A cannot act on tenant B's request.
    assert_eq!(
        f.client
            .try_approve(
                &f.approver,
                &other_tenant,
                &f.project,
                &id(&f.env, 11),
                &id(&f.env, 1),
                &1,
                &id(&f.env, 140),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn replays_return_recorded_results_without_side_effects() {
    let f = setup();
    assert_eq!(submit(&f, 10, 1, 9), 1);
    assert_eq!(submit(&f, 10, 1, 9), 1);
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(approve(&f, &f.approver, 11, 1, 1), 2);
    assert_eq!(approve(&f, &f.approver, 11, 1, 1), 2);
    assert_eq!(f.env.events().all().len(), 0);
    let record = get(&f, 1);
    assert_eq!(record.approvals, 1);
    assert_eq!(record.version, 2);
    // Replays still answer after later transitions moved the record on.
    approve(&f, &f.approver2, 12, 1, 2);
    assert_eq!(approve(&f, &f.approver, 11, 1, 1), 2);
    assert_eq!(get(&f, 1).approvals, 2);
    assert_eq!(f.env.events().all().len(), 0);
}

#[test]
fn request_reuse_with_other_payload_or_actor_conflicts() {
    let f = setup();
    submit(&f, 10, 1, 9);
    let err = f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &input(&f, 2, 8),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
    let other = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &other,
        &f.tenant,
        &f.project,
        &Role::Requester,
        &true,
        &0,
    );
    let err = f
        .client
        .try_submit_request(
            &other,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &input(&f, 1, 9),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
    approve(&f, &f.approver, 11, 1, 1);
    // Same request id, different approver: conflict, not a second approval.
    assert_eq!(
        try_approve(&f, &f.approver2, 11, 1, 1),
        ContractError::RequestConflict
    );
    assert_eq!(get(&f, 1).approvals, 1);
}

#[test]
fn only_one_open_request_per_target_and_duplicates_are_blocked() {
    let f = setup();
    submit(&f, 10, 1, 9);
    // A different request for the same target is blocked while one is open.
    let err = f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 11),
            &input(&f, 2, 9),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyOpen);
    // The same extension id can never be reused, even for a free target.
    let err = f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 12),
            &input(&f, 1, 8),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyExists);
    // Other targets are independent.
    assert_eq!(submit(&f, 13, 3, 7), 1);
}

#[test]
fn two_approvals_are_required_and_each_approver_counts_once() {
    let f = setup();
    submit(&f, 10, 1, 9);
    assert_eq!(approve(&f, &f.approver, 11, 1, 1), 2);
    let record = get(&f, 1);
    assert_eq!(record.state, ExtensionState::Requested);
    assert_eq!(record.approvals, 1);

    assert_eq!(
        try_approve(&f, &f.approver, 12, 1, 2),
        ContractError::AlreadyApproved
    );
    assert_eq!(get(&f, 1).approvals, 1);

    assert_eq!(approve(&f, &f.approver2, 13, 1, 2), 3);
    let record = get(&f, 1);
    assert_eq!(record.state, ExtensionState::Approved);
    assert_eq!(record.approvals, 2);
    // No further approvals once the threshold is met.
    let third = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &third,
        &f.tenant,
        &f.project,
        &Role::Approver,
        &true,
        &0,
    );
    assert_eq!(
        try_approve(&f, &third, 14, 1, 3),
        ContractError::InvalidTransition
    );
}

#[test]
fn requesters_cannot_approve_or_reject_their_own_requests() {
    let f = setup();
    f.client.set_role(
        &f.admin,
        &f.requester,
        &f.tenant,
        &f.project,
        &Role::Approver,
        &true,
        &0,
    );
    submit(&f, 10, 1, 9);
    assert_eq!(
        try_approve(&f, &f.requester, 11, 1, 1),
        ContractError::SeparationOfDuty
    );
    let err = f
        .client
        .try_reject(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 12),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 130),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::SeparationOfDuty);
    assert_eq!(get(&f, 1).approvals, 0);
}

#[test]
fn stale_versions_stop_concurrent_decisions() {
    let f = setup();
    submit(&f, 10, 1, 9);
    approve(&f, &f.approver, 11, 1, 1);
    // Second approver read version 1 before the first approval landed.
    assert_eq!(
        try_approve(&f, &f.approver2, 12, 1, 1),
        ContractError::StaleVersion
    );
    let err = f
        .client
        .try_reject(
            &f.approver2,
            &f.tenant,
            &f.project,
            &id(&f.env, 13),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 131),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::StaleVersion);
    // Retrying with the fresh version works under the same request id.
    assert_eq!(approve(&f, &f.approver2, 12, 1, 2), 3);
}

#[test]
fn rejection_is_a_final_immutable_decision() {
    let f = setup();
    submit(&f, 10, 1, 9);
    let v = f.client.reject(
        &f.approver,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &1,
        &id(&f.env, 132),
    );
    assert_eq!(v, 2);
    let record = get(&f, 1);
    assert_eq!(record.state, ExtensionState::Rejected);
    assert_eq!(record.decision_hash, id(&f.env, 132));
    assert_eq!(record.decided_at, NOW);
    // No later call can change it.
    assert_eq!(
        try_approve(&f, &f.approver2, 12, 1, 2),
        ContractError::InvalidTransition
    );
    let err = f
        .client
        .try_reject(
            &f.approver2,
            &f.tenant,
            &f.project,
            &id(&f.env, 13),
            &id(&f.env, 1),
            &2,
            &id(&f.env, 133),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
    let err = f
        .client
        .try_withdraw(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 14),
            &id(&f.env, 1),
            &2,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
    assert_eq!(get(&f, 1).decision_hash, id(&f.env, 132));
    // The target is free again for a fresh request.
    assert_eq!(submit(&f, 15, 2, 9), 1);
}

#[test]
fn withdrawal_is_limited_to_pending_requests_and_frees_the_target() {
    let f = setup();
    submit(&f, 10, 1, 9);
    let v = f.client.withdraw(
        &f.requester,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &1,
    );
    assert_eq!(v, 2);
    assert_eq!(get(&f, 1).state, ExtensionState::Withdrawn);
    assert_eq!(
        f.client
            .open_request_for(&f.viewer, &f.tenant, &f.project, &id(&f.env, 9)),
        None
    );
    assert_eq!(
        try_approve(&f, &f.approver, 12, 1, 2),
        ContractError::InvalidTransition
    );

    // Once approved the request can no longer be withdrawn.
    approved(&f, 2, 8, 20);
    let err = f
        .client
        .try_withdraw(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 30),
            &id(&f.env, 2),
            &3,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
}

#[test]
fn approved_requests_publish_a_revision_exactly_once() {
    let f = setup();
    submit(&f, 10, 1, 9);
    // Not yet approved: publishing is an invalid transition.
    let err = f
        .client
        .try_publish_revision(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 11),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 120),
            &id(&f.env, 121),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
    approve(&f, &f.approver, 12, 1, 1);
    approve(&f, &f.approver2, 13, 1, 2);
    let stale = f
        .client
        .try_publish_revision(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 14),
            &id(&f.env, 1),
            &2,
            &id(&f.env, 120),
            &id(&f.env, 121),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(stale, ContractError::StaleVersion);
    assert_eq!(publish(&f, 15, 1, 3), 4);
    let record = get(&f, 1);
    assert_eq!(record.state, ExtensionState::Applied);
    assert_eq!(record.revised_schedule_hash, id(&f.env, 120));
    assert_eq!(record.decision_hash, id(&f.env, 121));
    assert_eq!(record.decided_at, NOW);
    assert_eq!(record.funding_delta, 2_500);
    assert_eq!(record.proposed_deadline, NOW + 34 * DAY);

    // A second publication under a new request id is rejected; the record is frozen.
    let err = f
        .client
        .try_publish_revision(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 16),
            &id(&f.env, 1),
            &4,
            &id(&f.env, 122),
            &id(&f.env, 123),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
    assert_eq!(get(&f, 1).revised_schedule_hash, id(&f.env, 120));
    // A retry of the original request is an idempotent replay.
    assert_eq!(publish(&f, 15, 1, 3), 4);
    assert_eq!(f.env.events().all().len(), 0);
    // The target is free again.
    assert_eq!(submit(&f, 17, 2, 9), 1);
}

#[test]
fn expiry_blocks_late_decisions_and_frees_the_target() {
    let f = setup();
    submit(&f, 10, 1, 9);
    // Too early to expire.
    assert_eq!(
        f.client
            .try_expire(
                &f.viewer,
                &f.tenant,
                &f.project,
                &id(&f.env, 11),
                &id(&f.env, 1)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotYetExpired
    );
    f.env.ledger().set_timestamp(NOW + 10 * DAY);
    assert_eq!(
        try_approve(&f, &f.approver, 12, 1, 1),
        ContractError::RequestExpired
    );
    let err = f
        .client
        .try_reject(
            &f.approver,
            &f.tenant,
            &f.project,
            &id(&f.env, 13),
            &id(&f.env, 1),
            &1,
            &id(&f.env, 134),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestExpired);
    let v = f.client.expire(
        &f.viewer,
        &f.tenant,
        &f.project,
        &id(&f.env, 14),
        &id(&f.env, 1),
    );
    assert_eq!(v, 2);
    assert_eq!(get(&f, 1).state, ExtensionState::Expired);
    assert_eq!(get(&f, 1).decided_at, NOW + 10 * DAY);
    assert_eq!(
        f.client
            .open_request_for(&f.viewer, &f.tenant, &f.project, &id(&f.env, 9)),
        None
    );
    // Terminal now.
    assert_eq!(
        f.client
            .try_expire(
                &f.viewer,
                &f.tenant,
                &f.project,
                &id(&f.env, 15),
                &id(&f.env, 1)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
}

#[test]
fn approved_requests_cannot_be_published_after_expiry() {
    let f = setup();
    approved(&f, 1, 9, 10);
    f.env.ledger().set_timestamp(NOW + 10 * DAY);
    let err = f
        .client
        .try_publish_revision(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 20),
            &id(&f.env, 1),
            &3,
            &id(&f.env, 120),
            &id(&f.env, 121),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestExpired);
    f.client.expire(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 21),
        &id(&f.env, 1),
    );
    assert_eq!(get(&f, 1).state, ExtensionState::Expired);
}

#[test]
fn request_inputs_are_validated() {
    let f = setup();
    let check = |mut value: ExtensionInput, request: u8, expected: ContractError| {
        value.extension_id = id(&f.env, request);
        let err = f
            .client
            .try_submit_request(
                &f.requester,
                &f.tenant,
                &f.project,
                &id(&f.env, request),
                &value,
            )
            .unwrap_err()
            .unwrap();
        assert_eq!(err, expected, "request {request}");
    };
    let mut v = input(&f, 1, 9);
    v.proposed_deadline = v.current_deadline;
    check(v, 10, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.proposed_deadline = v.current_deadline + 90 * DAY + 1;
    check(v, 11, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.funding_delta = -1;
    check(v, 12, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.funding_delta = 1_000_000_000_000_000_001;
    check(v, 13, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.reason_hash = zero(&f.env);
    check(v, 14, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.impact_hash = zero(&f.env);
    check(v, 15, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.target_id = zero(&f.env);
    check(v, 16, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.approvals_required = 0;
    check(v, 17, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.approvals_required = 9;
    check(v, 18, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.current_deadline = NOW;
    v.proposed_deadline = NOW + DAY;
    v.expires_at = NOW;
    check(v, 19, ContractError::DeadlinePassed);
    let mut v = input(&f, 1, 9);
    v.expires_at = NOW;
    check(v, 20, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.expires_at = NOW + 30 * DAY + 1;
    v.current_deadline = NOW + 60 * DAY;
    v.proposed_deadline = NOW + 70 * DAY;
    check(v, 21, ContractError::InvalidInput);
    let mut v = input(&f, 1, 9);
    v.expires_at = v.current_deadline + 1;
    check(v, 22, ContractError::InvalidInput);

    // Boundary values are accepted.
    let mut ok = input(&f, 5, 5);
    ok.proposed_deadline = ok.current_deadline + 90 * DAY;
    ok.funding_delta = 1_000_000_000_000_000_000;
    ok.approvals_required = 8;
    ok.expires_at = NOW + 20 * DAY;
    assert!(f
        .client
        .try_submit_request(&f.requester, &f.tenant, &f.project, &id(&f.env, 30), &ok)
        .is_ok());
    let mut zero_cost = input(&f, 6, 6);
    zero_cost.funding_delta = 0;
    assert!(f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 31),
            &zero_cost
        )
        .is_ok());
}

#[test]
fn role_expiry_denies_access_after_the_deadline() {
    let f = setup();
    let temp = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &temp,
        &f.tenant,
        &f.project,
        &Role::Requester,
        &true,
        &(NOW + 100),
    );
    assert!(f
        .client
        .try_submit_request(
            &temp,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &input(&f, 1, 9)
        )
        .is_ok());
    f.env.ledger().set_timestamp(NOW + 100);
    let err = f
        .client
        .try_submit_request(
            &temp,
            &f.tenant,
            &f.project,
            &id(&f.env, 11),
            &input(&f, 2, 8),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RoleExpired);
    assert_eq!(
        f.client
            .try_set_role(
                &f.admin,
                &temp,
                &f.tenant,
                &f.project,
                &Role::Requester,
                &true,
                &(NOW + 5)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
    f.client.set_role(
        &f.admin,
        &f.approver,
        &f.tenant,
        &f.project,
        &Role::Approver,
        &false,
        &0,
    );
    assert_eq!(
        try_approve(&f, &f.approver, 12, 1, 1),
        ContractError::Unauthorized
    );
}

#[test]
fn storage_and_page_bounds_are_enforced() {
    let f = setup();
    for n in 0u32..64 {
        let mut value = input(&f, 1, 1);
        value.extension_id = id2(&f.env, 3, n as u8);
        value.target_id = id2(&f.env, 4, n as u8);
        f.client.submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id2(&f.env, 5, n as u8),
            &value,
        );
    }
    let mut value = input(&f, 1, 1);
    value.extension_id = id2(&f.env, 3, 200);
    value.target_id = id2(&f.env, 4, 200);
    let err = f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id2(&f.env, 5, 200),
            &value,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::LimitReached);
    let first = f
        .client
        .list_extensions(&f.viewer, &f.tenant, &f.project, &0, &32);
    assert_eq!(first.ids.len(), 32);
    assert!(first.has_more);
    let last = f
        .client
        .list_extensions(&f.viewer, &f.tenant, &f.project, &32, &32);
    assert_eq!(last.ids.len(), 32);
    assert_eq!(last.next, 64);
    assert!(!last.has_more);
    for (from, limit) in [(0u32, 0u32), (0, 33), (65, 1)] {
        assert_eq!(
            f.client
                .try_list_extensions(&f.viewer, &f.tenant, &f.project, &from, &limit)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidPage
        );
    }
}

#[test]
fn events_follow_the_documented_schema_without_private_payloads() {
    let f = setup();
    let topic_of = |f: &Fixture| -> Symbol {
        let events = f.env.events().all();
        assert_eq!(events.len(), 1);
        let (_, topics, _) = events.get(0).unwrap();
        assert_eq!(topics.len(), 1);
        Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap()
    };

    submit(&f, 10, 1, 9);
    assert_eq!(topic_of(&f), symbol_short!("EXT_REQ"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (tenant, project, ext, target, deadline, delta, version): (
        BytesN<32>,
        BytesN<32>,
        BytesN<32>,
        BytesN<32>,
        u64,
        i128,
        u32,
    ) = data.try_into_val(&f.env).unwrap();
    assert_eq!(tenant, f.tenant);
    assert_eq!(project, f.project);
    assert_eq!((ext, target), (id(&f.env, 1), id(&f.env, 9)));
    assert_eq!((deadline, delta, version), (NOW + 34 * DAY, 2_500, 1));

    approve(&f, &f.approver, 11, 1, 1);
    assert_eq!(topic_of(&f), symbol_short!("EXT_APPR"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, _, count, state, version): (BytesN<32>, BytesN<32>, BytesN<32>, u32, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((count, state, version), (1, 1, 2));

    approve(&f, &f.approver2, 12, 1, 2);
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, _, count, state, version): (BytesN<32>, BytesN<32>, BytesN<32>, u32, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((count, state, version), (2, 2, 3));

    publish(&f, 13, 1, 3);
    assert_eq!(topic_of(&f), symbol_short!("EXT_APL"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, _, deadline, delta, version): (BytesN<32>, BytesN<32>, BytesN<32>, u64, i128, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((deadline, delta, version), (NOW + 34 * DAY, 2_500, 4));

    submit(&f, 20, 2, 8);
    f.client.reject(
        &f.approver,
        &f.tenant,
        &f.project,
        &id(&f.env, 21),
        &id(&f.env, 2),
        &1,
        &id(&f.env, 135),
    );
    assert_eq!(topic_of(&f), symbol_short!("EXT_REJ"));

    submit(&f, 30, 3, 7);
    f.client.withdraw(
        &f.requester,
        &f.tenant,
        &f.project,
        &id(&f.env, 31),
        &id(&f.env, 3),
        &1,
    );
    assert_eq!(topic_of(&f), symbol_short!("EXT_WDR"));

    submit(&f, 40, 4, 6);
    f.env.ledger().set_timestamp(NOW + 10 * DAY);
    f.client.expire(
        &f.viewer,
        &f.tenant,
        &f.project,
        &id(&f.env, 41),
        &id(&f.env, 4),
    );
    assert_eq!(topic_of(&f), symbol_short!("EXT_EXP"));
    let events = f.env.events().all();
    let (_, _, data) = events.get(0).unwrap();
    let (_, _, ext, version): (BytesN<32>, BytesN<32>, BytesN<32>, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((ext, version), (id(&f.env, 4), 2));

    f.client.set_role(
        &f.admin,
        &f.viewer,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &0,
    );
    assert_eq!(topic_of(&f), symbol_short!("EXT_ROLE"));
}

#[test]
fn failed_calls_emit_no_events_and_change_nothing() {
    let f = setup();
    submit(&f, 10, 1, 9);
    assert_eq!(
        try_approve(&f, &f.viewer, 11, 1, 1),
        ContractError::Unauthorized
    );
    assert_eq!(
        try_approve(&f, &f.approver, 12, 1, 9),
        ContractError::StaleVersion
    );
    assert_eq!(f.env.events().all().len(), 0);
    let record = get(&f, 1);
    assert_eq!(record.approvals, 0);
    assert_eq!(record.version, 1);
}

#[test]
fn error_codes_and_discriminants_are_stable() {
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
    assert_eq!(ContractError::AlreadyOpen as u32, 14);
    assert_eq!(ContractError::DeadlinePassed as u32, 15);
    assert_eq!(ContractError::SeparationOfDuty as u32, 16);
    assert_eq!(ContractError::RequestExpired as u32, 17);
    assert_eq!(ContractError::AlreadyApproved as u32, 18);
    assert_eq!(ContractError::NotYetExpired as u32, 19);
    assert_eq!(
        [
            Role::Manager as u32,
            Role::Requester as u32,
            Role::Approver as u32,
            Role::Viewer as u32
        ],
        [1, 2, 3, 4]
    );
    assert_eq!(
        [
            ExtensionState::Requested as u32,
            ExtensionState::Approved as u32,
            ExtensionState::Rejected as u32,
            ExtensionState::Withdrawn as u32,
            ExtensionState::Expired as u32,
            ExtensionState::Applied as u32
        ],
        [1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn records_written_by_an_earlier_deployment_remain_readable() {
    let f = setup();
    let legacy = ExtensionRecord {
        extension_id: id(&f.env, 77),
        tenant_id: f.tenant.clone(),
        project_id: f.project.clone(),
        target_id: id(&f.env, 78),
        requester: f.requester.clone(),
        reason_hash: id(&f.env, 79),
        impact_hash: id(&f.env, 80),
        current_deadline: NOW + 20 * DAY,
        proposed_deadline: NOW + 25 * DAY,
        funding_delta: 7,
        approvals_required: 1,
        approvals: 0,
        expires_at: NOW + 5 * DAY,
        state: ExtensionState::Requested,
        decision_hash: zero(&f.env),
        revised_schedule_hash: zero(&f.env),
        created_at: 3,
        decided_at: 0,
        version: 1,
    };
    f.env.as_contract(&f.client.address, || {
        let storage = f.env.storage().persistent();
        storage.set(
            &DataKey::Extension(f.tenant.clone(), f.project.clone(), id(&f.env, 77)),
            &legacy,
        );
        storage.set(
            &DataKey::Open(f.tenant.clone(), f.project.clone(), id(&f.env, 78)),
            &id(&f.env, 77),
        );
        storage.set(
            &DataKey::Role(
                f.viewer.clone(),
                f.tenant.clone(),
                f.project.clone(),
                Role::Viewer,
            ),
            &RoleGrant {
                active: true,
                expires_at: 0,
            },
        );
        storage.set(
            &DataKey::Request(f.tenant.clone(), f.project.clone(), id(&f.env, 90)),
            &RequestRecord {
                input_hash: id(&f.env, 91),
                result: 1,
            },
        );
    });
    assert_eq!(get(&f, 77), legacy);
    // The legacy open pointer still blocks a duplicate for its target.
    let err = f
        .client
        .try_submit_request(
            &f.requester,
            &f.tenant,
            &f.project,
            &id(&f.env, 92),
            &input(&f, 5, 78),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyOpen);
    // And it continues through the lifecycle with its original version.
    assert_eq!(approve(&f, &f.approver, 93, 77, 1), 2);
    assert_eq!(get(&f, 77).state, ExtensionState::Approved);
}

#[test]
fn resource_budgets_stay_within_transaction_limits() {
    let f = setup();
    f.env.cost_estimate().budget().reset_default();
    submit(&f, 10, 1, 9);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    f.env.cost_estimate().budget().reset_default();
    approve(&f, &f.approver, 11, 1, 1);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    f.env.cost_estimate().budget().reset_default();
    approve(&f, &f.approver2, 12, 1, 2);
    publish(&f, 13, 1, 3);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 25_000_000);
    assert!(budget.memory_bytes_cost() < 10_485_760);
}
