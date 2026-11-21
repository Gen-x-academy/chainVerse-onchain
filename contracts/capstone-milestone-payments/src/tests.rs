extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneMilestonePayments, CapstoneMilestonePaymentsClient, ContractError, MilestoneState, Role,
};

const NOW: u64 = 1_000;

struct Fixture {
    env: Env,
    client: CapstoneMilestonePaymentsClient<'static>,
    admin: Address,
    coordinator: Address,
    reviewer: Address,
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
    let contract_id = env.register(CapstoneMilestonePayments, ());
    let client = CapstoneMilestonePaymentsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    let coordinator = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let finance = Address::generate(&env);
    client.grant_role(&admin, &tenant, &project, &coordinator, &Role::Coordinator);
    client.grant_role(&admin, &tenant, &project, &reviewer, &Role::Reviewer);
    client.grant_role(&admin, &tenant, &project, &finance, &Role::Finance);
    Fixture {
        env,
        client,
        admin,
        coordinator,
        reviewer,
        finance,
        tenant,
        project,
    }
}

#[test]
fn lifecycle_releases_deterministic_fee_and_net_amount() {
    let f = setup();
    let milestone_id = id(&f.env, 10);
    let milestone = f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &milestone_id,
        &id(&f.env, 20),
        &0,
        &1_000,
        &250,
        &100,
    );
    assert_eq!(milestone.state, MilestoneState::Configured);
    assert_eq!(milestone.version, 1);

    let accepted = f.client.record_acceptance(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &milestone_id,
        &id(&f.env, 21),
        &1,
        &id(&f.env, 30),
    );
    assert_eq!(accepted.state, MilestoneState::Accepted);
    assert_eq!(accepted.challenge_end, NOW + 100);
    assert_eq!(accepted.version, 2);

    f.env.ledger().set_timestamp(NOW + 99);
    assert_eq!(
        f.client
            .try_release_payment(
                &f.finance,
                &f.tenant,
                &f.project,
                &milestone_id,
                &id(&f.env, 22),
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ChallengeWindowOpen
    );

    f.env.ledger().set_timestamp(NOW + 100);
    let receipt = f.client.release_payment(
        &f.finance,
        &f.tenant,
        &f.project,
        &milestone_id,
        &id(&f.env, 22),
        &2,
    );
    assert_eq!(receipt.gross_amount, 1_000);
    assert_eq!(receipt.fee_amount, 25);
    assert_eq!(receipt.net_amount, 975);
    assert_eq!(receipt.released_at, NOW + 100);
    let stored = f
        .client
        .get_milestone(&f.tenant, &f.project, &milestone_id);
    assert_eq!(stored.state, MilestoneState::Released);
    assert_eq!(stored.version, 3);

    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("MSPPAY"));
    let (event_milestone, event_receipt, gross, fee, net): (BytesN<32>, BytesN<32>, i128, i128, i128) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(event_milestone, milestone_id);
    assert_eq!(event_receipt, id(&f.env, 22));
    assert_eq!((gross, fee, net), (1_000, 25, 975));
}

#[test]
fn scoped_roles_are_required_for_every_mutation() {
    let f = setup();
    let outsider = Address::generate(&f.env);
    let other_tenant = id(&f.env, 90);

    assert_eq!(
        f.client
            .try_configure_milestone(
                &outsider,
                &f.tenant,
                &f.project,
                &id(&f.env, 11),
                &id(&f.env, 23),
                &0,
                &100,
                &0,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &other_tenant,
                &f.project,
                &id(&f.env, 11),
                &id(&f.env, 24),
                &0,
                &100,
                &0,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.admin,
                &f.tenant,
                &f.project,
                &id(&f.env, 11),
                &id(&f.env, 25),
                &0,
                &100,
                &0,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert!(f.client.has_role(&f.tenant, &f.project, &f.coordinator, &Role::Coordinator));
    assert!(!f.client.has_role(&other_tenant, &f.project, &f.coordinator, &Role::Coordinator));
}

#[test]
fn replay_returns_cached_result_without_second_side_effect() {
    let f = setup();
    let milestone_id = id(&f.env, 12);
    let request_id = id(&f.env, 26);
    let first = f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &milestone_id,
        &request_id,
        &0,
        &500,
        &100,
        &10,
    );
    let replay = f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &milestone_id,
        &request_id,
        &0,
        &500,
        &100,
        &10,
    );
    assert_eq!(first, replay);
    assert_eq!(f.client.milestone_count(&f.tenant, &f.project), 1);
    assert_eq!(f.env.events().all().len(), 0);

    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &milestone_id,
                &request_id,
                &7,
                &500,
                &100,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(
        f.client
            .try_record_acceptance(
                &f.reviewer,
                &f.tenant,
                &f.project,
                &milestone_id,
                &id(&f.env, 27),
                &0,
                &id(&f.env, 31),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
}

#[test]
fn challenge_blocks_release_until_resolved() {
    let f = setup();
    let first = id(&f.env, 13);
    f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &first,
        &id(&f.env, 40),
        &0,
        &300,
        &0,
        &100,
    );
    f.client.record_acceptance(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &first,
        &id(&f.env, 41),
        &1,
        &id(&f.env, 50),
    );
    f.client.flag_challenge(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &first,
        &id(&f.env, 42),
        &2,
        &id(&f.env, 51),
    );
    assert_eq!(
        f.client
            .get_milestone(&f.tenant, &f.project, &first)
            .state,
        MilestoneState::Challenged
    );
    assert_eq!(
        f.client
            .try_release_payment(
                &f.finance,
                &f.tenant,
                &f.project,
                &first,
                &id(&f.env, 43),
                &3,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidState
    );
    f.client.resolve_challenge(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &first,
        &id(&f.env, 44),
        &3,
        &false,
        &id(&f.env, 52),
    );
    assert_eq!(
        f.client
            .get_milestone(&f.tenant, &f.project, &first)
            .state,
        MilestoneState::Cancelled
    );

    let second = id(&f.env, 14);
    f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 45),
        &0,
        &300,
        &0,
        &100,
    );
    f.client.record_acceptance(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 46),
        &1,
        &id(&f.env, 53),
    );
    f.client.flag_challenge(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 47),
        &2,
        &id(&f.env, 54),
    );
    f.client.resolve_challenge(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 48),
        &3,
        &true,
        &id(&f.env, 55),
    );
    let receipt = f.client.release_payment(
        &f.finance,
        &f.tenant,
        &f.project,
        &second,
        &id(&f.env, 49),
        &4,
    );
    assert_eq!(receipt.net_amount, 300);
}

#[test]
fn checked_fee_multiplication_overflow_is_actionable() {
    let f = setup();
    let milestone_id = id(&f.env, 15);
    f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &milestone_id,
        &id(&f.env, 60),
        &0,
        &i128::MAX,
        &2,
        &10,
    );
    f.client.record_acceptance(
        &f.reviewer,
        &f.tenant,
        &f.project,
        &milestone_id,
        &id(&f.env, 61),
        &1,
        &id(&f.env, 62),
    );
    f.env.ledger().set_timestamp(NOW + 10);
    assert_eq!(
        f.client
            .try_release_payment(
                &f.finance,
                &f.tenant,
                &f.project,
                &milestone_id,
                &id(&f.env, 63),
                &2,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::ArithmeticOverflow
    );
}

#[test]
fn invalid_configuration_values_are_rejected() {
    let f = setup();
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 16),
                &id(&f.env, 70),
                &0,
                &0,
                &0,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidAmount
    );
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 16),
                &id(&f.env, 71),
                &0,
                &100,
                &10_001,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidFee
    );
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 16),
                &id(&f.env, 72),
                &0,
                &100,
                &0,
                &2_592_001,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidWindow
    );
}

#[test]
fn bounded_milestone_index_rejects_next_item() {
    let f = setup();
    for n in 1u8..=16u8 {
        f.client.configure_milestone(
            &f.coordinator,
            &f.tenant,
            &f.project,
            &id(&f.env, n),
            &id(&f.env, 100 + n),
            &0,
            &100,
            &0,
            &10,
        );
    }
    assert_eq!(f.client.milestone_count(&f.tenant, &f.project), 16);
    assert_eq!(
        f.client
            .try_configure_milestone(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 17),
                &id(&f.env, 120),
                &0,
                &100,
                &0,
                &10,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::IndexFull
    );
}

#[test]
fn resource_budget_stays_within_transaction_limits() {
    let f = setup();
    f.env.cost_estimate().budget().reset_default();
    f.client.configure_milestone(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &id(&f.env, 18),
        &id(&f.env, 121),
        &0,
        &100,
        &100,
        &10,
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 100_000_000);
    assert!(budget.memory_bytes_cost() < 41_943_040);
}

#[test]
fn contract_version_is_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
}

#[test]
fn initialization_is_one_time() {
    let f = setup();
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}
