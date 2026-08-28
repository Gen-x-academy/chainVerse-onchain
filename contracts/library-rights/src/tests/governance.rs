use crate::{ContractError, LibraryRightsContractClient, Role};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn bootstrap_addrs(env: &Env) -> (Address, Address, Address, Address) {
    (
        Address::generate(env),
        Address::generate(env),
        Address::generate(env),
        Address::generate(env),
    )
}

// -- Positive --

#[test]
fn test_bootstrap_assigns_all_roles() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let (admin, treasury, policy_manager, emergency) = bootstrap_addrs(&env);

    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);

    assert_eq!(client.get_role(&Role::Admin), admin);
    assert_eq!(client.get_role(&Role::Treasury), treasury);
    assert_eq!(client.get_role(&Role::PolicyManager), policy_manager);
    assert_eq!(client.get_role(&Role::Emergency), emergency);
}

// -- Negative --

#[test]
fn test_get_role_before_bootstrap_fails() {
    let (env, contract_id) = super::setup();
    let client = LibraryRightsContractClient::new(&env, &contract_id);

    let result = client.try_get_role(&Role::Admin);

    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}

#[test]
fn test_bootstrap_rejects_duplicate_role_address() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let (admin, treasury, policy_manager, _emergency) = bootstrap_addrs(&env);

    // Reuse `admin` for the emergency role.
    let result = client.try_bootstrap(&admin, &treasury, &policy_manager, &admin);

    assert_eq!(result, Err(Ok(ContractError::DuplicateRole)));
}

// -- Authorization --

#[test]
fn test_bootstrap_requires_every_role_auth() {
    let (env, contract_id) = super::setup();
    // No mock_all_auths(): every role address must sign off on its own
    // assignment.
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let (admin, treasury, policy_manager, emergency) = bootstrap_addrs(&env);

    let result = client.try_bootstrap(&admin, &treasury, &policy_manager, &emergency);

    assert!(result.is_err());
}

// -- Boundary --

#[test]
fn test_second_bootstrap_call_rejected() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let (admin, treasury, policy_manager, emergency) = bootstrap_addrs(&env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);

    let (admin2, treasury2, policy_manager2, emergency2) = bootstrap_addrs(&env);
    let result = client.try_bootstrap(&admin2, &treasury2, &policy_manager2, &emergency2);

    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
    // The original roles must remain unchanged after the rejected call.
    assert_eq!(client.get_role(&Role::Admin), admin);
}
