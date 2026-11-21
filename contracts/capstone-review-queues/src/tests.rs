extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CapstoneReviewQueuesContract, CapstoneReviewQueuesContractClient, AuditKind, ContractError,
    ExpertiseFit, Priority, QueueFilter, QueueState, ReviewOutcome, Role,
};

struct Ctx {
    env: Env,
    admin: Address,
    coordinator: Address,
    reviewer: Address,
    second_reviewer: Address,
    observer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
    client: CapstoneReviewQueuesContractClient<'static>,
}

fn hash_u32(env: &Env, seed: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[28..].copy_from_slice(&seed.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = 2_000_000_000);
    let contract_id = env.register(CapstoneReviewQueuesContract, ());
    let client = CapstoneReviewQueuesContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let second_reviewer = Address::generate(&env);
    let observer = Address::generate(&env);
    client.initialize(&admin).unwrap();
    let tenant = hash_u32(&env, 1);
    let project = hash_u32(&env, 2);
    client
        .grant_role(&admin, &tenant, &coordinator, &Role::Coordinator)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &reviewer, &Role::Reviewer)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &second_reviewer, &Role::Reviewer)
        .unwrap();
    client
        .grant_role(&admin, &tenant, &observer, &Role::Observer)
        .unwrap();
    Ctx {
        env,
        admin,
        coordinator,
        reviewer,
        second_reviewer,
        observer,
        tenant,
        project,
        client,
    }
}

fn set_reviewer(ctx: &Ctx, reviewer: &Address, capacity: u32, mask: u32, version: u32) -> u32 {
    ctx.client
        .set_reviewer(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 10_000 + version),
            &version,
            reviewer,
            &capacity,
            &mask,
        )
        .unwrap()
}

fn enqueue(ctx: &Ctx, seed: u32, mask: u32, priority: Priority, due_at: u64, version: u32) -> BytesN<32> {
    let item_id = hash_u32(&ctx.env, seed);
    ctx.client
        .enqueue(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, seed + 1_000),
            &version,
            &item_id,
            &mask,
            &priority,
            &due_at,
        )
        .unwrap();
    item_id
}

fn assign(
    ctx: &Ctx,
    item_id: &BytesN<32>,
    reviewer: &Address,
    expected_assignee: Option<Address>,
    request_seed: u32,
    version: u32,
) -> u32 {
    ctx.client
        .assign(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, request_seed),
            &version,
            item_id,
            reviewer,
            &expected_assignee,
        )
        .unwrap()
}

fn no_assignee() -> Option<Address> {
    None
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
            .try_set_reviewer(
                &ctx.observer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 10),
                &0,
                &ctx.reviewer,
                &1,
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn reviewer_enqueue_assign_lifecycle() {
    let ctx = setup();
    assert_eq!(set_reviewer(&ctx, &ctx.reviewer, 2, 0b1111, 0), 1);
    let item = enqueue(&ctx, 20, 0b1100, Priority::High, ctx.env.ledger().timestamp() + 100, 1);
    assert_eq!(assign(&ctx, &item, &ctx.reviewer, no_assignee(), 21, 2), 3);

    let record = ctx
        .client
        .get_item(&ctx.observer, &ctx.tenant, &ctx.project, &item)
        .unwrap();
    assert_eq!(record.state, QueueState::Assigned);
    assert_eq!(record.assignee, Some(ctx.reviewer.clone()));
    assert_eq!(record.fit, ExpertiseFit::Full);
    assert_eq!(record.priority, Priority::High);
    assert_eq!(record.created_at, ctx.env.ledger().timestamp());

    let workload = ctx
        .client
        .workload(&ctx.observer, &ctx.tenant, &ctx.project, &ctx.reviewer)
        .unwrap();
    assert_eq!(workload.capacity, 2);
    assert_eq!(workload.assigned, 1);
    assert_eq!(workload.decided, 0);
    assert_eq!(workload.overdue, 0);
    assert_eq!(workload.expertise_mask, 0b1111);
    assert_eq!(ctx.client.project_version(&ctx.tenant, &ctx.project), 3);
}

#[test]
fn conflicts_block_outcomes_and_are_audited() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 2, 0b1111, 0);
    let item = enqueue(&ctx, 30, 0b0001, Priority::Normal, 0, 1);
    assign(&ctx, &item, &ctx.reviewer, no_assignee(), 31, 2);

    assert_eq!(
        ctx.client
            .declare_conflict(
                &ctx.reviewer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 32),
                &3,
                &item,
                &true,
            )
            .unwrap(),
        4
    );
    assert!(ctx
        .client
        .get_item(&ctx.observer, &ctx.tenant, &ctx.project, &item)
        .unwrap()
        .conflict);
    assert_eq!(
        ctx.client
            .try_record_outcome(
                &ctx.reviewer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 33),
                &4,
                &item,
                &ReviewOutcome::Approve,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ConflictOfInterest
    );

    ctx.client
        .declare_conflict(
            &ctx.reviewer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 34),
            &5,
            &item,
            &false,
        )
        .unwrap();
    ctx.client
        .record_outcome(
            &ctx.reviewer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 35),
            &6,
            &item,
            &ReviewOutcome::Approve,
        )
        .unwrap();
    let record = ctx
        .client
        .get_item(&ctx.observer, &ctx.tenant, &ctx.project, &item)
        .unwrap();
    assert_eq!(record.state, QueueState::Decided);
    assert_eq!(record.outcome, Some(ReviewOutcome::Approve));
    let audit = ctx
        .client
        .get_audit(&ctx.observer, &ctx.tenant, &ctx.project, &item)
        .unwrap();
    assert_eq!(audit.len(), 5);
    assert_eq!(audit.get(0).unwrap().kind, AuditKind::Enqueued);
    assert_eq!(audit.get(2).unwrap().kind, AuditKind::ConflictDeclared);
    assert_eq!(audit.get(3).unwrap().kind, AuditKind::ConflictCleared);
    assert_eq!(audit.get(4).unwrap().actor_role, Role::Reviewer);
    let workload = ctx
        .client
        .workload(&ctx.observer, &ctx.tenant, &ctx.project, &ctx.reviewer)
        .unwrap();
    assert_eq!(workload.assigned, 0);
    assert_eq!(workload.decided, 1);
}

#[test]
fn assignment_tokens_fit_and_capacity_are_enforced() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 1, 0b0001, 0);
    set_reviewer(&ctx, &ctx.second_reviewer, 1, 0b0010, 1);
    let item = enqueue(&ctx, 40, 0b0001, Priority::Normal, 0, 2);
    assign(&ctx, &item, &ctx.reviewer, no_assignee(), 41, 3);

    assert_eq!(
        ctx.client
            .try_assign(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 42),
                &4,
                &item,
                &ctx.second_reviewer,
                &no_assignee(),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::AssignmentConflict
    );
    assert_eq!(
        assign(
            &ctx,
            &item,
            &ctx.second_reviewer,
            Some(ctx.reviewer.clone()),
            43,
            4,
        ),
        5
    );
    let record = ctx
        .client
        .get_item(&ctx.observer, &ctx.tenant, &ctx.project, &item)
        .unwrap();
    assert_eq!(record.fit, ExpertiseFit::NoFit);
    assert_eq!(
        ctx.client
            .workload(&ctx.observer, &ctx.tenant, &ctx.project, &ctx.reviewer)
            .unwrap()
            .assigned,
        0
    );

    let second_item = enqueue(&ctx, 44, 0b0001, Priority::Low, 0, 5);
    assign(&ctx, &second_item, &ctx.reviewer, no_assignee(), 45, 6);
    let third_item = enqueue(&ctx, 46, 0b0001, Priority::Low, 0, 7);
    assert_eq!(
        ctx.client
            .try_assign(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 47),
                &8,
                &third_item,
                &ctx.reviewer,
                &no_assignee(),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ReviewerAtCapacity
    );
}

#[test]
fn assignment_replay_is_idempotent_and_outcomes_are_final() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 2, 0b0011, 0);
    let item = enqueue(&ctx, 50, 0b0010, Priority::Urgent, 0, 1);
    let request = hash_u32(&ctx.env, 51);
    ctx.client
        .assign(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &2,
            &item,
            &ctx.reviewer,
            &no_assignee(),
        )
        .unwrap();
    assert_eq!(ctx.env.events().all().len(), 1);
    let replay = ctx
        .client
        .assign(
            &ctx.coordinator,
            &ctx.tenant,
            &ctx.project,
            &request,
            &2,
            &item,
            &ctx.reviewer,
            &no_assignee(),
        )
        .unwrap();
    assert_eq!(replay, 3);
    assert_eq!(ctx.env.events().all().len(), 0);

    assert_eq!(
        ctx.client
            .try_assign(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &request,
                &2,
                &item,
                &ctx.second_reviewer,
                &no_assignee(),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    let outcome_version = ctx
        .client
        .record_outcome(
            &ctx.reviewer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 52),
            &3,
            &item,
            &ReviewOutcome::Reject,
        )
        .unwrap();
    assert_eq!(outcome_version, 4);
    assert_eq!(
        ctx.client
            .try_record_outcome(
                &ctx.reviewer,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 53),
                &4,
                &item,
                &ReviewOutcome::Approve,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn queue_filters_are_stable_and_validated() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 4, 0b1111, 0);
    let now = ctx.env.ledger().timestamp();
    let a = enqueue(&ctx, 60, 0b0001, Priority::Urgent, now + 100, 1);
    let b = enqueue(&ctx, 61, 0b0010, Priority::Low, now + 10, 2);
    let c = enqueue(&ctx, 62, 0b0100, Priority::High, now + 50, 3);
    assign(&ctx, &b, &ctx.reviewer, no_assignee(), 63, 4);

    let all = QueueFilter {
        priority: None,
        assignee: None,
        unassigned_only: false,
        due_before: None,
    };
    assert_eq!(
        ctx.client
            .list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &all)
            .unwrap()
            .len(),
        3
    );
    let high = QueueFilter {
        priority: Some(Priority::High),
        ..all.clone()
    };
    let high_page = ctx
        .client
        .list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &high)
        .unwrap();
    assert_eq!(high_page.len(), 1);
    assert_eq!(high_page.get(0).unwrap(), c.clone());
    let unassigned = QueueFilter {
        unassigned_only: true,
        ..all.clone()
    };
    let unassigned_page = ctx
        .client
        .list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &unassigned)
        .unwrap();
    assert_eq!(unassigned_page.len(), 2);
    assert_eq!(unassigned_page.get(0).unwrap(), a);
    assert_eq!(unassigned_page.get(1).unwrap(), c);
    let due = QueueFilter {
        due_before: Some(now + 60),
        ..all.clone()
    };
    let due_page = ctx
        .client
        .list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &due)
        .unwrap();
    assert_eq!(due_page.len(), 2);
    assert_eq!(due_page.get(0).unwrap(), b.clone());
    let assignee = QueueFilter {
        assignee: Some(ctx.reviewer.clone()),
        ..all.clone()
    };
    let assignee_page = ctx
        .client
        .list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &assignee)
        .unwrap();
    assert_eq!(assignee_page.len(), 1);
    assert_eq!(assignee_page.get(0).unwrap(), b);
    let invalid = QueueFilter {
        assignee: Some(ctx.reviewer.clone()),
        unassigned_only: true,
        ..all
    };
    assert_eq!(
        ctx.client
            .try_list_queue(&ctx.observer, &ctx.tenant, &ctx.project, &0, &64, &invalid)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidFilter
    );
}

#[test]
fn workload_overdue_is_derived_from_timestamps() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 2, 0b0001, 0);
    let past_due = ctx.env.ledger().timestamp().saturating_sub(10);
    let item = enqueue(&ctx, 70, 0b0001, Priority::Normal, past_due, 1);
    assign(&ctx, &item, &ctx.reviewer, no_assignee(), 71, 2);
    assert_eq!(
        ctx.client
            .workload(&ctx.observer, &ctx.tenant, &ctx.project, &ctx.reviewer)
            .unwrap()
            .overdue,
        1
    );
    ctx.client
        .record_outcome(
            &ctx.reviewer,
            &ctx.tenant,
            &ctx.project,
            &hash_u32(&ctx.env, 72),
            &3,
            &item,
            &ReviewOutcome::Approve,
        )
        .unwrap();
    let workload = ctx
        .client
        .workload(&ctx.observer, &ctx.tenant, &ctx.project, &ctx.reviewer)
        .unwrap();
    assert_eq!(workload.overdue, 0);
    assert_eq!(workload.decided, 1);
}

#[test]
fn reads_are_tenant_scoped_and_revocation_is_immediate() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 1, 1, 0);
    let item = enqueue(&ctx, 80, 1, Priority::Normal, 0, 1);
    let other_tenant = hash_u32(&ctx.env, 9_080);
    assert_eq!(
        ctx.client
            .try_get_item(&ctx.reviewer, &other_tenant, &ctx.project, &item)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    ctx.client
        .revoke_role(&ctx.admin, &ctx.tenant, &ctx.reviewer, &Role::Reviewer)
        .unwrap();
    assert_eq!(
        ctx.client
            .try_get_item(&ctx.reviewer, &ctx.tenant, &ctx.project, &item)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(ctx
        .client
        .get_item(&ctx.admin, &ctx.tenant, &ctx.project, &item)
        .is_ok());
}

#[test]
fn queue_audit_and_page_bounds_are_enforced() {
    let ctx = setup();
    assert_eq!(
        ctx.client
            .try_list_queue(
                &ctx.observer,
                &ctx.tenant,
                &ctx.project,
                &0,
                &0,
                &QueueFilter {
                    priority: None,
                    assignee: None,
                    unassigned_only: false,
                    due_before: None,
                },
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
    let mut version = 0;
    for i in 0..128u32 {
        let item_id = hash_u32(&ctx.env, 10_000 + i);
        ctx.client
            .enqueue(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 20_000 + i),
                &version,
                &item_id,
                &1,
                &Priority::Normal,
                &0,
            )
            .unwrap();
        version += 1;
    }
    assert_eq!(
        ctx.client
            .try_enqueue(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 30_000),
                &version,
                &hash_u32(&ctx.env, 30_001),
                &1,
                &Priority::Normal,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
    let first = hash_u32(&ctx.env, 10_000);
    for i in 0..31u32 {
        ctx.client
            .set_priority(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 40_000 + i),
                &version,
                &first,
                &Priority::High,
            )
            .unwrap();
        version += 1;
    }
    assert_eq!(
        ctx.client
            .get_audit(&ctx.observer, &ctx.tenant, &ctx.project, &first)
            .unwrap()
            .len(),
        32
    );
    assert_eq!(
        ctx.client
            .try_set_priority(
                &ctx.coordinator,
                &ctx.tenant,
                &ctx.project,
                &hash_u32(&ctx.env, 50_000),
                &version,
                &first,
                &Priority::High,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}

#[test]
fn events_use_stable_id_and_code_payloads() {
    let ctx = setup();
    set_reviewer(&ctx, &ctx.reviewer, 1, 0b0011, 0);
    let item = enqueue(&ctx, 90, 0b0011, Priority::High, 0, 1);
    assign(&ctx, &item, &ctx.reviewer, no_assignee(), 91, 2);
    let events = ctx.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&ctx.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("CQRASS"));
    let payload: (BytesN<32>, BytesN<32>, BytesN<32>, u32) = data.try_into_val(&ctx.env).unwrap();
    assert_eq!(payload.0, ctx.tenant);
    assert_eq!(payload.1, ctx.project);
    assert_eq!(payload.2, item);
    assert_eq!(payload.3, ExpertiseFit::Full as u32);
}
