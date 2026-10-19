#![cfg(test)]
use crate::{
    ContractError, MentorshipCriterion, ScholarshipEligibilityContract,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    vec, Address, BytesN, Env, Symbol,
};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipEligibilityContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn program_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// ── #1066 — composable eligibility rules ────────────────────────────────────

#[test]
fn test_publish_rule_starts_at_version_one() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let required = vec![
        &env,
        Symbol::new(&env, "enrolled"),
        Symbol::new(&env, "income_band"),
    ];
    let version = client.publish_eligibility_rule(&admin, &pid, &required);
    assert_eq!(version, 1);
    assert_eq!(client.get_latest_rule_version(&pid), 1);
}

#[test]
fn test_publish_rule_rejects_empty_requirements() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let empty = vec![&env];
    let result = client.try_publish_eligibility_rule(&admin, &pid, &empty);
    assert_eq!(result, Err(Ok(ContractError::EmptyRule)));
}

#[test]
fn test_publish_rule_rejects_too_many_requirements() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let mut required = soroban_sdk::Vec::new(&env);
    for i in 0..11 {
        required.push_back(Symbol::new(&env, "req"));
        let _ = i;
    }
    let result = client.try_publish_eligibility_rule(&admin, &pid, &required);
    assert_eq!(result, Err(Ok(ContractError::RuleTooLarge)));
}

#[test]
fn test_republishing_rule_creates_new_immutable_version() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let v1_types = vec![&env, Symbol::new(&env, "enrolled")];
    client.publish_eligibility_rule(&admin, &pid, &v1_types);

    let v2_types = vec![
        &env,
        Symbol::new(&env, "enrolled"),
        Symbol::new(&env, "income_band"),
    ];
    let v2 = client.publish_eligibility_rule(&admin, &pid, &v2_types);
    assert_eq!(v2, 2);

    // v1 is unchanged.
    let rule_v1 = client.get_eligibility_rule(&pid, &1);
    assert_eq!(rule_v1.required_types.len(), 1);
    let rule_v2 = client.get_eligibility_rule(&pid, &2);
    assert_eq!(rule_v2.required_types.len(), 2);
}

#[test]
fn test_non_admin_cannot_publish_rule() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    let attacker = Address::generate(&env);

    let required = vec![&env, Symbol::new(&env, "enrolled")];
    let result = client.try_publish_eligibility_rule(&attacker, &pid, &required);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// ── #1068 — attestations ─────────────────────────────────────────────────────

#[test]
fn test_unauthorized_issuer_cannot_issue_attestation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let result = client.try_issue_attestation(
        &issuer,
        &subject,
        &pid,
        &Symbol::new(&env, "enrolled"),
        &1_000u64,
    );
    assert_eq!(result, Err(Ok(ContractError::NotAuthorizedIssuer)));
}

#[test]
fn test_issue_attestation_rejects_past_expiry() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);
    env.ledger().set_timestamp(1_000);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let result = client.try_issue_attestation(
        &issuer,
        &subject,
        &pid,
        &Symbol::new(&env, "enrolled"),
        &500u64,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidExpiry)));
}

#[test]
fn test_issue_and_check_valid_attestation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let attestation_type = Symbol::new(&env, "enrolled");
    client.issue_attestation(&issuer, &subject, &pid, &attestation_type, &10_000u64);

    assert!(client.has_valid_attestation(&subject, &pid, &attestation_type));
}

#[test]
fn test_attestation_expires() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let attestation_type = Symbol::new(&env, "enrolled");
    client.issue_attestation(&issuer, &subject, &pid, &attestation_type, &1_000u64);

    assert!(client.has_valid_attestation(&subject, &pid, &attestation_type));
    env.ledger().set_timestamp(1_001);
    assert!(!client.has_valid_attestation(&subject, &pid, &attestation_type));
}

#[test]
fn test_revoke_attestation_invalidates_immediately() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let attestation_type = Symbol::new(&env, "enrolled");
    client.issue_attestation(&issuer, &subject, &pid, &attestation_type, &10_000u64);
    assert!(client.has_valid_attestation(&subject, &pid, &attestation_type));

    client.revoke_attestation(&issuer, &subject, &pid, &attestation_type);
    assert!(!client.has_valid_attestation(&subject, &pid, &attestation_type));
}

#[test]
fn test_only_issuer_or_admin_can_revoke() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);
    let stranger = Address::generate(&env);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let attestation_type = Symbol::new(&env, "enrolled");
    client.issue_attestation(&issuer, &subject, &pid, &attestation_type, &10_000u64);

    let result = client.try_revoke_attestation(&stranger, &subject, &pid, &attestation_type);
    assert_eq!(result, Err(Ok(ContractError::NotAttestationIssuer)));
}

#[test]
fn test_admin_can_revoke_any_attestation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let subject = Address::generate(&env);
    let pid = program_id(&env, 1);
    let attestation_type = Symbol::new(&env, "enrolled");
    client.issue_attestation(&issuer, &subject, &pid, &attestation_type, &10_000u64);

    client.revoke_attestation(&admin, &subject, &pid, &attestation_type);
    assert!(!client.has_valid_attestation(&subject, &pid, &attestation_type));
}

// ── deterministic evaluation ─────────────────────────────────────────────────

#[test]
fn test_evaluate_eligibility_true_when_all_requirements_met() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 1);
    let enrolled = Symbol::new(&env, "enrolled");
    let income = Symbol::new(&env, "income_band");
    client.publish_eligibility_rule(&admin, &pid, &vec![&env, enrolled.clone(), income.clone()]);

    let subject = Address::generate(&env);
    client.issue_attestation(&issuer, &subject, &pid, &enrolled, &10_000u64);
    client.issue_attestation(&issuer, &subject, &pid, &income, &10_000u64);

    assert!(client.evaluate_eligibility(&pid, &subject));
}

#[test]
fn test_evaluate_eligibility_false_when_one_requirement_missing() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 1);
    let enrolled = Symbol::new(&env, "enrolled");
    let income = Symbol::new(&env, "income_band");
    client.publish_eligibility_rule(&admin, &pid, &vec![&env, enrolled.clone(), income.clone()]);

    let subject = Address::generate(&env);
    client.issue_attestation(&issuer, &subject, &pid, &enrolled, &10_000u64);
    // income_band attestation never issued.

    assert!(!client.evaluate_eligibility(&pid, &subject));
}

#[test]
fn test_evaluate_eligibility_accepts_globally_scoped_attestation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 1);
    let identity = Symbol::new(&env, "identity_verified");
    client.publish_eligibility_rule(&admin, &pid, &vec![&env, identity.clone()]);

    let subject = Address::generate(&env);
    let global = client.global_scope();
    client.issue_attestation(&issuer, &subject, &global, &identity, &10_000u64);

    assert!(client.evaluate_eligibility(&pid, &subject));
}

#[test]
fn test_mentorship_rule_composes_all_requested_criteria_privately() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 21);
    let subject = Address::generate(&env);
    let criteria = vec![
        &env,
        MentorshipCriterion::Role(BytesN::from_array(&env, &[1; 32])),
        MentorshipCriterion::Organization(BytesN::from_array(&env, &[2; 32])),
        MentorshipCriterion::CourseProgress(BytesN::from_array(&env, &[3; 32]), 8_000),
        MentorshipCriterion::Skill(BytesN::from_array(&env, &[4; 32]), 2),
        MentorshipCriterion::GeographyPolicy(BytesN::from_array(&env, &[5; 32])),
        MentorshipCriterion::AgeGuardian(18, true),
        MentorshipCriterion::Prerequisite(program_id(&env, 22)),
    ];
    let no_alternatives = soroban_sdk::Vec::new(&env);
    assert_eq!(
        client.publish_mentorship_eligibility_rule(&admin, &pid, &criteria, &no_alternatives),
        1
    );

    let evidence_hash = BytesN::from_array(&env, &[99; 32]);
    for criterion in criteria.iter() {
        client.issue_criterion_attestation(
            &issuer,
            &subject,
            &pid,
            &criterion,
            &evidence_hash,
            &10_000,
        );
    }
    assert!(client.evaluate_mentorship_eligibility(&pid, &subject));

    client.revoke_criterion_attestation(
        &issuer,
        &subject,
        &pid,
        &MentorshipCriterion::AgeGuardian(18, true),
    );
    assert!(!client.evaluate_mentorship_eligibility(&pid, &subject));
}

#[test]
fn test_mentorship_rule_supports_any_of_and_requires_authorized_criterion() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 23);
    let subject = Address::generate(&env);
    let role = MentorshipCriterion::Role(BytesN::from_array(&env, &[1; 32]));
    let skill = MentorshipCriterion::Skill(BytesN::from_array(&env, &[2; 32]), 3);
    let course = MentorshipCriterion::CourseProgress(BytesN::from_array(&env, &[3; 32]), 7_500);
    let required = vec![&env, role.clone()];
    let alternatives = vec![&env, skill.clone(), course.clone()];
    client.publish_mentorship_eligibility_rule(&admin, &pid, &required, &alternatives);

    let evidence_hash = BytesN::from_array(&env, &[88; 32]);
    client.issue_criterion_attestation(
        &issuer,
        &subject,
        &pid,
        &role,
        &evidence_hash,
        &10_000,
    );
    assert!(!client.evaluate_mentorship_eligibility(&pid, &subject));

    client.issue_criterion_attestation(
        &issuer,
        &subject,
        &pid,
        &course,
        &evidence_hash,
        &10_000,
    );
    assert!(client.evaluate_mentorship_eligibility(&pid, &subject));

    let unrelated = MentorshipCriterion::GeographyPolicy(BytesN::from_array(&env, &[4; 32]));
    assert_eq!(
        client.try_issue_criterion_attestation(
            &issuer,
            &subject,
            &pid,
            &unrelated,
            &evidence_hash,
            &10_000,
        ),
        Err(Ok(ContractError::CriterionNotInRule))
    );
}

#[test]
fn test_mentorship_rule_rejects_empty_and_invalid_criteria() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 24);
    let empty = soroban_sdk::Vec::new(&env);
    assert_eq!(
        client.try_publish_mentorship_eligibility_rule(&admin, &pid, &empty, &empty),
        Err(Ok(ContractError::EmptyMentorshipRule))
    );

    let invalid = vec![
        &env,
        MentorshipCriterion::CourseProgress(BytesN::from_array(&env, &[1; 32]), 10_001),
    ];
    assert_eq!(
        client.try_publish_mentorship_eligibility_rule(&admin, &pid, &invalid, &empty),
        Err(Ok(ContractError::InvalidCriterion))
    );

    let mut oversized = soroban_sdk::Vec::new(&env);
    for index in 0..21 {
        oversized.push_back(MentorshipCriterion::Role(BytesN::from_array(
            &env,
            &[index; 32],
        )));
    }
    assert_eq!(
        client.try_publish_mentorship_eligibility_rule(&admin, &pid, &oversized, &empty),
        Err(Ok(ContractError::MentorshipRuleTooLarge))
    );
}

#[test]
fn test_mentorship_rule_lookup_reports_missing_rule_and_attestation() {
    let (env, contract_id, _admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 25);
    let subject = Address::generate(&env);
    let criterion = MentorshipCriterion::Role(BytesN::from_array(&env, &[7; 32]));

    assert_eq!(
        client.try_get_latest_mentorship_rule_version(&pid),
        Err(Ok(ContractError::NoMentorshipRulePublished))
    );
    assert_eq!(
        client.try_get_mentorship_eligibility_rule(&pid, &1),
        Err(Ok(ContractError::MentorshipRuleNotFound))
    );
    assert_eq!(
        client.try_get_criterion_attestation(&subject, &pid, &criterion),
        Err(Ok(ContractError::CriterionAttestationNotFound))
    );
}

#[test]
fn test_only_criterion_issuer_or_admin_can_revoke() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 26);
    let subject = Address::generate(&env);
    let criterion = MentorshipCriterion::GeographyPolicy(BytesN::from_array(&env, &[8; 32]));
    let all_of = vec![&env, criterion.clone()];
    let any_of = soroban_sdk::Vec::new(&env);
    client.publish_mentorship_eligibility_rule(&admin, &pid, &all_of, &any_of);
    client.issue_criterion_attestation(
        &issuer,
        &subject,
        &pid,
        &criterion,
        &BytesN::from_array(&env, &[9; 32]),
        &10_000,
    );

    assert_eq!(
        client.try_revoke_criterion_attestation(&stranger, &subject, &pid, &criterion),
        Err(Ok(ContractError::NotCriterionAttestationIssuer))
    );
}

#[test]
fn test_evaluate_eligibility_ignores_revoked_attestation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 1);
    let enrolled = Symbol::new(&env, "enrolled");
    client.publish_eligibility_rule(&admin, &pid, &vec![&env, enrolled.clone()]);

    let subject = Address::generate(&env);
    client.issue_attestation(&issuer, &subject, &pid, &enrolled, &10_000u64);
    assert!(client.evaluate_eligibility(&pid, &subject));

    client.revoke_attestation(&issuer, &subject, &pid, &enrolled);
    assert!(!client.evaluate_eligibility(&pid, &subject));
}

#[test]
fn test_evaluate_eligibility_requires_published_rule() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    let subject = Address::generate(&env);

    let result = client.try_evaluate_eligibility(&pid, &subject);
    assert_eq!(result, Err(Ok(ContractError::NoRulePublished)));
}

// Evaluation is deterministic: repeated calls with unchanged state must
// agree.
#[test]
fn test_evaluate_eligibility_is_deterministic() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipEligibilityContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let issuer = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    let pid = program_id(&env, 1);
    let enrolled = Symbol::new(&env, "enrolled");
    client.publish_eligibility_rule(&admin, &pid, &vec![&env, enrolled.clone()]);

    let subject = Address::generate(&env);
    client.issue_attestation(&issuer, &subject, &pid, &enrolled, &10_000u64);

    let first = client.evaluate_eligibility(&pid, &subject);
    let second = client.evaluate_eligibility(&pid, &subject);
    assert_eq!(first, second);
    assert!(first);
}
