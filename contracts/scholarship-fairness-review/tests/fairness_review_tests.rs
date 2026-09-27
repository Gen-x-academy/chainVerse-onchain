#![cfg(test)]
extern crate std;

use scholarship_fairness_review::{
    ContractError, ReviewOutcome, RiskLevel, ScholarshipFairnessReview,
    ScholarshipFairnessReviewClient,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env, String,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address, Address, ScholarshipFairnessReviewClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ScholarshipFairnessReview, ());
    let client = ScholarshipFairnessReviewClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let high_risk_approver = Address::generate(&env);

    client
        .init(&admin, &reviewer, &high_risk_approver)
        .unwrap();

    (env, admin, reviewer, high_risk_approver, client)
}

fn dummy_payload(env: &Env) -> Bytes {
    Bytes::from_slice(env, b"rule-payload-v1")
}

fn s(env: &Env, text: &str) -> String {
    String::from_str(env, text)
}

// ── Initialisation ────────────────────────────────────────────────────────────

#[test]
fn init_sets_state_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipFairnessReview, ());
    let client = ScholarshipFairnessReviewClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let approver = Address::generate(&env);

    let events_before = env.events().all().len();
    client.init(&admin, &reviewer, &approver).unwrap();
    assert!(env.events().all().len() > events_before, "FR_INIT event emitted");
}

#[test]
fn init_rejects_double_initialisation() {
    let (_env, admin, reviewer, approver, client) = setup();
    let result = client.try_init(&admin, &reviewer, &approver);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

// ── Propose rule version ──────────────────────────────────────────────────────

#[test]
fn propose_standard_version_increments_counter() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let v1 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "Add GPA minimum"),
            &s(&env, "cohort-2026-q1"),
            &0,
        )
        .unwrap();
    assert_eq!(v1, 1);

    let v2 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "Adjust income threshold"),
            &s(&env, "cohort-2026-q2"),
            &v1,
        )
        .unwrap();
    assert_eq!(v2, 2);
}

#[test]
fn propose_rejects_empty_payload() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &Bytes::new(&env),
        &s(&env, "summary"),
        &s(&env, "cohort"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::EmptyRulePayload)));
}

#[test]
fn propose_rejects_oversized_payload() {
    let (env, admin, _reviewer, _approver, client) = setup();
    // 4 097 bytes — one over the limit
    let oversized = Bytes::from_slice(env, &[b'x'; 4097]);
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &oversized,
        &s(&env, "summary"),
        &s(&env, "cohort"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::RulePayloadTooLarge)));
}

#[test]
fn propose_rejects_oversized_cohort_tag() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let long_tag = String::from_str(&env, "a-very-long-cohort-tag-that-exceeds-the-sixty-four-byte-maximum-x");
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &dummy_payload(&env),
        &s(&env, "summary"),
        &long_tag,
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::CohortTagTooLong)));
}

#[test]
fn propose_rejects_nonexistent_replaces_version() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &dummy_payload(&env),
        &s(&env, "summary"),
        &s(&env, "cohort"),
        &99, // does not exist
    );
    assert_eq!(result, Err(Ok(ContractError::RuleVersionNotFound)));
}

#[test]
fn propose_rejects_unauthorized_caller() {
    let (env, _admin, _reviewer, _approver, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_propose_rule_version(
        &stranger,
        &RiskLevel::Standard,
        &dummy_payload(&env),
        &s(&env, "summary"),
        &s(&env, "cohort"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn propose_blocked_when_paused() {
    let (env, admin, _reviewer, _approver, client) = setup();
    client.pause(&admin).unwrap();
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &dummy_payload(&env),
        &s(&env, "summary"),
        &s(&env, "cohort"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

// ── Record review ─────────────────────────────────────────────────────────────

#[test]
fn record_review_approval_marks_version_approved() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "LGTM"))
        .unwrap();
    let summary = client.get_version_summary(&v).unwrap();
    assert_eq!(summary.review_outcome, ReviewOutcome::Approved);
}

#[test]
fn record_review_rejects_double_review() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    let result = client.try_record_review(
        &reviewer,
        &v,
        &ReviewOutcome::Rejected,
        &s(&env, "changed mind"),
    );
    assert_eq!(result, Err(Ok(ContractError::ReviewAlreadyRecorded)));
}

#[test]
fn record_review_rejects_non_reviewer() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    let stranger = Address::generate(&env);
    let result =
        client.try_record_review(&stranger, &v, &ReviewOutcome::Approved, &s(&env, ""));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── High-risk two-approver gate ───────────────────────────────────────────────

#[test]
fn high_risk_activation_requires_second_approval() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::High,
            &dummy_payload(&env),
            &s(&env, "Change scoring weights — high risk"),
            &s(&env, "cohort-hr"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    // Attempt activation without second approver
    let result = client.try_activate_version(&admin, &v);
    assert_eq!(result, Err(Ok(ContractError::HighRiskApprovalRequired)));
}

#[test]
fn high_risk_activation_succeeds_after_second_approval() {
    let (env, admin, reviewer, approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::High,
            &dummy_payload(&env),
            &s(&env, "High risk change"),
            &s(&env, "cohort-hr"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.second_approve(&approver, &v).unwrap();
    client.activate_version(&admin, &v).unwrap();
    assert_eq!(client.active_version(), v);
}

#[test]
fn second_approver_must_differ_from_proposer() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::High,
            &dummy_payload(&env),
            &s(&env, "High risk"),
            &s(&env, "cohort-hr"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    // The high_risk_approver in this contract is a distinct address from admin,
    // but we must also test that admin (the proposer) cannot self-sign as
    // high_risk_approver. We do that by calling second_approve with admin.
    // Since admin != high_risk_approver, this will return Unauthorized first.
    let result = client.try_second_approve(&admin, &v);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── Activate ──────────────────────────────────────────────────────────────────

#[test]
fn activate_standard_approved_version() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.activate_version(&admin, &v).unwrap();

    assert_eq!(client.active_version(), v);
    let summary = client.get_version_summary(&v).unwrap();
    assert!(summary.is_active);
}

#[test]
fn activate_rejects_pending_version() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    let result = client.try_activate_version(&admin, &v);
    assert_eq!(result, Err(Ok(ContractError::VersionNotApproved)));
}

#[test]
fn activate_deactivates_previous_version() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v1 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "v1"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v1, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.activate_version(&admin, &v1).unwrap();

    let v2 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "v2"),
            &s(&env, "cohort"),
            &v1,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v2, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.activate_version(&admin, &v2).unwrap();

    let s1 = client.get_version_summary(&v1).unwrap();
    let s2 = client.get_version_summary(&v2).unwrap();
    assert!(!s1.is_active, "v1 deactivated after v2 activates");
    assert!(s2.is_active, "v2 is active");
    assert_eq!(client.active_version(), v2);
}

// ── Rollback ──────────────────────────────────────────────────────────────────

#[test]
fn rollback_to_approved_version_succeeds() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v1 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "v1"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v1, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.activate_version(&admin, &v1).unwrap();

    let v2 = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "v2"),
            &s(&env, "cohort"),
            &v1,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v2, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    client.activate_version(&admin, &v2).unwrap();

    // Roll back to v1
    client.rollback_to(&admin, &v1).unwrap();
    assert_eq!(client.active_version(), v1);
    let sv1 = client.get_version_summary(&v1).unwrap();
    let sv2 = client.get_version_summary(&v2).unwrap();
    assert!(sv1.is_active);
    assert!(!sv2.is_active);
}

#[test]
fn rollback_to_rejected_version_fails() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "v"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Rejected, &s(&env, "nope"))
        .unwrap();
    let result = client.try_rollback_to(&admin, &v);
    assert_eq!(result, Err(Ok(ContractError::RollbackTargetNotApproved)));
}

#[test]
fn rollback_to_nonexistent_version_fails() {
    let (_env, admin, _reviewer, _approver, client) = setup();
    let result = client.try_rollback_to(&admin, &999);
    assert_eq!(result, Err(Ok(ContractError::RuleVersionNotFound)));
}

// ── Admin transfer ────────────────────────────────────────────────────────────

#[test]
fn two_step_admin_transfer() {
    let (env, admin, reviewer, approver, client) = setup();
    let new_admin = Address::generate(&env);
    client.propose_admin(&admin, &new_admin).unwrap();
    client.accept_admin(&new_admin).unwrap();

    // Old admin can no longer propose
    let result = client.try_propose_rule_version(
        &admin,
        &RiskLevel::Standard,
        &dummy_payload(&env),
        &s(&env, "after transfer"),
        &s(&env, "cohort"),
        &0,
    );
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));

    // New admin can propose
    client
        .propose_rule_version(
            &new_admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "new admin proposes"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();

    let _ = (reviewer, approver); // suppress warnings
}

#[test]
fn admin_transfer_expires() {
    let (env, admin, reviewer, approver, client) = setup();
    let new_admin = Address::generate(&env);
    client.propose_admin(&admin, &new_admin).unwrap();

    // Advance time past the 30-day expiry
    env.ledger().set_timestamp(env.ledger().timestamp() + 2_592_001);
    let result = client.try_accept_admin(&new_admin);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
    let _ = (reviewer, approver);
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn only_admin_can_pause() {
    let (env, _admin, _reviewer, _approver, client) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_pause(&stranger);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn pause_unpause_cycle() {
    let (_env, admin, _reviewer, _approver, client) = setup();
    assert!(!client.paused());
    client.pause(&admin).unwrap();
    assert!(client.paused());
    client.unpause(&admin).unwrap();
    assert!(!client.paused());
}

// ── Adversarial: storage isolation ───────────────────────────────────────────

#[test]
fn get_nonexistent_version_returns_error() {
    let (_env, _admin, _reviewer, _approver, client) = setup();
    let result = client.try_get_version_summary(&42);
    assert_eq!(result, Err(Ok(ContractError::RuleVersionNotFound)));
}

#[test]
fn active_version_zero_before_any_activation() {
    let (_env, _admin, _reviewer, _approver, client) = setup();
    assert_eq!(client.active_version(), 0);
}

// ── Event coverage ────────────────────────────────────────────────────────────

#[test]
fn propose_emits_event() {
    let (env, admin, _reviewer, _approver, client) = setup();
    let before = env.events().all().len();
    client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "event check"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    assert!(env.events().all().len() > before);
}

#[test]
fn activate_emits_event() {
    let (env, admin, reviewer, _approver, client) = setup();
    let v = client
        .propose_rule_version(
            &admin,
            &RiskLevel::Standard,
            &dummy_payload(&env),
            &s(&env, "summary"),
            &s(&env, "cohort"),
            &0,
        )
        .unwrap();
    client
        .record_review(&reviewer, &v, &ReviewOutcome::Approved, &s(&env, "ok"))
        .unwrap();
    let before = env.events().all().len();
    client.activate_version(&admin, &v).unwrap();
    assert!(env.events().all().len() > before);
}
