#![cfg(test)]

use crate::{
    ContractError, ImpactInterpretation, MetricField, ScholarshipAggregateAnalytics,
    SponsorImpactSubmission,
};
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

#[test]
fn sponsor_impact_publishes_only_releasable_immutable_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, recorder, cid) = setup(&env);
    let client = crate::ScholarshipAggregateAnalyticsClient::new(&env, &cid);
    let tag = String::from_str(&env, "2026-q1-stem");
    client.create_cohort(&recorder, &tag).unwrap();

    let too_small = SponsorImpactSubmission {
        cohort_tag: tag.clone(),
        recipient_count: 10,
        funded_learning_count: 10,
        completed_count: 4,
        credentials_issued_count: 0,
        funded_award_count: 10,
        total_funded_amount: 50_000,
    };
    assert_eq!(
        client.try_publish_sponsor_impact(&recorder, &too_small),
        Err(Ok(ContractError::CohortSuppressed))
    );
    assert!(client.get_sponsor_impact(&tag).is_none());

    let releasable = SponsorImpactSubmission {
        completed_count: 10,
        ..too_small
    };
    client
        .publish_sponsor_impact(&recorder, &releasable)
        .unwrap();
    let report = client.get_sponsor_impact(&tag).unwrap();
    assert_eq!(report.recipient_count, 10);
    assert_eq!(report.funded_learning_count, 10);
    assert_eq!(report.completed_count, 10);
    assert_eq!(report.credentials_issued_count, 0);
    assert_eq!(report.total_funded_amount, 50_000);
    assert_eq!(
        report.interpretation,
        ImpactInterpretation::DescriptiveAssociationOnly
    );
    assert_eq!(
        client.try_publish_sponsor_impact(&recorder, &releasable),
        Err(Ok(ContractError::ImpactReportAlreadyPublished))
    );
    client.set_k_threshold(&_admin, &11).unwrap();
    assert!(client.get_sponsor_impact(&tag).is_none());
}

#[test]
fn sponsor_impact_requires_minimum_recipient_cohort() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, recorder, cid) = setup(&env);
    let client = crate::ScholarshipAggregateAnalyticsClient::new(&env, &cid);
    let tag = String::from_str(&env, "small-recipient-cohort");
    client.create_cohort(&recorder, &tag).unwrap();

    let submission = SponsorImpactSubmission {
        cohort_tag: tag,
        recipient_count: 4,
        funded_learning_count: 0,
        completed_count: 0,
        credentials_issued_count: 0,
        funded_award_count: 0,
        total_funded_amount: 0,
    };
    let result = client.try_publish_sponsor_impact(&recorder, &submission);
    assert_eq!(result, Err(Ok(ContractError::CohortSuppressed)));
}
