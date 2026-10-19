#![cfg(test)]
use crate::{
    DataExportStatus, PrivacyError, PrivacyRequestStatus, PrivacyRequestType,
    ScholarshipApplicationsContract, ScholarshipApplicationsContractClient,
};
use soroban_sdk::{
    testutils::{Address as _, Events as _},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
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

#[test]
fn test_data_export_request_is_completed_with_content_and_consent_commitments() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    submit_sample_application(&env, &client, &admin, &applicant, &pid, &hash(&env, 1));

    let request_id = client.request_data_export(&applicant, &pid);
    assert_eq!(
        client.get_latest_data_export_request_id(&applicant, &pid),
        Some(request_id)
    );
    let pending = client.get_data_export_request(&request_id);
    assert_eq!(pending.status, DataExportStatus::Pending);
    assert_eq!(pending.applicant, applicant);
    assert_eq!(pending.program_id, pid);
    assert_eq!(pending.export_hash, hash(&env, 0));

    assert_eq!(
        client.try_request_data_export(&applicant, &pid),
        Err(Ok(PrivacyError::ExportAlreadyPending))
    );

    let export_hash = hash(&env, 30);
    let consent_history_hash = hash(&env, 31);
    client.complete_data_export(&admin, &request_id, &export_hash, &consent_history_hash);

    let completed = client.get_data_export_request(&request_id);
    assert_eq!(completed.status, DataExportStatus::Completed);
    assert_eq!(completed.export_hash, export_hash);
    assert_eq!(completed.consent_history_hash, consent_history_hash);
    assert_eq!(completed.resolved_at, env.ledger().timestamp());
    assert_eq!(
        client.try_complete_data_export(
            &admin,
            &request_id,
            &export_hash,
            &consent_history_hash,
        ),
        Err(Ok(PrivacyError::ExportRequestAlreadyResolved))
    );
}

#[test]
fn test_export_requires_owned_request_and_applicant_can_cancel() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    submit_sample_application(&env, &client, &admin, &applicant, &pid, &hash(&env, 1));
    let request_id = client.request_data_export(&applicant, &pid);

    let other_applicant = Address::generate(&env);
    assert_eq!(
        client.try_cancel_data_export(&other_applicant, &request_id),
        Err(Ok(PrivacyError::Unauthorized))
    );

    client.cancel_data_export(&applicant, &request_id);
    assert_eq!(
        client.get_data_export_request(&request_id).status,
        DataExportStatus::Cancelled
    );
    let another_request = client.request_data_export(&applicant, &pid);
    assert_ne!(another_request, request_id);
}

#[test]
fn test_data_export_requires_an_application_or_consent_record() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let applicant = Address::generate(&env);
    let pid = program_id(&env, 1);

    assert_eq!(
        client.try_request_data_export(&applicant, &pid),
        Err(Ok(PrivacyError::NoExportableData))
    );
}

#[test]
fn test_only_admin_can_complete_data_export() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    submit_sample_application(&env, &client, &admin, &applicant, &pid, &hash(&env, 1));
    let request_id = client.request_data_export(&applicant, &pid);
    let unauthorized = Address::generate(&env);

    assert_eq!(
        client.try_complete_data_export(
            &unauthorized,
            &request_id,
            &hash(&env, 30),
            &hash(&env, 31),
        ),
        Err(Ok(PrivacyError::Unauthorized))
    );
    assert_eq!(
        client.get_data_export_request(&request_id).status,
        DataExportStatus::Pending
    );
}

#[test]
fn test_consent_revocation_emits_history_event() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &contract_id);
    let pid = program_id(&env, 1);
    let applicant = Address::generate(&env);
    client.initialize(&admin);
    client.register_program(&admin, &pid, &1_000_000);
    client.publish_consent_terms(&admin, &pid, &hash(&env, 20));
    let consent_version = client.record_consent(&applicant, &pid);

    client.revoke_consent(&applicant, &pid);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("CONSRVK"));
    let (event_applicant, event_program, event_version): (Address, BytesN<32>, u32) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_applicant, applicant);
    assert_eq!(event_program, pid);
    assert_eq!(event_version, consent_version);
}
