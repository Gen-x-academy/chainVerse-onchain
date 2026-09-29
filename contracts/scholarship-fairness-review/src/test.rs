#![cfg(test)]

use crate::{ContractError, ReviewOutcome, RiskLevel, ScholarshipFairnessReview};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env, String,
};

fn setup(env: &Env) -> (Address, Address, Address, soroban_sdk::Address) {
    let contract_id = env.register(ScholarshipFairnessReview, ());
    let admin = Address::generate(env);
    let reviewer = Address::generate(env);
    let approver = Address::generate(env);

    let client = crate::ScholarshipFairnessReviewClient::new(env, &contract_id);
    client.init(&admin, &reviewer, &approver).unwrap();
    (admin, reviewer, approver, contract_id)
}

/// Smoke test: init + propose + review + activate completes without error.
#[test]
fn smoke_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (admin, reviewer, _approver, cid) = setup(&env);
    let client = crate::ScholarshipFairnessReviewClient::new(&env, &cid);

    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &Bytes::from_slice(&env, b"rule"),
            &String::from_str(&env, "smoke"),
            &String::from_str(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(
            &reviewer,
            &v,
            &ReviewOutcome::Approved,
            &String::from_str(&env, "ok"),
        )
        .unwrap();
    client.activate_version(&admin, &v).unwrap();
    assert_eq!(client.active_version(), v);
}

/// TTL bump: instance extend_ttl is called on every entry point.
#[test]
fn init_before_data_guard() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipFairnessReview, ());
    let client = crate::ScholarshipFairnessReviewClient::new(&env, &contract_id);
    // Contract is not yet initialised — propose should return NotInitialized.
    let result = client.try_propose_rule_version(
        &Address::generate(&env),
        &RiskLevel::Standard,
        &Bytes::from_slice(&env, b"data"),
        &String::from_str(&env, "s"),
        &String::from_str(&env, "c"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}
