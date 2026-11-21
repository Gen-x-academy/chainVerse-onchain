extern crate std;

use soroban_sdk::{
    testutils::Address as _, Address, BytesN, Env, Vec as SdkVec,
};

use crate::{
    CapstoneIndependentReviewsContract, CapstoneIndependentReviewsContractClient, Confidence,
    ConflictState, ContractError, Recommendation, ReviewStatus, Role, Score,
};

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn setup() -> (
    Env,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneIndependentReviewsContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(2_000);
    let contract_id = env.register(CapstoneIndependentReviewsContract, ());
    let client = CapstoneIndependentReviewsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin);
    client.grant_role(&admin, &tenant, &project, &reviewer, &Role::Reviewer);
    (env, admin, reviewer, tenant, project, client)
}

fn scores(env: &Env, points: u32) -> SdkVec<Score> {
    let mut out = SdkVec::new(env);
    out.push_back(Score {
        criterion_id: hash(env, 90),
        points,
    });
    out
}

#[test]
fn initialization_is_one_time_and_unauthorized_cannot_submit() {
    let (env, admin, reviewer, tenant, project, client) = setup();
    let other = Address::generate(&env);
    let outsider = Address::generate(&env);

    assert!(client.has_role(&reviewer, &tenant, &project, &Role::Reviewer));
    assert!(!client.has_role(&outsider, &tenant, &project, &Role::Reviewer));
    assert_eq!(
        client.try_initialize(&admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        client
            .try_grant_role(&other, &tenant, &project, &outsider, &Role::Reviewer)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        client
            .try_submit_review(
                &outsider,
                &tenant,
                &project,
                &hash(&env, 3),
                &hash(&env, 4),
                &hash(&env, 5),
                &hash(&env, 6),
                &1,
                &hash(&env, 7),
                &1,
                &hash(&env, 8),
                &Recommendation::Approve,
                &Confidence::High,
                &ConflictState::NoConflict,
                &scores(&env, 80),
                &hash(&env, 9),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn submit_is_idempotent_and_preserves_immutable_linkage() {
    let (env, _, reviewer, tenant, project, client) = setup();
    let review_id = hash(&env, 10);
    let request = hash(&env, 11);
    let payload = hash(&env, 12);
    let proposal = hash(&env, 13);
    let rubric = hash(&env, 14);
    let reviewer_ref = hash(&env, 15);
    let comments = hash(&env, 16);

    let first = client.submit_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &request,
        &payload,
        &proposal,
        &7,
        &rubric,
        &3,
        &reviewer_ref,
        &Recommendation::Approve,
        &Confidence::High,
        &ConflictState::NoConflict,
        &scores(&env, 90),
        &comments,
    );
    let replay = client.submit_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &request,
        &payload,
        &proposal,
        &7,
        &rubric,
        &3,
        &reviewer_ref,
        &Recommendation::Approve,
        &Confidence::High,
        &ConflictState::NoConflict,
        &scores(&env, 90),
        &comments,
    );
    assert_eq!(first, 1);
    assert_eq!(replay, 1);
    assert_eq!(
        client
            .try_submit_review(
                &reviewer,
                &tenant,
                &project,
                &review_id,
                &request,
                &hash(&env, 99),
                &proposal,
                &7,
                &rubric,
                &3,
                &reviewer_ref,
                &Recommendation::Approve,
                &Confidence::High,
                &ConflictState::NoConflict,
                &scores(&env, 90),
                &comments,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );

    let stored = client.get_review(&tenant, &project, &review_id).unwrap();
    assert_eq!(stored.submission_version, 7);
    assert_eq!(stored.rubric_version, 3);
    assert_eq!(stored.reviewer_ref, reviewer_ref);
    assert_eq!(stored.status, ReviewStatus::Submitted);
    assert_eq!(client.get_scores(&tenant, &project, &review_id).len(), 1);
    assert_eq!(
        client
            .get_submission_review_ids(&tenant, &project, &proposal, 7)
            .len(),
        1
    );
}

#[test]
fn amend_and_withdraw_require_versions_and_original_reviewer() {
    let (env, admin, reviewer, tenant, project, client) = setup();
    let review_id = hash(&env, 20);
    client.submit_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &hash(&env, 21),
        &hash(&env, 22),
        &hash(&env, 23),
        &1,
        &hash(&env, 24),
        &1,
        &hash(&env, 25),
        &Recommendation::Approve,
        &Confidence::Medium,
        &ConflictState::NoConflict,
        &scores(&env, 70),
        &hash(&env, 26),
    );
    let other = Address::generate(&env);
    client.grant_role(&admin, &tenant, &project, &other, &Role::Reviewer);

    assert_eq!(
        client
            .try_amend_review(
                &other,
                &tenant,
                &project,
                &review_id,
                &hash(&env, 27),
                &hash(&env, 28),
                &1,
                &Recommendation::Reject,
                &Confidence::High,
                &ConflictState::NoConflict,
                &scores(&env, 10),
                &hash(&env, 29),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        client
            .try_amend_review(
                &reviewer,
                &tenant,
                &project,
                &review_id,
                &hash(&env, 30),
                &hash(&env, 31),
                &2,
                &Recommendation::Reject,
                &Confidence::High,
                &ConflictState::NoConflict,
                &scores(&env, 10),
                &hash(&env, 32),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );

    let amended = client.amend_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &hash(&env, 33),
        &hash(&env, 34),
        &1,
        &Recommendation::Reject,
        &Confidence::High,
        &ConflictState::Declared,
        &scores(&env, 10),
        &hash(&env, 35),
    );
    assert_eq!(amended, 2);
    assert_eq!(
        client
            .get_review(&tenant, &project, &review_id)
            .unwrap()
            .recommendation,
        Recommendation::Reject
    );

    let withdrawn = client.withdraw_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &hash(&env, 36),
        &hash(&env, 37),
        &2,
    );
    assert_eq!(withdrawn, 3);
    assert_eq!(
        client
            .get_review(&tenant, &project, &review_id)
            .unwrap()
            .status,
        ReviewStatus::Withdrawn
    );
    assert_eq!(
        client
            .try_amend_review(
                &reviewer,
                &tenant,
                &project,
                &review_id,
                &hash(&env, 38),
                &hash(&env, 39),
                &3,
                &Recommendation::Approve,
                &Confidence::Low,
                &ConflictState::NoConflict,
                &scores(&env, 100),
                &hash(&env, 40),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
}

#[test]
fn scores_are_bounded_and_timestamps_are_recorded() {
    let (env, _, reviewer, tenant, project, client) = setup();
    env.ledger().set_timestamp(5_000);
    let review_id = hash(&env, 50);
    client.submit_review(
        &reviewer,
        &tenant,
        &project,
        &review_id,
        &hash(&env, 51),
        &hash(&env, 52),
        &hash(&env, 53),
        &2,
        &hash(&env, 54),
        &1,
        &hash(&env, 55),
        &Recommendation::Abstain,
        &Confidence::Low,
        &ConflictState::NoConflict,
        &scores(&env, 100),
        &hash(&env, 56),
    );
    let stored = client.get_review(&tenant, &project, &review_id).unwrap();
    assert_eq!(stored.submitted_at, 5_000);
    assert_eq!(stored.updated_at, 5_000);

    let mut too_many = SdkVec::new(&env);
    let mut i = 0;
    while i < 17 {
        too_many.push_back(Score {
            criterion_id: hash(&env, 100 + i),
            points: 50,
        });
        i += 1;
    }
    assert_eq!(
        client
            .try_submit_review(
                &reviewer,
                &tenant,
                &project,
                &hash(&env, 60),
                &hash(&env, 61),
                &hash(&env, 62),
                &hash(&env, 63),
                &1,
                &hash(&env, 64),
                &1,
                &hash(&env, 65),
                &Recommendation::Approve,
                &Confidence::High,
                &ConflictState::NoConflict,
                &too_many,
                &hash(&env, 66),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidScore
    );
    let mut invalid_points = SdkVec::new(&env);
    invalid_points.push_back(Score {
        criterion_id: hash(&env, 90),
        points: 101,
    });
    assert_eq!(
        client
            .try_submit_review(
                &reviewer,
                &tenant,
                &project,
                &hash(&env, 67),
                &hash(&env, 68),
                &hash(&env, 69),
                &hash(&env, 70),
                &1,
                &hash(&env, 71),
                &1,
                &hash(&env, 72),
                &Recommendation::Approve,
                &Confidence::High,
                &ConflictState::NoConflict,
                &invalid_points,
                &hash(&env, 73),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidScore
    );
}

#[test]
fn submission_index_is_bounded() {
    let (env, _, reviewer, tenant, project, client) = setup();
    let proposal = hash(&env, 80);
    let mut i = 1u8;
    while i <= 64 {
        client.submit_review(
            &reviewer,
            &tenant,
            &project,
            &hash(&env, i),
            &hash(&env, 100 + i),
            &hash(&env, 150 + i),
            &proposal,
            &1,
            &hash(&env, 7),
            &1,
            &hash(&env, 8),
            &Recommendation::Approve,
            &Confidence::High,
            &ConflictState::NoConflict,
            &scores(&env, 80),
            &hash(&env, 9),
        );
        i += 1;
    }
    assert_eq!(
        client
            .get_submission_review_ids(&tenant, &project, &proposal, 1)
            .len(),
        64
    );
    assert_eq!(
        client
            .try_submit_review(
                &reviewer,
                &tenant,
                &project,
                &hash(&env, 240),
                &hash(&env, 241),
                &hash(&env, 242),
                &proposal,
                &1,
                &hash(&env, 7),
                &1,
                &hash(&env, 8),
                &Recommendation::Approve,
                &Confidence::High,
                &ConflictState::NoConflict,
                &scores(&env, 80),
                &hash(&env, 9),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CollectionFull
    );
}
