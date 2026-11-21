extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, vec, Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneExpenseClaims, CapstoneExpenseClaimsClient, Category, ClaimState, ContractError, Role,
};

const NOW: u64 = 1_000;

struct Fixture {
    env: Env,
    client: CapstoneExpenseClaimsClient<'static>,
    admin: Address,
    coordinator: Address,
    claimant: Address,
    reviewer: Address,
    reviewer2: Address,
    finance: Address,
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
    let contract_id = env.register(CapstoneExpenseClaims, ());
    let client = CapstoneExpenseClaimsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    let coordinator = Address::generate(&env);
    let claimant = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let reviewer2 = Address::generate(&env);
    let finance = Address::generate(&env);
    client.grant_role(&admin, &tenant, &project, &coordinator, &Role::Coordinator);
    client.grant_role(&admin, &tenant, &project, &claimant, &Role::Claimant);
    client.grant_role(&admin, &tenant, &project, &reviewer, &Role::Reviewer);
    client.grant_role(&admin, &tenant, &project, &reviewer2, &Role::Reviewer);
    client.grant_role(&admin, &tenant, &project, &finance, &Role::Finance);
    Fixture {
        env,
        client,
        admin,
        coordinator,
        claimant,
        reviewer,
        reviewer2,
        finance,
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
        &1_000,
        &3,
        &1,
    );
}

#[test]
fn claim_lifecycle_links_payout_with_typed_event() {
    let f = setup();
    default_policy(&f);
    let claim_id = id(&f.env, 10);
    let evidence = vec![&f.env, id(&f.env, 40), id(&f.env, 41)];
    let claim = f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 81),
        &0,
        &250,
        &Category::Materials,
        &evidence,
    );
    assert_eq!(claim.state, ClaimState::Submitted);
    assert_eq!(claim.version, 1);

    let approved = f.client.approve_claim(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 82),
        &1,
    );
    assert_eq!(approved.state, ClaimState::Approved);
    assert_eq!(approved.approvals, 1);

    let payout_hash = id(&f.env, 42);
    let paid = f.client.link_payout(
        &f.finance,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 83),
        &2,
        &payout_hash,
    );
    assert_eq!(paid.state, ClaimState::Paid);
    assert_eq!(paid.payout_hash, payout_hash);
    assert_eq!(paid.version, 3);

    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("EXCPAID"));
    let (event_claim, event_hash, amount): (BytesN<32>, BytesN<32>, i128) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(event_claim, claim_id);
    assert_eq!(event_hash, payout_hash);
    assert_eq!(amount, 250);
}

#[test]
fn two_approvals_enforce_reviewer_separation() {
    let f = setup();
    f.client.configure_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 84),
        &0,
        &1_000,
        &3,
        &2,
    );
    f.client.grant_role(
        &f.admin,
        &f.tenant,
        &f.project,
        &f.claimant,
        &Role::Reviewer,
    );
    let claim_id = id(&f.env, 11);
    let evidence = vec![&f.env, id(&f.env, 43)];
    f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 85),
        &0,
        &100,
        &Category::Travel,
        &evidence,
    );

    assert_eq!(
        f.client
            .try_approve_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &claim_id,
                &id(&f.env, 86),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::SelfReviewNotAllowed
    );
    let first = f.client.approve_claim(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 87),
        &1,
    );
    assert_eq!(first.state, ClaimState::Submitted);
    assert_eq!(first.approvals, 1);
    let second = f.client.approve_claim(
        &f.reviewer2,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 88),
        &2,
    );
    assert_eq!(second.state, ClaimState::Approved);
    assert_eq!(second.approvals, 2);
    assert!(f.client.has_approved(&f.tenant, &f.project, &claim_id, &f.reviewer));
    assert!(!f.client.has_approved(&f.tenant, &f.project, &claim_id, &f.claimant));
}

#[test]
fn policy_and_evidence_constraints_are_enforced() {
    let f = setup();
    default_policy(&f);
    let evidence = vec![&f.env, id(&f.env, 44)];
    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 90),
                &0,
                &1_001,
                &Category::Other,
                &evidence,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidAmount
    );
    let empty = vec![&f.env];
    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 91),
                &0,
                &100,
                &Category::Other,
                &empty,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidEvidence
    );
    let too_many = vec![
        &f.env,
        id(&f.env, 44),
        id(&f.env, 45),
        id(&f.env, 46),
        id(&f.env, 47),
    ];
    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 92),
                &0,
                &100,
                &Category::Other,
                &too_many,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidEvidence
    );
    f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &id(&f.env, 12),
        &id(&f.env, 93),
        &0,
        &100,
        &Category::Training,
        &evidence,
    );
    assert_eq!(
        f.client
            .try_reject_claim(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 94),
                &1,
                &5,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidReason
    );
}

#[test]
fn replay_is_cached_and_stale_writes_fail() {
    let f = setup();
    default_policy(&f);
    let claim_id = id(&f.env, 13);
    let request_id = id(&f.env, 95);
    let evidence = vec![&f.env, id(&f.env, 48)];
    let first = f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &claim_id,
        &request_id,
        &0,
        &120,
        &Category::Software,
        &evidence,
    );
    let replay = f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &claim_id,
        &request_id,
        &0,
        &120,
        &Category::Software,
        &evidence,
    );
    assert_eq!(first, replay);
    assert_eq!(f.client.claim_count(&f.tenant, &f.project), 1);
    assert_eq!(f.env.events().all().len(), 0);

    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &claim_id,
                &request_id,
                &9,
                &120,
                &Category::Software,
                &evidence,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(
        f.client
            .try_approve_claim(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &claim_id,
                &id(&f.env, 96),
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
}

#[test]
fn rejected_claim_cannot_be_approved_or_paid() {
    let f = setup();
    default_policy(&f);
    let claim_id = id(&f.env, 14);
    let evidence = vec![&f.env, id(&f.env, 49)];
    f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 97),
        &0,
        &90,
        &Category::Materials,
        &evidence,
    );
    let rejected = f.client.reject_claim(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &claim_id,
        &id(&f.env, 98),
        &1,
        &2,
    );
    assert_eq!(rejected.state, ClaimState::Rejected);
    assert_eq!(
        f.client
            .try_approve_claim(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &claim_id,
                &id(&f.env, 99),
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    assert_eq!(
        f.client
            .try_link_payout(
                &f.finance,
                &f.tenant,
                &f.project,
                &claim_id,
                &id(&f.env, 100),
                &2,
                &id(&f.env, 50),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
}

#[test]
fn cross_tenant_roles_do_not_authorize_claims() {
    let f = setup();
    default_policy(&f);
    let other_tenant = id(&f.env, 91);
    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &other_tenant,
                &f.project,
                &id(&f.env, 15),
                &id(&f.env, 101),
                &0,
                &100,
                &Category::Other,
                &vec![&f.env, id(&f.env, 51)],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(!f.client.has_role(&other_tenant, &f.project, &f.claimant, &Role::Claimant));
}

#[test]
fn bounded_claim_index_rejects_next_claim() {
    let f = setup();
    f.client.configure_policy(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 102),
        &0,
        &1_000,
        &1,
        &1,
    );
    for n in 1u8..=16u8 {
        f.client.submit_claim(
            &f.claimant,
            &f.tenant,
            &f.project,
            &id(&f.env, n),
            &id(&f.env, 110 + n),
            &0,
            &50,
            &Category::Other,
            &vec![&f.env, id(&f.env, 60 + n)],
        );
    }
    assert_eq!(f.client.claim_count(&f.tenant, &f.project), 16);
    assert_eq!(
        f.client
            .try_submit_claim(
                &f.claimant,
                &f.tenant,
                &f.project,
                &id(&f.env, 17),
                &id(&f.env, 130),
                &0,
                &50,
                &Category::Other,
                &vec![&f.env, id(&f.env, 140)],
            )
            .unwrap_err()
            .unwrap(),
        ContractError::IndexFull
    );
}

#[test]
fn resource_budget_stays_within_transaction_limits() {
    let f = setup();
    default_policy(&f);
    f.env.cost_estimate().budget().reset_default();
    f.client.submit_claim(
        &f.claimant,
        &f.tenant,
        &f.project,
        &id(&f.env, 18),
        &id(&f.env, 150),
        &0,
        &100,
        &Category::Software,
        &vec![&f.env, id(&f.env, 151), id(&f.env, 152)],
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
