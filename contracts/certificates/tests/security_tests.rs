#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Env, Address};
use crate::{CourseContract, CourseContractClient};
use crate::errors::Error;

#[test]
#[should_panic(expected = "Unauthorized")]
fn test_replay_attack_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, CourseContract);
    let client = CourseContractClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let course_id = 1;

    // No auth provided
    client.purchase(&user, &course_id, &100i128);

    // Should panic because require_auth() not satisfied
}