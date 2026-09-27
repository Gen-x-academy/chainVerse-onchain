#![cfg(test)]

use crate::{ContractError, MetricField, ScholarshipAggregateAnalytics};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup(env: &Env) -> (Address, Address, soroban_sdk::Address) {
    let cid = env.register(ScholarshipAggregateAnalytics, ());
    let admin = Address::generate(env);
    let recorder = Address::generate(env);
    let client = crate::ScholarshipAggregateAnalyticsClient::new(env, &cid);
    client.init(&admin, &recorder, &10).unwrap();
    (admin, recorder, cid)
}

/// Smoke: init → create cohort → increment → get metrics visible.
#[test]
fn smoke_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, recorder, cid) = setup(&env);
    let client = crate::ScholarshipAggregateAnalyticsClient::new(&env, &cid);
    let tag = String::from_str(&env, "smoke");
    client.create_cohort(&recorder, &tag).unwrap();
    client
        .increment(&recorder, &tag, &MetricField::TotalApplicants, &10)
        .unwrap();
    let r = client.get_cohort_metrics(&tag).unwrap();
    assert!(matches!(r, crate::CohortResult::Metrics(_)));
}

/// Unit: not initialised → Unauthorized (no admin stored → NotInitialized).
#[test]
fn requires_init_before_create_cohort() {
    let env = Env::default();
    env.mock_all_auths();
    let cid = env.register(ScholarshipAggregateAnalytics, ());
    let client = crate::ScholarshipAggregateAnalyticsClient::new(&env, &cid);
    let result = client.try_create_cohort(
        &Address::generate(&env),
        &String::from_str(&env, "c"),
    );
    assert_eq!(result, Err(Ok(ContractError::NotInitialized)));
}
