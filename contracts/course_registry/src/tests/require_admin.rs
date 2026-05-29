#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

use crate::{ContractError, CourseRegistryContract, CourseRegistryContractClient};

/// Verifies that calling an admin-gated function before initialize()
/// returns ContractError::NotInitialized instead of panicking.
#[test]
fn require_admin_returns_not_initialized_before_init() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CourseRegistryContract);
    let client = CourseRegistryContractClient::new(&env, &contract_id);

    let result = client.try_upsert_course(
        &Symbol::new(&env, "course1"),
        &100,
        &50,
        &true,
    );

    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}

/// Verifies that after initialization, admin-gated functions succeed.
#[test]
fn require_admin_succeeds_after_init() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CourseRegistryContract);
    let client = CourseRegistryContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin).unwrap();

    let result = client.try_upsert_course(
        &Symbol::new(&env, "course1"),
        &100,
        &50,
        &true,
    );

    assert!(result.is_ok());
}