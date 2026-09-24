#![cfg(test)]
use crate::{ContractError, ScholarshipApplicationsContract};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipApplicationsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn program_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn setup_program(
    env: &Env,
    client: &crate::ScholarshipApplicationsContractClient,
    admin: &Address,
    pid: &BytesN<32>,
) {
    client.initialize(admin);
    client.register_program(admin, pid, &1_000_000u64);
}

// ── #1070 — versioned form schemas ──────────────────────────────────────────

#[test]
fn test_publish_form_schema_starts_at_version_one() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    let version = client.publish_form_schema(&admin, &pid, &hash(&env, 1));
    assert_eq!(version, 1);
    assert_eq!(client.get_latest_form_version(&pid), 1);
}

#[test]
fn test_publish_form_schema_versions_increment_and_are_immutable() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    client.publish_form_schema(&admin, &pid, &hash(&env, 1));
    let v2 = client.publish_form_schema(&admin, &pid, &hash(&env, 2));
    assert_eq!(v2, 2);

    // Version 1 must be unchanged — publishing v2 didn't overwrite it.
    let schema_v1 = client.get_form_schema(&pid, &1);
    assert_eq!(schema_v1.schema_hash, hash(&env, 1));
    let schema_v2 = client.get_form_schema(&pid, &2);
    assert_eq!(schema_v2.schema_hash, hash(&env, 2));
}

#[test]
fn test_non_admin_cannot_publish_form_schema() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    let attacker = Address::generate(&env);
    let result = client.try_publish_form_schema(&attacker, &pid, &hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_get_latest_form_version_before_publish_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    let result = client.try_get_latest_form_version(&pid);
    assert_eq!(result, Err(Ok(ContractError::NoFormSchemaPublished)));
}

// ── #1073 — consent terms + applicant consent ──────────────────────────────

#[test]
fn test_record_consent_requires_published_terms() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    let applicant = Address::generate(&env);
    let result = client.try_record_consent(&applicant, &pid);
    assert_eq!(result, Err(Ok(ContractError::NoConsentTermsPublished)));
}

#[test]
fn test_record_consent_is_affirmative_and_timestamped() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);
    client.publish_consent_terms(&admin, &pid, &hash(&env, 9));

    env.ledger().set_timestamp(555);
    let applicant = Address::generate(&env);
    let version = client.record_consent(&applicant, &pid);
    assert_eq!(version, 1);

    let record = client.get_consent(&applicant, &pid);
    assert_eq!(record.version, 1);
    assert_eq!(record.accepted_at, 555);
    assert!(!record.revoked);
    assert!(client.has_valid_consent(&applicant, &pid));
}

#[test]
fn test_revoke_consent_invalidates_it() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);
    client.publish_consent_terms(&admin, &pid, &hash(&env, 9));

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    assert!(client.has_valid_consent(&applicant, &pid));

    client.revoke_consent(&applicant, &pid);
    assert!(!client.has_valid_consent(&applicant, &pid));

    let record = client.get_consent(&applicant, &pid);
    assert!(record.revoked);
}

// Changing the terms (publishing a new version) must require re-consent —
// an applicant's consent to the old version no longer counts as valid.
#[test]
fn test_republished_terms_require_re_consent() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);
    client.publish_consent_terms(&admin, &pid, &hash(&env, 9));

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    assert!(client.has_valid_consent(&applicant, &pid));

    // Terms change (e.g. updated privacy notice).
    client.publish_consent_terms(&admin, &pid, &hash(&env, 10));
    assert!(!client.has_valid_consent(&applicant, &pid));

    // Re-consenting brings it current again.
    let version = client.record_consent(&applicant, &pid);
    assert_eq!(version, 2);
    assert!(client.has_valid_consent(&applicant, &pid));
}

#[test]
fn test_revoke_consent_without_prior_consent_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    setup_program(&env, &client, &admin, &pid);

    let applicant = Address::generate(&env);
    let result = client.try_revoke_consent(&applicant, &pid);
    assert_eq!(result, Err(Ok(ContractError::ConsentNotFound)));
}

// ── submission wired to form version + consent ──────────────────────────────

fn ready_program(
    env: &Env,
    client: &crate::ScholarshipApplicationsContractClient,
    admin: &Address,
) -> BytesN<32> {
    let pid = program_id(env, 1);
    setup_program(env, client, admin, &pid);
    client.publish_form_schema(admin, &pid, &hash(env, 1));
    client.publish_consent_terms(admin, &pid, &hash(env, 9));
    pid
}

#[test]
fn test_submit_application_success_records_versions() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    client.submit_application(&applicant, &pid, &hash(&env, 42), &1);

    let application = client.get_application(&applicant, &pid);
    assert_eq!(application.form_version, 1);
    assert_eq!(application.consent_version, 1);
}

#[test]
fn test_submit_application_rejects_unknown_form_version() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    let result = client.try_submit_application(&applicant, &pid, &hash(&env, 42), &99);
    assert_eq!(result, Err(Ok(ContractError::FormSchemaNotFound)));
}

#[test]
fn test_submit_application_without_consent_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    let result = client.try_submit_application(&applicant, &pid, &hash(&env, 42), &1);
    assert_eq!(result, Err(Ok(ContractError::ConsentNotFound)));
}

#[test]
fn test_submit_application_with_revoked_consent_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    client.revoke_consent(&applicant, &pid);

    let result = client.try_submit_application(&applicant, &pid, &hash(&env, 42), &1);
    assert_eq!(result, Err(Ok(ContractError::ConsentRevoked)));
}

#[test]
fn test_submit_application_with_stale_consent_version_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);

    // Terms change after consent was recorded but before submission.
    client.publish_consent_terms(&admin, &pid, &hash(&env, 10));

    let result = client.try_submit_application(&applicant, &pid, &hash(&env, 42), &1);
    assert_eq!(result, Err(Ok(ContractError::ConsentOutOfDate)));
}

#[test]
fn test_duplicate_application_still_rejected_with_versioning() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = ready_program(&env, &client, &admin);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &pid);
    client.submit_application(&applicant, &pid, &hash(&env, 42), &1);

    let result = client.try_submit_application(&applicant, &pid, &hash(&env, 43), &1);
    assert_eq!(result, Err(Ok(ContractError::DuplicateApplication)));
}
