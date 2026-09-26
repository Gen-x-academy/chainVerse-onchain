#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Vec};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> Address {
    let admin = Address::generate(env);
    ScholarshipReviewWorkflowContract::initialize(env.clone(), admin.clone()).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);

    assert!(ScholarshipReviewWorkflowContract::initialize(env.clone(), admin.clone()).is_ok());
    assert_eq!(
        ScholarshipReviewWorkflowContract::initialize(env, admin),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_create_rubric() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Academic Merit"),
            description: String::from_str(&env, "GPA and academic achievements"),
            weight: 5000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Rate 1-10"),
            required_comment: true,
            disqualifying: false,
            disqualify_threshold: None,
        },
        RubricCriterion {
            id: String::from_str(&env, "c2"),
            name: String::from_str(&env, "Personal Statement"),
            description: String::from_str(&env, "Quality of personal essay"),
            weight: 3000,
            scale: CriterionScale::Numeric { min: 1, max: 5 },
            guidance: String::from_str(&env, "Rate 1-5"),
            required_comment: true,
            disqualifying: false,
            disqualify_threshold: None,
        },
        RubricCriterion {
            id: String::from_str(&env, "c3"),
            name: String::from_str(&env, "Recommendations"),
            description: String::from_str(&env, "Strength of recommendation letters"),
            weight: 2000,
            scale: CriterionScale::Categorical { options: 4 },
            guidance: String::from_str(&env, "Weak/Average/Strong/Exceptional"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Standard Rubric"),
        criteria,
    ).unwrap();

    assert_eq!(rubric.program_id, program_id);
    assert_eq!(rubric.version, 1);
    assert_eq!(rubric.criteria.len(), 3);
    assert!(!rubric.is_active);
}

#[test]
fn test_rubric_weights_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    // Weights don't sum to 10000
    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 5000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
        RubricCriterion {
            id: String::from_str(&env, "c2"),
            name: String::from_str(&env, "Criterion 2"),
            description: String::from_str(&env, "Desc"),
            weight: 3000, // Total = 8000, not 10000
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let result = ScholarshipReviewWorkflowContract::create_rubric(
        env,
        admin,
        program_id,
        String::from_str(&env, "Bad Rubric"),
        criteria,
    );
    assert_eq!(result, Err(ContractError::WeightsDontReconcile));
}

#[test]
fn test_activate_rubric() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    ScholarshipReviewWorkflowContract::activate_rubric(env.clone(), admin.clone(), rubric.id.clone()).unwrap();

    let activated = ScholarshipReviewWorkflowWorkflowContract::get_rubric(env, rubric.id).unwrap();
    assert!(activated.is_active);
    assert!(activated.approved_at.is_some());
}

#[test]
fn test_update_rubric_creates_new_version() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    // Update rubric
    let updated = ScholarshipReviewWorkflowContract::update_rubric(
        env.clone(),
        admin.clone(),
        rubric.id.clone(),
        Some(String::from_str(&env, "Updated Rubric")),
        None,
    ).unwrap();

    assert_eq!(updated.version, 2);
    assert_eq!(updated.name, String::from_str(&env, "Updated Rubric"));

    // Original version still accessible
    let v1 = ScholarshipReviewWorkflowContract::get_rubric_version(env, rubric.id, 1).unwrap();
    assert_eq!(v1.version, 1);
    assert_eq!(v1.name, String::from_str(&env, "Test Rubric"));
}

#[test]
fn test_save_draft() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let reviewer = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    ScholarshipReviewWorkflowContract::activate_rubric(env.clone(), admin.clone(), rubric.id.clone()).unwrap();

    let scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 8,
            comment: String::from_str(&env, "Good work"),
        },
    ]);

    let draft = ScholarshipReviewWorkflowContract::save_draft(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        rubric.id.clone(),
        scores,
        false,
    ).unwrap();

    assert_eq!(draft.reviewer, reviewer);
    assert_eq!(draft.application_id, application_id);
    assert_eq!(draft.rubric_version, 1);
    assert!(!draft.is_complete);

    // Get draft
    let retrieved = ScholarshipReviewWorkflowContract::get_draft(env, reviewer, application_id).unwrap();
    assert_eq!(retrieved.scores.len(), 1);
    assert_eq!(retrieved.scores[0].score, 8);
}

#[test]
fn test_submit_review() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let reviewer = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    ScholarshipReviewWorkflowContract::activate_rubric(env.clone(), admin.clone(), rubric.id.clone()).unwrap();

    let scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 9,
            comment: String::from_str(&env, "Excellent"),
        },
    ]);

    ScholarshipReviewWorkflowContract::save_draft(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        rubric.id.clone(),
        scores,
        true,
    ).unwrap();

    let review = ScholarshipReviewWorkflowContract::submit_review(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
    ).unwrap();

    assert_eq!(review.reviewer, reviewer);
    assert_eq!(review.application_id, application_id);
    assert_eq!(review.status, ReviewStatus::Submitted);
    assert_eq!(review.total_score, 9000); // 9 * 10000 / 100 = 9000
}

#[test]
fn test_amend_review() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let reviewer = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    ScholarshipReviewWorkflowContract::activate_rubric(env.clone(), admin.clone(), rubric.id.clone()).unwrap();

    let scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 7,
            comment: String::from_str(&env, "Good"),
        },
    ]);

    ScholarshipReviewWorkflowContract::save_draft(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        rubric.id.clone(),
        scores,
        true,
    ).unwrap();

    let review = ScholarshipReviewWorkflowContract::submit_review(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
    ).unwrap();

    // Amend review
    let new_scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 8,
            comment: String::from_str(&env, "Re-evaluated: stronger than initially thought"),
        },
    ]);

    ScholarshipReviewWorkflowContract::amend_review(
        env.clone(),
        reviewer.clone(),
        review.id.clone(),
        new_scores,
        String::from_str(&env, "Re-evaluated after second reading"),
    ).unwrap();

    let amended = ScholarshipReviewWorkflowContract::get_review(env, review.id).unwrap();
    assert_eq!(amended.status, ReviewStatus::Amended);
    assert_eq!(amended.scores[0].score, 8);
    assert_eq!(amended.amendment_count, 1);
}

#[test]
fn test_cannot_amend_finalized() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let reviewer = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let criteria = Vec::from_array(&env, [
        RubricCriterion {
            id: String::from_str(&env, "c1"),
            name: String::from_str(&env, "Criterion 1"),
            description: String::from_str(&env, "Desc"),
            weight: 10000,
            scale: CriterionScale::Numeric { min: 1, max: 10 },
            guidance: String::from_str(&env, "Guide"),
            required_comment: false,
            disqualifying: false,
            disqualify_threshold: None,
        },
    ]);

    let rubric = ScholarshipReviewWorkflowContract::create_rubric(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "Test Rubric"),
        criteria,
    ).unwrap();

    ScholarshipReviewWorkflowContract::activate_rubric(env.clone(), admin.clone(), rubric.id.clone()).unwrap();

    let scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 7,
            comment: String::from_str(&env, "Good"),
        },
    ]);

    ScholarshipReviewWorkflowContract::save_draft(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        rubric.id.clone(),
        scores,
        true,
    ).unwrap();

    let review = ScholarshipReviewWorkflowContract::submit_review(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
    ).unwrap();

    // Finalize
    ScholarshipReviewWorkflowContract::finalize_review(env.clone(), admin.clone(), review.id.clone()).unwrap();

    // Try to amend
    let new_scores = Vec::from_array(&env, [
        ReviewScore {
            criterion_id: String::from_str(&env, "c1"),
            score: 8,
            comment: String::from_str(&env, "Attempt to amend finalized"),
        },
    ]);

    let result = ScholarshipReviewWorkflowContract::amend_review(
        env,
        reviewer,
        review.id,
        new_scores,
        String::from_str(&env, "Should fail"),
    );
    assert_eq!(result, Err(ContractError::CannotAmendFinalized));
}

#[test]
fn test_request_info() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let reviewer = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let questions = Vec::from_array(&env, [
        String::from_str(&env, "Please clarify your research methodology."),
        String::from_str(&env, "Provide additional references for section 3."),
    ]);

    let deadline = env.ledger().timestamp() + 86400 * 7; // 1 week

    let request = ScholarshipReviewWorkflowContract::request_info(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        questions,
        Vec::from_array(&env, [
            String::from_str(&env, "research_methodology"),
            String::from_str(&env, "references"),
        ]),
        deadline,
    ).unwrap();

    assert_eq!(request.reviewer, reviewer);
    assert_eq!(request.application_id, application_id);
    assert_eq!(request.questions.len(), 2);
    assert_eq!(request.status, InfoRequestStatus::Pending);
}

#[test]
fn test_respond_to_info_request() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env)
    let reviewer = Address::generate(&env);
    let applicant = Address::generate(&env);
    let application_id = BytesN::from_array(&env, &[3u8; 32]);

    let deadline = env.ledger().timestamp() + 86400 * 7;

    let request = ScholarshipReviewWorkflowContract::request_info(
        env.clone(),
        reviewer.clone(),
        application_id.clone(),
        Vec::from_array(&env, [
            String::from_str(&env, "Question 1"),
        ]),
        Vec::new(&env),
        deadline,
    ).unwrap();

    let response = ScholarshipReviewWorkflowContract::respond_to_info_request(
        env.clone(),
        applicant.clone(),
        request.id.clone(),
        Vec::from_array(&env, [
            String::from_str(&env, "My response to question 1"),
        ]),
    ).unwrap();

    assert_eq!(response.applicant, applicant);
    assert_eq!(response.request_id, request.id);
    assert_eq!(response.responses.len(), 1);
    assert_eq!(response.version, 1);

    let updated_request = ScholarshipReviewWorkflowContract::get_info_request(env, request.id).unwrap();
    assert_eq!(updated_request.status, InfoRequestStatus::Responded);
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipReviewWorkflowContract::version(env), 1);
}