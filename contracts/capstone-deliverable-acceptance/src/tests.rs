use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{
    CapstoneDeliverableAcceptanceContract, CapstoneDeliverableAcceptanceContractClient,
    ContractError, DecisionKind, Role,
};

fn setup() -> (
    Env,
    Address,
    Address,
    CapstoneDeliverableAcceptanceContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(500);
    let id = env.register(CapstoneDeliverableAcceptanceContract, ());
    let client = CapstoneDeliverableAcceptanceContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let reviewer = Address::generate(&env);
    client.initialize(&admin);
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    client.set_role(&admin, &reviewer, &tenant, &project, &Role::Reviewer, &true);
    (env, admin, reviewer, client)
}

#[test]
fn reviewer_decision_is_idempotent_and_denies_unauthorized() {
    let (env, _admin, reviewer, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let milestone = BytesN::from_array(&env, &[3u8; 32]);
    let deliverable = BytesN::from_array(&env, &[4u8; 32]);
    let rubric = BytesN::from_array(&env, &[5u8; 32]);
    let reason = BytesN::from_array(&env, &[6u8; 32]);
    let d1 = client.decide(
        &reviewer,
        &tenant,
        &project,
        &milestone,
        &deliverable,
        &1,
        &rubric,
        &reason,
        &DecisionKind::Accept,
        &0,
    );
    let replay = client.decide(
        &reviewer,
        &tenant,
        &project,
        &milestone,
        &deliverable,
        &1,
        &rubric,
        &reason,
        &DecisionKind::Accept,
        &0,
    );
    assert_eq!(d1, replay);
    let stranger = Address::generate(&env);
    let err = client
        .try_decide(
            &stranger,
            &tenant,
            &project,
            &milestone,
            &BytesN::from_array(&env, &[7u8; 32]),
            &1,
            &rubric,
            &reason,
            &DecisionKind::Reject,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn conflicting_redecision_is_rejected() {
    let (env, _admin, reviewer, client) = setup();
    let tenant = BytesN::from_array(&env, &[1u8; 32]);
    let project = BytesN::from_array(&env, &[2u8; 32]);
    let milestone = BytesN::from_array(&env, &[3u8; 32]);
    let deliverable = BytesN::from_array(&env, &[8u8; 32]);
    let rubric = BytesN::from_array(&env, &[9u8; 32]);
    let reason = BytesN::from_array(&env, &[10u8; 32]);
    client.decide(&reviewer, &tenant, &project, &milestone, &deliverable, &2, &rubric, &reason, &DecisionKind::RequestChanges, &0);
    let conflict = client
        .try_decide(&reviewer, &tenant, &project, &milestone, &deliverable, &2, &rubric, &BytesN::from_array(&env, &[11u8; 32]), &DecisionKind::Accept, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(conflict, ContractError::AlreadyDecided);
}
