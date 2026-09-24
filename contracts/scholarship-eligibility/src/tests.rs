#![cfg(test)]
use crate::{ContractError, ScholarshipEligibilityContract};
use soroban_sdk::{testutils::Address as _, vec, Address, BytesN, Env, Symbol};

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

    let required = vec![&env, Symbol::new(&env, "enrolled"), Symbol::new(&env, "income_band")];
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

    let v2_types = vec![&env, Symbol::new(&env, "enrolled"), Symbol::new(&env, "income_band")];
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
