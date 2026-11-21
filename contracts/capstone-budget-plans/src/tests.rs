#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    symbol_short, Address, BytesN, Env, Symbol, Vec,
};

use crate::{
    BudgetLine, BudgetState, CapstoneBudgetPlansContract, CapstoneBudgetPlansContractClient,
    ContractError, Role,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn line(env: &Env, seed: u8, quantity: u32, unit_amount: i128) -> BudgetLine {
    BudgetLine {
        category_hash: hash(env, seed),
        justification_hash: hash(env, seed + 1),
        funding_source_hash: hash(env, seed + 2),
        quantity,
        unit_amount,
        line_total: i128::from(quantity) * unit_amount,
    }
}

fn lines(env: &Env) -> Vec<BudgetLine> {
    Vec::from_array(&env, &[line(env, 1, 2, 100), line(env, 4, 3, 200)])
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneBudgetPlansContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneBudgetPlansContract, ());
    let client = CapstoneBudgetPlansContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let tenant = hash(&env, 20);
    let project = hash(&env, 21);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &coordinator, &Role::Coordinator);
    client.grant_role(&admin, &tenant, &reviewer, &Role::Reviewer);
    (
        env,
        contract_id,
        admin,
        coordinator,
        reviewer,
        tenant,
        project,
        client,
    )
}

#[test]
fn initialization_and_role_grants_are_denied_by_default() {
    let (env, _, admin, _, _, tenant, _, client) = setup();
    let outsider = Address::generate(&env);
    let account = Address::generate(&env);

    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    assert_eq!(
        client.try_grant_role(&outsider, &tenant, &account, &Role::Coordinator),
        Err(Ok(ContractError::Unauthorized))
    );
    assert!(!client.has_role(&tenant, &account, &Role::Coordinator));

    client.revoke_role(&admin, &tenant, &account, &Role::Coordinator);
    assert!(!client.has_role(&tenant, &account, &Role::Coordinator));
}

#[test]
fn proposal_is_idempotent_and_emits_only_public_values() {
    let (env, _, _, coordinator, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    let args = (
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 30),
        &hash(&env, 31),
        &lines(&env),
        &hash(&env, 32),
        &1u32,
        &expiry,
        &0u32,
    );

    assert_eq!(client.propose_plan(
        args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
    ), 1);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CBPPLN"));
    let (event_tenant, event_project, version, total): (BytesN<32>, BytesN<32>, u32, i128) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_tenant, tenant);
    assert_eq!(event_project, project);
    assert_eq!(version, 1);
    assert_eq!(total, 800);

    assert_eq!(client.propose_plan(
        args.0, args.1, args.2, args.3, args.4, args.5, args.6, args.7, args.8, args.9,
    ), 1);
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(client.get_plan(&tenant, &project).version, 1);

    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 30),
            &hash(&env, 40),
            &lines(&env),
            &hash(&env, 32),
            &1u32,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::RequestConflict))
    );
}

#[test]
fn only_stored_coordinator_role_can_propose_and_version_must_match() {
    let (env, _, _, coordinator, reviewer, tenant, project, client) = setup();
    let outsider = Address::generate(&env);
    let expiry = env.ledger().timestamp() + 500;

    assert_eq!(
        client.try_propose_plan(
            &outsider,
            &tenant,
            &project,
            &hash(&env, 50),
            &hash(&env, 51),
            &lines(&env),
            &hash(&env, 52),
            &1u32,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );
    assert_eq!(
        client.try_propose_plan(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 50),
            &hash(&env, 51),
            &lines(&env),
            &hash(&env, 52),
            &1u32,
            &expiry,
            &0u32,
        ),
        Err(Ok(ContractError::Unauthorized))
    );

    client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 50),
        &hash(&env, 51),
        &lines(&env),
        &hash(&env, 52),
        &1u32,
        &expiry,
        &0u32,
    );
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, coordinator);

    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 53),
            &hash(&env, 54),
            &lines(&env),
            &hash(&env, 55),
            &1u32,
            &expiry,
            &1u32,
        ),
        Err(Ok(ContractError::VersionConflict))
    );
    assert_eq!(
        client.try_get_plan(&hash(&env, 60), &project),
        Err(Ok(ContractError::NotFound))
    );
}

#[test]
fn approvals_are_bounded_by_threshold_and_revision_supersedes_prior_version() {
    let (env, _, admin, coordinator, reviewer, tenant, project, client) = setup();
    let second_reviewer = Address::generate(&env);
    client.grant_role(&admin, &tenant, &second_reviewer, &Role::Reviewer);
    let expiry = env.ledger().timestamp() + 500;
    client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 70),
        &hash(&env, 71),
        &lines(&env),
        &hash(&env, 72),
        &2u32,
        &expiry,
        &0u32,
    );
    client.submit_plan(&coordinator, &tenant, &project, &1u32);
    client.approve_plan(
        &reviewer,
        &tenant,
        &project,
        &hash(&env, 73),
        &hash(&env, 74),
        &1u32,
    );
    let partial = client.get_plan(&tenant, &project);
    assert_eq!(partial.approval_count, 1);
    assert_eq!(partial.state, BudgetState::Submitted);

    client.approve_plan(
        &second_reviewer,
        &tenant,
        &project,
        &hash(&env, 75),
        &hash(&env, 76),
        &1u32,
    );
    let approved = client.get_plan(&tenant, &project);
    assert_eq!(approved.state, BudgetState::Approved);
    assert_eq!(approved.approval_count, 2);

    assert_eq!(
        client.try_submit_plan(&coordinator, &tenant, &project, &1u32),
        Err(Ok(ContractError::InvalidState))
    );

    let next_expiry = env.ledger().timestamp() + 900;
    let next_version = client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 77),
        &hash(&env, 78),
        &lines(&env),
        &hash(&env, 79),
        &1u32,
        &next_expiry,
        &1u32,
    );
    assert_eq!(next_version, 2);
    assert_eq!(client.get_history(&tenant, &project, &1).unwrap().state, BudgetState::Approved);
    assert_eq!(client.get_lines(&tenant, &project, &2).len(), 2);

    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 80),
            &hash(&env, 81),
            &lines(&env),
            &hash(&env, 82),
            &1u32,
            &next_expiry,
            &1u32,
        ),
        Err(Ok(ContractError::VersionConflict))
    );
}

#[test]
fn approval_retry_does_not_duplicate_approvals_or_events() {
    let (env, _, _, coordinator, reviewer, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 90),
        &hash(&env, 91),
        &lines(&env),
        &hash(&env, 92),
        &1u32,
        &expiry,
        &0u32,
    );
    client.submit_plan(&coordinator, &tenant, &project, &1u32);
    client.approve_plan(
        &reviewer,
        &tenant,
        &project,
        &hash(&env, 93),
        &hash(&env, 94),
        &1u32,
    );
    let events_before = env.events().all().len();

    client.approve_plan(
        &reviewer,
        &tenant,
        &project,
        &hash(&env, 93),
        &hash(&env, 94),
        &1u32,
    );
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(client.get_plan(&tenant, &project).approval_count, 1);
    assert_eq!(events_before, 1);

    assert_eq!(
        client.try_approve_plan(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 95),
            &hash(&env, 96),
            &1u32,
        ),
        Err(Ok(ContractError::AlreadyApproved))
    );
}

#[test]
fn expiry_bounds_and_checked_arithmetic_are_enforced() {
    let (env, _, _, coordinator, reviewer, tenant, project, client) = setup();
    let now = env.ledger().timestamp();

    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 100),
            &hash(&env, 101),
            &lines(&env),
            &hash(&env, 102),
            &1u32,
            &now,
            &0u32,
        ),
        Err(Ok(ContractError::InvalidExpiry))
    );
    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 100),
            &hash(&env, 101),
            &Vec::new(&env),
            &hash(&env, 102),
            &1u32,
            &now + 100,
            &0u32,
        ),
        Err(Ok(ContractError::EmptyLines))
    );
    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 100),
            &hash(&env, 101),
            &Vec::from_array(
                &env,
                &[
                    line(&env, 1, 1, 1),
                    line(&env, 4, 1, 1),
                    line(&env, 7, 1, 1),
                    line(&env, 10, 1, 1),
                    line(&env, 13, 1, 1),
                    line(&env, 16, 1, 1),
                    line(&env, 19, 1, 1),
                    line(&env, 22, 1, 1),
                    line(&env, 25, 1, 1),
                    line(&env, 28, 1, 1),
                    line(&env, 31, 1, 1),
                    line(&env, 34, 1, 1),
                    line(&env, 37, 1, 1),
                    line(&env, 40, 1, 1),
                    line(&env, 43, 1, 1),
                    line(&env, 46, 1, 1),
                    line(&env, 49, 1, 1),
                ],
            ),
            &hash(&env, 102),
            &1u32,
            &now + 100,
            &0u32,
        ),
        Err(Ok(ContractError::TooManyLines))
    );
    let overflow_line = BudgetLine {
        category_hash: hash(&env, 1),
        justification_hash: hash(&env, 2),
        funding_source_hash: hash(&env, 3),
        quantity: 2,
        unit_amount: i128::MAX,
        line_total: i128::MAX,
    };
    assert_eq!(
        client.try_propose_plan(
            &coordinator,
            &tenant,
            &project,
            &hash(&env, 100),
            &hash(&env, 101),
            &Vec::from_array(&env, &[overflow_line]),
            &hash(&env, 102),
            &1u32,
            &now + 100,
            &0u32,
        ),
        Err(Ok(ContractError::Overflow))
    );

    let expiry = now + 20;
    client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 110),
        &hash(&env, 111),
        &lines(&env),
        &hash(&env, 112),
        &1u32,
        &expiry,
        &0u32,
    );
    client.submit_plan(&coordinator, &tenant, &project, &1u32);
    env.ledger().set_timestamp(expiry);
    assert_eq!(
        client.try_approve_plan(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, 113),
            &hash(&env, 114),
            &1u32,
        ),
        Err(Ok(ContractError::Expired))
    );
}

#[test]
fn records_remain_readable_through_a_fresh_client() {
    let (env, contract_id, _, coordinator, _, tenant, project, client) = setup();
    let expiry = env.ledger().timestamp() + 500;
    client.propose_plan(
        &coordinator,
        &tenant,
        &project,
        &hash(&env, 120),
        &hash(&env, 121),
        &lines(&env),
        &hash(&env, 122),
        &1u32,
        &expiry,
        &0u32,
    );

    let fresh = CapstoneBudgetPlansContractClient::new(&env, &contract_id);
    assert_eq!(fresh.get_plan(&tenant, &project), client.get_plan(&tenant, &project));
    assert_eq!(fresh.get_lines(&tenant, &project, &1), client.get_lines(&tenant, &project, &1));
}
