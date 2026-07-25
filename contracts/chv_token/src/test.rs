#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{CHVToken, TokenError};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CHVToken);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    (env, contract_id, admin, treasury)
}

#[test]
fn test_initialize_sets_treasury_balance() {
    let (env, contract_id, admin, treasury) = setup();
    let client = crate::CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    assert!(client.balance(&treasury) > 0);
}

#[test]
fn test_transfer_moves_tokens() {
    let (env, contract_id, admin, treasury) = setup();
    let client = crate::CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let recipient = Address::generate(&env);
    client.transfer(&treasury, &recipient, &1000_i128);
    assert_eq!(client.balance(&recipient), 1000);
}

#[test]
fn test_self_transfer_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = crate::CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let result = client.try_transfer(&treasury, &treasury, &100_i128);
    assert_eq!(result, Err(Ok(TokenError::SelfTransfer)));
}

#[test]
fn test_transfer_insufficient_balance_rejected() {
    let (env, contract_id, admin, _) = setup();
    let client = crate::CHVTokenClient::new(&env, &contract_id);
    let treasury = Address::generate(&env);
    client.initialize(&admin, &treasury);
    let user = Address::generate(&env);
    let result = client.try_transfer(&user, &treasury, &100_i128);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn test_negative_transfer_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = crate::CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let recipient = Address::generate(&env);
    let result = client.try_transfer(&treasury, &recipient, &(-1_i128));
    assert_eq!(result, Err(Ok(TokenError::InvalidAmount)));
}
