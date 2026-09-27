#![cfg(test)]
use crate::{CompareOutcome, ContractError, Criterion, ScholarshipReviewsContract};
use soroban_sdk::{testutils::Address as _, vec, Address, BytesN, Env, Vec};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipReviewsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn key(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

/// A two-criterion rubric (60% / 40%) whose weights sum to exactly 10_000 bps.
fn criteria(env: &Env) -> Vec<Criterion> {
    vec![
        env,
        Criterion {
            weight_bps: 6_000,
            max_score: 100,
        },
        Criterion {
            weight_bps: 4_000,
            max_score: 50,
        },
    ]
}

fn empty_criteria(env: &Env) -> Vec<Criterion> {
    Vec::new(env)
}

/// Initialize + register a program (min 2 / max 4 reviews) + publish rubric v1.
fn ready(env: &Env, client: &crate::ScholarshipReviewsContractClient, admin: &Address) -> BytesN<32> {
    let pid = key(env, 1);
    client.initialize(admin);
    client.register_program(admin, &pid, &2u32, &4u32);
    client.publish_rubric(admin, &pid, &criteria(env));
    pid
}

// ── rubric publication ──────────────────────────────────────────────────────

#[test]
fn test_publish_rubric_starts_at_version_one() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    assert_eq!(client.get_latest_rubric_version(&pid), 1);
    assert_eq!(client.get_rubric(&pid, &1).criteria.len(), 2);
}

#[test]
fn test_publish_rubric_creates_immutable_new_version() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let v2 = client.publish_rubric(&admin, &pid, &criteria(&env));
    assert_eq!(v2, 2);

    // Version 1 is unchanged and still retrievable.
    assert_eq!(client.get_rubric(&pid, &1).version, 1);
    assert_eq!(client.get_rubric(&pid, &2).version, 2);
}

#[test]
fn test_publish_rubric_rejects_empty_criteria() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &4u32);

    let result = client.try_publish_rubric(&admin, &pid, &empty_criteria(&env));
    assert_eq!(result, Err(Ok(ContractError::InvalidRubric)));
}

#[test]
fn test_publish_rubric_rejects_weights_not_summing_to_10000() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &4u32);

    let bad = vec![
        &env,
        Criterion {
            weight_bps: 6_000,
            max_score: 100,
        },
        Criterion {
            weight_bps: 3_000,
            max_score: 50,
        },
    ];
    let result = client.try_publish_rubric(&admin, &pid, &bad);
    assert_eq!(result, Err(Ok(ContractError::InvalidRubric)));
}

#[test]
fn test_publish_rubric_rejects_zero_max_score() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &4u32);

    let bad = vec![
        &env,
        Criterion {
            weight_bps: 10_000,
            max_score: 0,
        },
    ];
    let result = client.try_publish_rubric(&admin, &pid, &bad);
    assert_eq!(result, Err(Ok(ContractError::InvalidRubric)));
}

#[test]
fn test_non_admin_cannot_publish_rubric() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let attacker = Address::generate(&env);
    let result = client.try_publish_rubric(&attacker, &pid, &criteria(&env));
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_register_program_rejects_invalid_review_config() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);

    let result = client.try_register_program(&admin, &pid, &0u32, &4u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidReviewConfig)));

    let result = client.try_register_program(&admin, &pid, &3u32, &2u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidReviewConfig)));
}

// ── review submission ───────────────────────────────────────────────────────

#[test]
fn test_submit_review_requires_registered_program() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let result = client.try_submit_review(
        &reviewer,
        &key(&env, 9),
        &applicant,
        &vec![&env, 100u32, 50u32],
    );
    assert_eq!(result, Err(Ok(ContractError::ProgramNotFound)));
}

#[test]
fn test_submit_review_requires_published_rubric() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &4u32);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let result = client.try_submit_review(
        &reviewer,
        &pid,
        &applicant,
        &vec![&env, 100u32, 50u32],
    );
    assert_eq!(result, Err(Ok(ContractError::NoRubricPublished)));
}

#[test]
fn test_submit_review_rejects_wrong_number_of_scores() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let result =
        client.try_submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32]);
    assert_eq!(result, Err(Ok(ContractError::InvalidScores)));
}

#[test]
fn test_submit_review_rejects_score_above_criterion_max() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    // Second criterion has max_score 50; 51 is out of bounds.
    let result =
        client.try_submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32, 51u32]);
    assert_eq!(result, Err(Ok(ContractError::InvalidScores)));
}

#[test]
fn test_submit_review_returns_weighted_total_and_ties_to_rubric_version() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let total = client.submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    // (10000 * 6000 + 10000 * 4000) / 10000 = 10000
    assert_eq!(total, 10_000);

    let review = client.get_review(&pid, &applicant, &reviewer);
    assert_eq!(review.rubric_version, 1);
    assert!(client.has_review(&pid, &applicant, &reviewer));
    assert_eq!(client.get_reviewer_count(&pid, &applicant), 1);
}

#[test]
fn test_duplicate_review_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    client.submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32, 50u32]);

    let result =
        client.try_submit_review(&reviewer, &pid, &applicant, &vec![&env, 10u32, 5u32]);
    assert_eq!(result, Err(Ok(ContractError::ReviewAlreadyExists)));
}

#[test]
fn test_reviewer_capacity_is_enforced() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &1u32); // at most one review
    client.publish_rubric(&admin, &pid, &criteria(&env));

    let applicant = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    client.submit_review(&r1, &pid, &applicant, &vec![&env, 100u32, 50u32]);

    let result = client.try_submit_review(&r2, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    assert_eq!(result, Err(Ok(ContractError::ReviewerCapacityExceeded)));
}

#[test]
fn test_review_rejected_for_inactive_program() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);
    client.set_program_active(&admin, &pid, &false);

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let result =
        client.try_submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    assert_eq!(result, Err(Ok(ContractError::ProgramInactive)));
}

// ── aggregate score ─────────────────────────────────────────────────────────

#[test]
fn test_aggregate_requires_minimum_number_of_reviews() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin); // min_reviews = 2

    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    client.submit_review(&reviewer, &pid, &applicant, &vec![&env, 100u32, 50u32]);

    let result = client.try_aggregate_score(&pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::InsufficientReviews)));
}

#[test]
fn test_aggregate_is_rubric_weighted_average_of_reviews() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = ready(&env, &client, &admin);

    let applicant = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    // 10000 and 5000 weighted totals -> (10000 + 5000) / 2 = 7500
    client.submit_review(&r1, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    client.submit_review(&r2, &pid, &applicant, &vec![&env, 50u32, 25u32]);

    assert_eq!(client.aggregate_score(&pid, &applicant), 7_500);
}

#[test]
fn test_aggregate_floors_at_each_division() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &2u32, &4u32);
    client.publish_rubric(&admin, &pid, &criteria(&env));

    let applicant = Address::generate(&env);
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);
    // Weighted totals 10000, 10000, 5000 -> 25000 / 3 = 8333 (floor), not 8334.
    client.submit_review(&r1, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    client.submit_review(&r2, &pid, &applicant, &vec![&env, 100u32, 50u32]);
    client.submit_review(&r3, &pid, &applicant, &vec![&env, 50u32, 25u32]);

    assert_eq!(client.aggregate_score(&pid, &applicant), 8_333);
}

#[test]
fn test_aggregate_rejects_mixed_rubric_versions() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = key(&env, 1);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1u32, &4u32);
    client.publish_rubric(&admin, &pid, &criteria(&env));

    let applicant = Address::generate(&env);
    let r1 = Address::generate(&env);
    client.submit_review(&r1, &pid, &applicant, &vec![&env, 100u32, 50u32]);

    // A second rubric version is published before the next review is scored.
    client.publish_rubric(&admin, &pid, &criteria(&env));
    let r2 = Address::generate(&env);
    client.submit_review(&r2, &pid, &applicant, &vec![&env, 100u32, 50u32]);

    let result = client.try_aggregate_score(&pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::MixedRubricVersions)));
}

// ── tie policy ──────────────────────────────────────────────────────────────

fn single_review_program(
    env: &Env,
    client: &crate::ScholarshipReviewsContractClient,
    admin: &Address,
) -> BytesN<32> {
    let pid = key(env, 2);
    client.initialize(admin);
    client.register_program(admin, &pid, &1u32, &4u32);
    client.publish_rubric(admin, &pid, &criteria(env));
    pid
}

#[test]
fn test_compare_prefers_higher_aggregate() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = single_review_program(&env, &client, &admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let r = Address::generate(&env);
    client.submit_review(&r, &pid, &a, &vec![&env, 100u32, 50u32]); // 10000
    client.submit_review(&r, &pid, &b, &vec![&env, 50u32, 25u32]); // 5000

    assert_eq!(client.compare_applicants(&pid, &a, &b), CompareOutcome::ABetter);
    assert_eq!(client.compare_applicants(&pid, &b, &a), CompareOutcome::BBetter);
}

#[test]
fn test_compare_ties_break_on_earliest_review() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = single_review_program(&env, &client, &admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(100);
    client.submit_review(&r, &pid, &a, &vec![&env, 100u32, 50u32]);
    env.ledger().set_timestamp(200);
    client.submit_review(&r, &pid, &b, &vec![&env, 100u32, 50u32]);

    // Equal aggregates; A's earliest review predates B's.
    assert_eq!(client.compare_applicants(&pid, &a, &b), CompareOutcome::ABetter);
}

#[test]
fn test_compare_reports_tie_when_indistinguishable() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    let pid = single_review_program(&env, &client, &admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let r = Address::generate(&env);

    env.ledger().set_timestamp(500);
    client.submit_review(&r, &pid, &a, &vec![&env, 100u32, 50u32]);
    client.submit_review(&r, &pid, &b, &vec![&env, 100u32, 50u32]);

    assert_eq!(client.compare_applicants(&pid, &a, &b), CompareOutcome::Tie);
}

#[test]
fn test_version_is_one() {
    let (env, contract_id, _admin) = setup();
    let client = crate::ScholarshipReviewsContractClient::new(&env, &contract_id);
    assert_eq!(client.version(), 1);
}
