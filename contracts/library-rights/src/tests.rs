use crate::{ContractError, LibraryRightsContract};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryRightsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

// -- Positive --

#[test]
fn test_initialize_sets_admin() {
    let (env, contract_id, admin) = setup();
    env.mock_all_auths();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

    client.initialize(&admin);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_version_reports_current_abi() {
    let (env, contract_id, _admin) = setup();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

    assert_eq!(client.version(), String::from_str(&env, "0.1.0"));
}

// -- Negative --

#[test]
fn test_get_admin_before_initialize_fails() {
    let (env, contract_id, _admin) = setup();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

    let result = client.try_get_admin();

    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}

// -- Authorization --

#[test]
fn test_initialize_requires_admin_auth() {
    let (env, contract_id, admin) = setup();
    // No mock_all_auths(): the admin's own signature must be required.
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);

    let result = client.try_initialize(&admin);

    assert!(result.is_err());
}

// -- Boundary (single-use initialization) --

#[test]
fn test_second_initialize_call_rejected() {
    let (env, contract_id, admin) = setup();
    env.mock_all_auths();
    let client = crate::LibraryRightsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let other = Address::generate(&env);
    let result = client.try_initialize(&other);

    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
    // The original admin must remain unchanged after the rejected call.
    assert_eq!(client.get_admin(), admin);
}
