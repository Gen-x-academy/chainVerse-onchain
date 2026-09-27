#![cfg(test)]
extern crate std;

use scholarship_aggregate_analytics::{
    ContractError, CohortResult, DisbursementBatch, MetricField,
    ScholarshipAggregateAnalytics, ScholarshipAggregateAnalyticsClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env, String, Vec,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, ScholarshipAggregateAnalyticsClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ScholarshipAggregateAnalytics, ());
    let client = ScholarshipAggregateAnalyticsClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let recorder = Address::generate(&env);

    // k_threshold = 10 for most tests
    client.init(&admin, &recorder, &10).unwrap();

    (env, admin, recorder, client)
}

fn tag(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

// ── Initialisation ─────────────────────────────────────────────────────────────

#[test]
fn init_sets_k_threshold() {
    let (_env, _admin, _recorder, client) = setup();
    assert_eq!(client.k_threshold(), 10);
}

#[test]
fn init_enforces_minimum_k_threshold_of_5() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAggregateAnalytics, ());
    let client = ScholarshipAggregateAnalyticsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recorder = Address::generate(&env);

    // Pass k=1 — should be floored to 5
    client.init(&admin, &recorder, &1).unwrap();
    assert_eq!(client.k_threshold(), 5);
}

#[test]
fn init_rejects_double_init() {
    let (env, admin, recorder, client) = setup();
    let result = client.try_init(&admin, &recorder, &10);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

// ── Cohort creation ────────────────────────────────────────────────────────────

#[test]
fn create_cohort_idempotent() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "2026-q1");
    client.create_cohort(&recorder, &t).unwrap();
    // Second call must not error
    client.create_cohort(&recorder, &t).unwrap();
}

#[test]
fn create_cohort_rejects_long_tag() {
    let (env, _admin, recorder, client) = setup();
    let long = String::from_str(&env, "a-very-long-cohort-tag-that-is-definitely-more-than-sixty-four-bytes-long");
    let result = client.try_create_cohort(&recorder, &long);
    assert_eq!(result, Err(Ok(ContractError::CohortTagTooLong)));
}

#[test]
fn create_cohort_unauthorized_for_non_recorder() {
    let (env, _admin, _recorder, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_create_cohort(&stranger, &tag(&env, "cohort"));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── k-anonymity suppression ────────────────────────────────────────────────────

#[test]
fn cohort_suppressed_below_threshold() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "small-cohort");
    client.create_cohort(&recorder, &t).unwrap();

    // Add 9 applicants (below k=10)
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &9)
        .unwrap();

    let result = client.get_cohort_metrics(&t).unwrap();
    assert!(
        matches!(result, CohortResult::Suppressed(_)),
        "must be suppressed when total < k_threshold"
    );
}

#[test]
fn cohort_visible_at_or_above_threshold() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "visible-cohort");
    client.create_cohort(&recorder, &t).unwrap();
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &10)
        .unwrap();

    let result = client.get_cohort_metrics(&t).unwrap();
    assert!(
        matches!(result, CohortResult::Metrics(_)),
        "must be visible at exactly k_threshold"
    );
}

#[test]
fn raising_k_threshold_hides_previously_visible_cohort() {
    let (env, admin, recorder, client) = setup();
    let t = tag(&env, "threshold-test");
    client.create_cohort(&recorder, &t).unwrap();
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &10)
        .unwrap();

    // visible at k=10
    assert!(matches!(client.get_cohort_metrics(&t).unwrap(), CohortResult::Metrics(_)));

    // raise to k=20
    client.set_k_threshold(&admin, &20).unwrap();
    assert!(matches!(
        client.get_cohort_metrics(&t).unwrap(),
        CohortResult::Suppressed(_)
    ));
}

// ── Counter operations ─────────────────────────────────────────────────────────

#[test]
fn increment_all_fields() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "all-fields");
    client.create_cohort(&recorder, &t).unwrap();

    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &20)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::EligibleCount, &18)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::ReviewCompletedCount, &15)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::ApprovedCount, &10)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::RejectedCount, &5)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::WithdrawnCount, &3)
        .unwrap();
    client
        .increment(&recorder, &t, &MetricField::DisbursedCount, &10)
        .unwrap();

    let result = client.get_cohort_metrics(&t).unwrap();
    if let CohortResult::Metrics(m) = result {
        assert_eq!(m.total_applicants, 20);
        assert_eq!(m.eligible_count, 18);
        assert_eq!(m.review_completed_count, 15);
        assert_eq!(m.approved_count, 10);
        assert_eq!(m.rejected_count, 5);
        assert_eq!(m.withdrawn_count, 3);
        assert_eq!(m.disbursed_count, 10);
    } else {
        panic!("expected Metrics variant");
    }
}

#[test]
fn decrement_corrects_over_count() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "decrement-test");
    client.create_cohort(&recorder, &t).unwrap();
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &15)
        .unwrap();
    // Over-counted by 2; correct it
    client
        .decrement(&recorder, &t, &MetricField::TotalApplicants, &2)
        .unwrap();

    if let CohortResult::Metrics(m) = client.get_cohort_metrics(&t).unwrap() {
        assert_eq!(m.total_applicants, 13);
    } else {
        panic!("expected Metrics variant");
    }
}

#[test]
fn decrement_below_zero_returns_underflow() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "underflow-test");
    client.create_cohort(&recorder, &t).unwrap();
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &5)
        .unwrap();
    let result =
        client.try_decrement(&recorder, &t, &MetricField::TotalApplicants, &10);
    assert_eq!(result, Err(Ok(ContractError::CounterUnderflow)));
}

#[test]
fn increment_nonexistent_cohort_fails() {
    let (env, _admin, recorder, client) = setup();
    let result = client.try_increment(
        &recorder,
        &tag(&env, "ghost"),
        &MetricField::TotalApplicants,
        &1,
    );
    assert_eq!(result, Err(Ok(ContractError::CohortNotFound)));
}

// ── Disbursements ──────────────────────────────────────────────────────────────

#[test]
fn record_disbursements_accumulates_total() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "disbursement-cohort");
    client.create_cohort(&recorder, &t).unwrap();
    // Bring total_applicants above threshold
    client
        .increment(&recorder, &t, &MetricField::TotalApplicants, &12)
        .unwrap();

    let batch = DisbursementBatch {
        cohort_tag: t.clone(),
        amounts: vec![&env, 1_000_i128, 2_000_i128, 3_000_i128],
    };
    client.record_disbursements(&recorder, &batch).unwrap();

    if let CohortResult::Metrics(m) = client.get_cohort_metrics(&t).unwrap() {
        assert_eq!(m.disbursed_count, 3);
        assert_eq!(m.total_disbursed_amount, 6_000);
    } else {
        panic!("expected Metrics");
    }
}

#[test]
fn disbursement_batch_for_unknown_cohort_fails() {
    let (env, _admin, recorder, client) = setup();
    let batch = DisbursementBatch {
        cohort_tag: tag(&env, "ghost"),
        amounts: vec![&env, 500_i128],
    };
    let result = client.try_record_disbursements(&recorder, &batch);
    assert_eq!(result, Err(Ok(ContractError::CohortNotFound)));
}

// ── Metric definitions ─────────────────────────────────────────────────────────

#[test]
fn upsert_and_retrieve_metric_definition() {
    let (env, admin, _recorder, client) = setup();
    client
        .upsert_metric_definition(
            &admin,
            &tag(&env, "approved"),
            &tag(&env, "Approved Count"),
            &String::from_str(
                &env,
                "Number of applicants who received a positive scholarship decision.",
            ),
        )
        .unwrap();

    let defn = client
        .get_metric_definition(&tag(&env, "approved"))
        .unwrap();
    assert_eq!(defn.name, tag(&env, "Approved Count"));
}

#[test]
fn metric_definition_key_too_long_fails() {
    let (env, admin, _recorder, client) = setup();
    let long_key = String::from_str(&env, "a-key-that-is-more-than-thirty-two-bytes-long");
    let result = client.try_upsert_metric_definition(
        &admin,
        &long_key,
        &tag(&env, "name"),
        &tag(&env, "definition"),
    );
    assert_eq!(result, Err(Ok(ContractError::MetricKeyTooLong)));
}

// ── Pause / admin ──────────────────────────────────────────────────────────────

#[test]
fn pause_blocks_create_cohort() {
    let (env, admin, recorder, client) = setup();
    client.pause(&admin).unwrap();
    let result = client.try_create_cohort(&recorder, &tag(&env, "blocked"));
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn pause_blocks_increment() {
    let (env, admin, recorder, client) = setup();
    let t = tag(&env, "p-cohort");
    client.create_cohort(&recorder, &t).unwrap();
    client.pause(&admin).unwrap();
    let result = client.try_increment(&recorder, &t, &MetricField::TotalApplicants, &1);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn only_admin_can_pause() {
    let (env, _admin, _recorder, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_pause(&stranger);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn set_k_threshold_below_minimum_is_floored() {
    let (env, admin, _recorder, client) = setup();
    client.set_k_threshold(&admin, &2).unwrap();
    assert_eq!(client.k_threshold(), 5, "should be floored to MINIMUM_K_THRESHOLD=5");
}

// ── Adversarial ────────────────────────────────────────────────────────────────

#[test]
fn non_recorder_cannot_increment() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "auth-check");
    client.create_cohort(&recorder, &t).unwrap();
    let attacker = Address::generate(&env);
    let result = client.try_increment(&attacker, &t, &MetricField::TotalApplicants, &1);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn non_recorder_cannot_record_disbursements() {
    let (env, _admin, recorder, client) = setup();
    let t = tag(&env, "dis-auth");
    client.create_cohort(&recorder, &t).unwrap();
    let attacker = Address::generate(&env);
    let batch = DisbursementBatch {
        cohort_tag: t,
        amounts: vec![&env, 100_i128],
    };
    let result = client.try_record_disbursements(&attacker, &batch);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn get_nonexistent_cohort_returns_error() {
    let (env, _admin, _recorder, client) = setup();
    let result = client.try_get_cohort_metrics(&tag(&env, "does-not-exist"));
    assert_eq!(result, Err(Ok(ContractError::CohortNotFound)));
}
