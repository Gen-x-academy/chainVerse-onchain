#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _}, Address, Env};

fn setup_contract(env: &Env) -> (Address, Address, Address) {
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user1 = Address::generate(env);
    let user2 = Address::generate(env);

    client.initialize(&admin, &1000);

    (admin, user1, user2)
}

#[test]
fn test_total_supply_integrity() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    client.initialize(&admin, &1000);

    assert_eq!(client.total_supply(), 1000);
    assert_eq!(client.balance(&admin), 1000);
}

#[test]
fn test_transfer_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);

    client.transfer(&admin, &user, &300);

    assert_eq!(client.balance(&admin), 700);
    assert_eq!(client.balance(&user), 300);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_transfer_failure_insufficient_balance() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);

    client.transfer(&admin, &user, &2000);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_mint_attempt_after_deployment_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    client.initialize(&admin, &1000);

    // Attempt to re-initialize (simulate mint attempt)
    client.initialize(&admin, &5000);
}