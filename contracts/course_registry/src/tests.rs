#![cfg(test)]
use crate::CourseRegistryContract;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CourseRegistryContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

#[test]
fn test_admin_can_upsert_course() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "RUST101");
    let result = client.try_upsert_course(&course_id, &1000_i128, &0_i128, &true);
    assert!(result.is_ok());
}

#[test]
fn test_non_admin_cannot_upsert_course() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let _attacker = Address::generate(&env);
    let course_id = Symbol::new(&env, "HACK");
    let result = client.try_upsert_course(&course_id, &0_i128, &0_i128, &true);
    assert!(result.is_ok());
}

#[test]
fn test_free_course_price_zero_accepted() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "FREE101");
    let result = client.try_upsert_course(&course_id, &0_i128, &0_i128, &true);
    assert!(result.is_ok());
}

#[test]
fn test_deactivate_course_sets_inactive() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let course_id = Symbol::new(&env, "DEACT");
    client.upsert_course(&course_id, &100_i128, &0_i128, &true);
    client.deactivate_course(&course_id);
    let course = client.get_course(&course_id);
    assert!(!course.is_active);
}

#[test]
fn test_typed_enrollment_interface_round_trip() {
    let (env, contract_id, admin) = setup();
    let client = crate::CourseRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let student = Address::generate(&env);
    let course_id = Symbol::new(&env, "RUST101");
    assert!(!client.is_enrolled(&student, &course_id));
    client.set_enrollment(&student, &course_id, &true);
    assert!(client.is_enrolled(&student, &course_id));
    client.set_enrollment(&student, &course_id, &false);
    assert!(!client.is_enrolled(&student, &course_id));
}
