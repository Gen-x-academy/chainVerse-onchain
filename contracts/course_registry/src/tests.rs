#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol};
use crate::{CourseRegistryContract, ContractError};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CourseRegistryContract);
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

#[test]
fn test_admin_can_upsert_course() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "RUST101");
    let title = String::from_str(&env, "Intro to Rust");
    let result = client.try_upsert_course(&admin, &course_id, &title, &1000_i128, &true);
    assert!(result.is_ok());
}

#[test]
fn test_non_admin_cannot_upsert_course() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let attacker = Address::generate(&env);
    let course_id = Symbol::new(&env, "HACK");
    let title = String::from_str(&env, "Bad Course");
    let result = client.try_upsert_course(&attacker, &course_id, &title, &0_i128, &true);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_free_course_price_zero_accepted() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "FREE101");
    let title = String::from_str(&env, "Free Course");
    let result = client.try_upsert_course(&admin, &course_id, &title, &0_i128, &true);
    assert!(result.is_ok());
}

#[test]
fn test_deactivate_course_sets_inactive() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "DEACT");
    let title = String::from_str(&env, "To Deactivate");
    client.upsert_course(&admin, &course_id, &title, &100_i128, &true);
    client.deactivate_course(&admin, &course_id);
    let course = client.get_course(&course_id).unwrap();
    assert!(!course.is_active);
}
