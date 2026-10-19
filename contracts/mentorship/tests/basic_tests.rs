#![cfg(test)]

use mentorship::{EngagementStatus, MentorshipContract, MentorshipContractClient};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

#[test]
fn test_init() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, MentorshipContract);
    let client = MentorshipContractClient::new(&env, &contract_id);

    client.init(&admin);
}

#[test]
fn test_create_basic_engagement() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register_contract(None, MentorshipContract);
    let client = MentorshipContractClient::new(&env, &contract_id);

    client.init(&admin);

    let agreement_hash = BytesN::from_array(&env, &[1u8; 32]);
    
    let engagement_id = client.create_engagement(
        &mentor,
        &mentee,
        &agreement_hash,
        &1000i128,
        &token,
        &10u32,
        &false,
    );

    assert_eq!(engagement_id, 1);
    
    let engagement = client.get_engagement(&engagement_id);
    assert_eq!(engagement.status, EngagementStatus::Active);
}
