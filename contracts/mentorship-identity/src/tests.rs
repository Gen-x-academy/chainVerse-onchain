#![cfg(test)]

use crate::{
    ContractError, MentorshipIdentityContract, SafetyStatus, VerificationDecision,
    VerificationStatus,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _}, Address, BytesN, Env,
};

struct Fixture {
    env: Env,
    client: crate::MentorshipIdentityContractClient<'static>,
    mentor: Address,
    admin: Address,
    recorder: Address,
    verifier: Address,
    moderator: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipIdentityContract, ());
    let client = crate::MentorshipIdentityContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recorder = Address::generate(&env);
    let verifier = Address::generate(&env);
    let moderator = Address::generate(&env);
    let mentor = Address::generate(&env);
    client.initialize(&admin, &recorder, &verifier, &moderator);
    Fixture {
        env,
        client,
        mentor,
        admin,
        recorder,
        verifier,
        moderator,
    }
}

fn commitment(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

fn create_profile(fixture: &Fixture) {
    fixture.client.update_profile(
        &fixture.mentor,
        &commitment(&fixture.env, 1),
        &3,
    );
}

#[test]
fn completion_is_computed_from_bounded_section_count() {
    let fixture = setup();
    create_profile(&fixture);

    let indicators = fixture.client.get_indicators(&fixture.mentor);
    assert_eq!(indicators.profile.completed_sections, 3);
    assert_eq!(indicators.required_profile_sections, 6);
    assert_eq!(indicators.profile.completion_basis_points, 5_000);
    assert_eq!(indicators.safety_status, SafetyStatus::Unassessed);
    assert!(!indicators.is_verified);

    let invalid = fixture.client.try_update_profile(
        &fixture.mentor,
        &commitment(&fixture.env, 1),
        &7,
    );
    assert_eq!(invalid, Err(Ok(ContractError::InvalidCompletion)));
}

#[test]
fn mentee_learning_profile_stores_field_commitments_and_preserves_creation_time() {
    let fixture = setup();
    fixture.env.ledger().set_timestamp(77);
    fixture.client.set_mentee_learning_profile(
        &fixture.mentor,
        &commitment(&fixture.env, 11),
        &commitment(&fixture.env, 12),
        &commitment(&fixture.env, 13),
        &commitment(&fixture.env, 14),
        &commitment(&fixture.env, 15),
        &commitment(&fixture.env, 16),
        &commitment(&fixture.env, 17),
    );

    let created = fixture.client.get_mentee_learning_profile(&fixture.mentor);
    assert_eq!(created.goals_commitment, commitment(&fixture.env, 11));
    assert_eq!(created.current_level_commitment, commitment(&fixture.env, 12));
    assert_eq!(created.interests_commitment, commitment(&fixture.env, 13));
    assert_eq!(created.preferred_language_commitment, commitment(&fixture.env, 14));
    assert_eq!(created.time_zone_commitment, commitment(&fixture.env, 15));
    assert_eq!(created.accessibility_needs_commitment, commitment(&fixture.env, 16));
    assert_eq!(created.matching_preferences_commitment, commitment(&fixture.env, 17));
    assert_eq!(created.created_at, 77);
    assert_eq!(created.updated_at, 77);

    fixture.env.ledger().set_timestamp(88);
    fixture.client.set_mentee_learning_profile(
        &fixture.mentor,
        &commitment(&fixture.env, 21),
        &commitment(&fixture.env, 12),
        &commitment(&fixture.env, 13),
        &commitment(&fixture.env, 14),
        &commitment(&fixture.env, 15),
        &commitment(&fixture.env, 16),
        &commitment(&fixture.env, 17),
    );
    let updated = fixture.client.get_mentee_learning_profile(&fixture.mentor);
    assert_eq!(updated.goals_commitment, commitment(&fixture.env, 21));
    assert_eq!(updated.created_at, 77);
    assert_eq!(updated.updated_at, 88);
}

#[test]
fn mentee_learning_profile_rejects_empty_commitments() {
    let fixture = setup();
    let invalid = fixture.client.try_set_mentee_learning_profile(
        &fixture.mentor,
        &commitment(&fixture.env, 11),
        &commitment(&fixture.env, 12),
        &commitment(&fixture.env, 13),
        &commitment(&fixture.env, 14),
        &commitment(&fixture.env, 15),
        &commitment(&fixture.env, 16),
        &commitment(&fixture.env, 0),
    );
    assert_eq!(invalid, Err(Ok(ContractError::InvalidCommitment)));
}

#[test]
fn performance_attestation_exposes_rates_and_commitment_not_activity_records() {
    let fixture = setup();
    create_profile(&fixture);
    fixture.env.ledger().set_timestamp(500);

    fixture.client.attest_performance(
        &fixture.recorder,
        &fixture.mentor,
        &9_000,
        &8_000,
        &commitment(&fixture.env, 2),
        &100,
        &400,
    );

    let indicators = fixture.client.get_indicators(&fixture.mentor);
    let performance = indicators.performance.unwrap();
    assert_eq!(performance.response_rate_basis_points, 9_000);
    assert_eq!(performance.delivery_rate_basis_points, 8_000);
    assert_eq!(performance.evidence_commitment, commitment(&fixture.env, 2));

    let unauthorized = fixture.client.try_attest_performance(
        &fixture.verifier,
        &fixture.mentor,
        &9_000,
        &8_000,
        &commitment(&fixture.env, 2),
        &100,
        &400,
    );
    assert_eq!(
        unauthorized,
        Err(Ok(ContractError::NotAuthorizedRecorder))
    );
}

#[test]
fn verification_requires_consent_and_supports_rejection_expiry_and_reverification() {
    let fixture = setup();
    create_profile(&fixture);
    fixture.env.ledger().set_timestamp(100);
    let no_consent = fixture.client.try_submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 6),
        &commitment(&fixture.env, 3),
        &false,
        &1,
    );
    assert_eq!(no_consent, Err(Ok(ContractError::ConsentRequired)));

    fixture.client.submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 6),
        &commitment(&fixture.env, 3),
        &true,
        &1,
    );
    let pending = fixture.client.get_indicators(&fixture.mentor);
    assert_eq!(pending.verification.unwrap().status, VerificationStatus::Pending);

    let duplicate = fixture.client.try_submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 6),
        &commitment(&fixture.env, 3),
        &true,
        &1,
    );
    assert_eq!(duplicate, Err(Ok(ContractError::VerificationPending)));

    fixture.client.review_verification(
        &fixture.verifier,
        &fixture.mentor,
        &VerificationDecision::Reject,
        &0,
        &commitment(&fixture.env, 7),
    );
    let rejected = fixture.client.get_indicators(&fixture.mentor);
    let rejected_attestation = rejected.verification.unwrap();
    assert_eq!(rejected_attestation.status, VerificationStatus::Rejected);
    assert_eq!(rejected_attestation.consent_given_at, 100);
    assert_eq!(rejected_attestation.rejection_reason_commitment, commitment(&fixture.env, 7));

    fixture.client.submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 8),
        &commitment(&fixture.env, 9),
        &true,
        &2,
    );
    let resubmitted = fixture.client.get_indicators(&fixture.mentor);
    let resubmitted_attestation = resubmitted.verification.unwrap();
    assert_eq!(resubmitted_attestation.attempt, 2);
    assert_eq!(resubmitted_attestation.status, VerificationStatus::Pending);
    assert_eq!(resubmitted_attestation.provider_reference, commitment(&fixture.env, 8));

    fixture.client.review_verification(
        &fixture.verifier,
        &fixture.mentor,
        &VerificationDecision::Approve,
        &200,
        &commitment(&fixture.env, 0),
    );
    assert!(fixture.client.get_indicators(&fixture.mentor).is_verified);
    fixture.env.ledger().set_timestamp(201);
    let expired = fixture.client.get_indicators(&fixture.mentor);
    assert!(!expired.is_verified);
    assert_eq!(expired.verification.unwrap().status, VerificationStatus::Expired);

    fixture.client.submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 10),
        &commitment(&fixture.env, 3),
        &true,
        &2,
    );
    let reverified = fixture.client.get_indicators(&fixture.mentor);
    assert_eq!(reverified.verification.unwrap().attempt, 3);
}

#[test]
fn mentor_can_withdraw_consent_and_cancel_pending_review() {
    let fixture = setup();
    create_profile(&fixture);
    fixture.client.submit_verification(
        &fixture.mentor,
        &commitment(&fixture.env, 6),
        &commitment(&fixture.env, 3),
        &true,
        &1,
    );
    fixture.client.cancel_verification(&fixture.mentor);

    let cancelled = fixture.client.get_indicators(&fixture.mentor);
    let attestation = cancelled.verification.unwrap();
    assert_eq!(attestation.status, VerificationStatus::Cancelled);
    assert!(!attestation.consent_given);
    assert_eq!(attestation.consent_revoked_at, Some(fixture.env.ledger().timestamp()));

    let review = fixture.client.try_review_verification(
        &fixture.verifier,
        &fixture.mentor,
        &VerificationDecision::Approve,
        &300,
        &commitment(&fixture.env, 0),
    );
    assert_eq!(review, Err(Ok(ContractError::InvalidReview)));
}

#[test]
fn safety_is_a_coarse_moderator_attestation() {
    let fixture = setup();
    create_profile(&fixture);
    fixture.client.attest_safety(
        &fixture.moderator,
        &fixture.mentor,
        &SafetyStatus::Clear,
        &commitment(&fixture.env, 4),
    );
    let indicators = fixture.client.get_indicators(&fixture.mentor);
    assert_eq!(indicators.safety_status, SafetyStatus::Clear);
    assert_eq!(
        indicators.safety.unwrap().evidence_commitment,
        commitment(&fixture.env, 4)
    );

    let unauthorized = fixture.client.try_attest_safety(
        &fixture.verifier,
        &fixture.mentor,
        &SafetyStatus::Restricted,
        &commitment(&fixture.env, 5),
    );
    assert_eq!(
        unauthorized,
        Err(Ok(ContractError::NotAuthorizedModerator))
    );
}