#![cfg(test)]
use crate::{
    PrivacyError, PrivacyRequestStatus, PrivacyRequestType, ScholarshipApplicationsContract,
    ScholarshipApplicationsContractClient,
};
use soroban_sdk::{
    testutils::Address as _,
    Address, BytesN, Env,
};

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

fn submit_sample_application(
    env: &Env,
    client: &ScholarshipApplicationsContractClient,
    admin: &Address,
    applicant: &Address,
    pid: &BytesN<32>,
    initial_hash: &BytesN<32>,
) {
    client.initialize(admin);
    client.register_program(admin, pid, &1_000_000u64);
    client.publish_form_schema(admin, pid, &hash(env, 10));
    client.publish_consent_terms(admin, pid, &hash(env, 20));
    client.record_consent(applicant, pid);
    client.submit_application(applicant, pid, initial_hash, &1);
}

#[test]
fn test_request_correction_success() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let initial_hash = hash(&env, 1);
    let corrected_hash = hash(&env, 2);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &initial_hash);

    let req_id = client.request_correction(&applicant, &pid, &corrected_hash);
    assert_eq!(req_id, 1);

    let req = client.get_privacy_request(&req_id);
    assert_eq!(req.id, 1);
    assert_eq!(req.applicant, applicant);
    assert_eq!(req.program_id, pid);
    assert_eq!(req.request_type, PrivacyRequestType::Correction);
    assert_eq!(req.status, PrivacyRequestStatus::Pending);
    assert_eq!(req.target_data_hash, corrected_hash);
    assert_eq!(req.downstream_addressed, false);
    assert_eq!(client.get_latest_privacy_request_id(&applicant, &pid), Some(1));
}

#[test]
fn test_request_erasure_success() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let initial_hash = hash(&env, 1);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &initial_hash);

    let req_id = client.request_erasure(&applicant, &pid);
    assert_eq!(req_id, 1);

    let req = client.get_privacy_request(&req_id);
    assert_eq!(req.id, 1);
    assert_eq!(req.applicant, applicant);
    assert_eq!(req.request_type, PrivacyRequestType::Erasure);
    assert_eq!(req.status, PrivacyRequestStatus::Pending);
    assert_eq!(req.target_data_hash, hash(&env, 0));
    assert_eq!(req.downstream_addressed, false);
}

#[test]
fn test_request_without_application_fails() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    client.initialize(&admin);

    let err = client.try_request_erasure(&applicant, &pid);
    assert_eq!(err, Err(Ok(PrivacyError::ApplicationNotFound)));

    let err_corr = client.try_request_correction(&applicant, &pid, &hash(&env, 2));
    assert_eq!(err_corr, Err(Ok(PrivacyError::ApplicationNotFound)));
}

#[test]
fn test_approve_correction_updates_application() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let initial_hash = hash(&env, 1);
    let corrected_hash = hash(&env, 2);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &initial_hash);

    let req_id = client.request_correction(&applicant, &pid, &corrected_hash);
    client.process_privacy_request(&admin, &req_id, &true, &hash(&env, 0), &true);

    let req = client.get_privacy_request(&req_id);
    assert_eq!(req.status, PrivacyRequestStatus::Approved);
    assert_eq!(req.downstream_addressed, true);

    // Verify application state has been updated to corrected hash
    let app = client.get_application(&applicant, &pid);
    assert_eq!(app.data_hash, corrected_hash);
}

#[test]
fn test_approve_erasure_redacts_application_hash() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let initial_hash = hash(&env, 1);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &initial_hash);

    let req_id = client.request_erasure(&applicant, &pid);
    client.process_privacy_request(&admin, &req_id, &true, &hash(&env, 0), &true);

    let req = client.get_privacy_request(&req_id);
    assert_eq!(req.status, PrivacyRequestStatus::Approved);
    assert_eq!(req.downstream_addressed, true);

    // Verify application answers commitment is redacted with zero hash,
    // while immutable application entry is preserved for financial audit records
    let app = client.get_application(&applicant, &pid);
    assert_eq!(app.data_hash, hash(&env, 0));
}

#[test]
fn test_deny_request_documents_reason() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let initial_hash = hash(&env, 1);
    let denial_reason = hash(&env, 99); // Commitment to statutory audit retention basis

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &initial_hash);

    let req_id = client.request_erasure(&applicant, &pid);
    client.process_privacy_request(&admin, &req_id, &false, &denial_reason, &false);

    let req = client.get_privacy_request(&req_id);
    assert_eq!(req.status, PrivacyRequestStatus::Denied);
    assert_eq!(req.denial_reason_hash, denial_reason);
    assert_eq!(req.downstream_addressed, false);

    // Application data hash remains untouched
    let app = client.get_application(&applicant, &pid);
    assert_eq!(app.data_hash, initial_hash);
}

#[test]
fn test_non_admin_cannot_process_request() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    let unauthorized_caller = Address::generate(&env);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &hash(&env, 1));

    let req_id = client.request_erasure(&applicant, &pid);
    let res = client.try_process_privacy_request(
        &unauthorized_caller,
        &req_id,
        &true,
        &hash(&env, 0),
        &true,
    );
    assert_eq!(res, Err(Ok(PrivacyError::Unauthorized)));
}

#[test]
fn test_already_processed_request_cannot_be_reprocessed() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);

    submit_sample_application(&env, &client, &admin, &applicant, &pid, &hash(&env, 1));

    let req_id = client.request_erasure(&applicant, &pid);
    client.process_privacy_request(&admin, &req_id, &true, &hash(&env, 0), &true);

    let res = client.try_process_privacy_request(&admin, &req_id, &true, &hash(&env, 0), &true);
    assert_eq!(res, Err(Ok(PrivacyError::RequestAlreadyProcessed)));
}

#[test]
fn test_get_non_existent_request_fails() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);

    let res = client.try_get_privacy_request(&999);
    assert_eq!(res, Err(Ok(PrivacyError::RequestNotFound)));
}
