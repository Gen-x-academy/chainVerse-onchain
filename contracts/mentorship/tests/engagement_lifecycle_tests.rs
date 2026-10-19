#![cfg(test)]

use mentorship::{
    Engagement, EngagementStatus, MentorshipContract, MentorshipContractClient,
    MentorshipError, TerminationReason,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

fn create_test_env() -> (Env, Address, Address, Address, Address, MentorshipContractClient) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register_contract(None, MentorshipContract);
    let client = MentorshipContractClient::new(&env, &contract_id);

    client.init(&admin);

    (env, admin, mentor, mentee, token, client)
}

#[test]
fn test_create_engagement_without_consent() {
    let (env, _admin, mentor, mentee, token, client) = create_test_env();

    let agreement_hash = BytesN::from_array(&env, &[1u8; 32]);
    let payment = 1000i128;
    let sessions = 10u32;

    let engagement_id = client.create_engagement(
        &mentor,
        &mentee,
        &agreement_hash,
        &payment,
        &token,
        &sessions,
        &false,
    );

    assert_eq!(engagement_id, 1);

    let engagement = client.get_engagement(&engagement_id);
    assert_eq!(engagement.mentor, mentor);
    assert_eq!(engagement.mentee, mentee);
    assert_eq!(engagement.status, EngagementStatus::Active);
}
