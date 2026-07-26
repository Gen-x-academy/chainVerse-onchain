#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env, String};
use crate::{StakingContract, StakingError};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    (env, contract_id, admin, token)
}

#[test]
fn test_initialize_rejects_zero_penalty() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &0u32);
    assert_eq!(result, Err(Ok(StakingError::PenaltyTooLow)));
}

#[test]
fn test_initialize_accepts_valid_penalty() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &500u32);
    assert!(result.is_ok());
}

#[test]
fn test_reinitialize_rejected() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let result = client.try_initialize(&admin, &token, &500u32);
    assert_eq!(result, Err(Ok(StakingError::AlreadyInitialized)));
}

#[test]
fn test_stake_requires_valid_tier() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    let tier = String::from_str(&env, "gold");
    let result = client.try_stake_tokens(&user, &tier, &1000_i128);
    assert_eq!(result, Err(Ok(StakingError::TierNotFound)));
}
