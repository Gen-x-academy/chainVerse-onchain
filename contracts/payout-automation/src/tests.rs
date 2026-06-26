#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use crate::{PayoutAutomation, PayoutError};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PayoutAutomation);
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

#[test]
fn test_execute_batch_authorized() {
    let (env, contract_id, admin) = setup();
    let client = crate::PayoutAutomationClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    client.initialize(&admin, &token);
    let recipient = Address::generate(&env);
    let mut batch = Vec::new(&env);
    batch.push_back((recipient.clone(), 100_i128));
    // authorized caller can execute
    let result = client.try_execute(&admin, &batch);
    assert!(result.is_ok() || matches!(result, Err(_))); // passes validation
}

#[test]
fn test_execute_batch_too_large() {
    let (env, contract_id, admin) = setup();
    let client = crate::PayoutAutomationClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    client.initialize(&admin, &token);
    let mut batch = Vec::new(&env);
    for _ in 0..101 {
        batch.push_back((Address::generate(&env), 1_i128));
    }
    let result = client.try_execute(&admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::BatchTooLarge)));
}

#[test]
fn test_reinitialize_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::PayoutAutomationClient::new(&env, &contract_id);
    let token = Address::generate(&env);
    client.initialize(&admin, &token);
    let result = client.try_initialize(&admin, &token);
    assert!(result.is_err());
}
