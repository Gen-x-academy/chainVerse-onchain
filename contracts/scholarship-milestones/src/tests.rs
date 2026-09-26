#![cfg(test)]
use crate::{
    ContractError, EvidenceStatus, MilestoneStatus, ReasonCode, ScholarshipMilestonesContract,
    VerificationDecision,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipMilestonesContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

/// Bootstraps an initialized contract with milestone `1` created under
/// program `1`, and returns `(env, contract_id, admin, program_id)`.
fn setup_ready() -> (Env, Address, Address, BytesN<32>) {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.create_milestone(&admin, &pid, &1u64);
    (env, contract_id, admin, pid)
}

fn program_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// ── #1094 — milestone setup + evidence submission ───────────────────────────

#[test]
fn test_create_milestone_is_active_and_not_recreatable() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    client.create_milestone(&admin, &pid, &7u64);
    let milestone = client.get_milestone(&pid, &7u64);
    assert_eq!(milestone.status, MilestoneStatus::Active);

    let result = client.try_create_milestone(&admin, &pid, &7u64);
    assert_eq!(result, Err(Ok(ContractError::MilestoneAlreadyExists)));
}

#[test]
fn test_recipient_can_submit_own_evidence() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    assert_eq!(version, 1);
    assert_eq!(client.latest_evidence_version(&pid, &1u64, &recipient), 1);
}

#[test]
fn test_authorized_submitter_can_submit_for_recipient() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let trusted = Address::generate(&env);
    client.add_submitter(&admin, &trusted);
    assert!(client.is_authorized_submitter(&trusted));

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&trusted, &recipient, &pid, &1u64, &hash(&env, 2));
    assert_eq!(version, 1);
}

#[test]
fn test_unauthorized_submitter_rejected() {
    let (env, contract_id, _admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let stranger = Address::generate(&env);
    let recipient = Address::generate(&env);
    let result = client.try_submit_evidence(&stranger, &recipient, &pid, &1u64, &hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::NotAuthorizedSubmitter)));
}

#[test]
fn test_submit_evidence_requires_active_milestone() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.set_milestone_status(&admin, &pid, &1u64, &MilestoneStatus::Closed);

    let recipient = Address::generate(&env);
    let result = client.try_submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::MilestoneNotActive)));
}

#[test]
fn test_submit_evidence_unknown_milestone() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let recipient = Address::generate(&env);
    let result = client.try_submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    assert_eq!(result, Err(Ok(ContractError::MilestoneNotFound)));
}

#[test]
fn test_duplicate_submission_is_idempotent() {
    let (env, contract_id, _admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let recipient = Address::generate(&env);
    let first = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 9));
    let second = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 9));
    assert_eq!(first, second);
    assert_eq!(client.latest_evidence_version(&pid, &1u64, &recipient), 1);
}

#[test]
fn test_different_evidence_creates_new_version() {
    let (env, contract_id, _admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let recipient = Address::generate(&env);
    client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    let v2 = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 2));
    assert_eq!(v2, 2);

    // Version 1 is preserved unchanged.
    let v1 = client.get_evidence(&pid, &1u64, &recipient, &1u32);
    assert_eq!(v1.data_hash, hash(&env, 1));
}

// ── #1095 — verification ───────────────────────────────────────────────────

#[test]
fn test_verifier_can_approve_with_meets_criteria_reason() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));

    client.verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );

    let record = client.get_evidence(&pid, &1u64, &recipient, &version);
    assert_eq!(record.status, EvidenceStatus::Approved);
    assert!(record.decided);
    assert_eq!(record.decided_by, verifier);
    assert_eq!(record.reason_code, ReasonCode::MeetsCriteria);
}

#[test]
fn test_approval_is_terminal_so_eligibility_event_is_at_most_once() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    client.verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );

    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Rejected,
        &ReasonCode::InsufficientEvidence,
    );
    assert_eq!(result, Err(Ok(ContractError::AlreadyDecided)));
}

#[test]
fn test_verifier_cannot_decide_own_evidence() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    // A verifier who submits for themselves is conflicted.
    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let version = client.submit_evidence(&verifier, &verifier, &pid, &1u64, &hash(&env, 1));

    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &verifier,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::VerifierConflict)));
}

#[test]
fn test_verifier_cannot_decide_evidence_they_submitted_for_another() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    client.add_submitter(&admin, &verifier);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&verifier, &recipient, &pid, &1u64, &hash(&env, 1));

    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::VerifierConflict)));
}

#[test]
fn test_unauthorized_verifier_rejected() {
    let (env, contract_id, _admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));

    let stranger = Address::generate(&env);
    let result = client.try_verify_evidence(
        &stranger,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::NotAuthorizedVerifier)));
}

#[test]
fn test_incoherent_reason_codes_are_rejected() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));

    // Approval with a rejection reason is incoherent.
    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::InsufficientEvidence,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidReasonCode)));

    // Rejection cannot claim the criteria were met.
    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Rejected,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidReasonCode)));

    // `NotReviewed` is not a decision reason.
    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::ChangesRequested,
        &ReasonCode::NotReviewed,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidReasonCode)));
}

#[test]
fn test_changes_requested_is_recorded_and_auditable() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));

    client.verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::ChangesRequested,
        &ReasonCode::InsufficientEvidence,
    );

    let record = client.get_evidence(&pid, &1u64, &recipient, &version);
    assert_eq!(record.status, EvidenceStatus::ChangesRequested);
    assert_eq!(record.reason_code, ReasonCode::InsufficientEvidence);
    // The decision did not erase the evidence itself.
    assert_eq!(record.data_hash, hash(&env, 1));
}

#[test]
fn test_resubmission_after_changes_requested_starts_new_pending_version() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let recipient = Address::generate(&env);
    let v1 = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));
    client.verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &v1,
        &VerificationDecision::ChangesRequested,
        &ReasonCode::InsufficientEvidence,
    );

    let v2 = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 2));
    assert_eq!(v2, 2);
    let record = client.get_evidence(&pid, &1u64, &recipient, &v2);
    assert_eq!(record.status, EvidenceStatus::Pending);
    assert!(!record.decided);
}

#[test]
fn test_removed_verifier_can_no_longer_decide() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    client.remove_verifier(&admin, &verifier);

    let recipient = Address::generate(&env);
    let version = client.submit_evidence(&recipient, &recipient, &pid, &1u64, &hash(&env, 1));

    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &version,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::NotAuthorizedVerifier)));
}

#[test]
fn test_verify_unknown_evidence() {
    let (env, contract_id, admin, pid) = setup_ready();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);

    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let recipient = Address::generate(&env);

    let result = client.try_verify_evidence(
        &verifier,
        &pid,
        &1u64,
        &recipient,
        &1u32,
        &VerificationDecision::Approved,
        &ReasonCode::MeetsCriteria,
    );
    assert_eq!(result, Err(Ok(ContractError::EvidenceNotFound)));
}

#[test]
fn test_non_admin_cannot_configure_allowlists() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipMilestonesContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let attacker = Address::generate(&env);
    let target = Address::generate(&env);
    assert_eq!(
        client.try_add_verifier(&attacker, &target),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_add_submitter(&attacker, &target),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_create_milestone(&attacker, &program_id(&env, 1), &1u64),
        Err(Ok(ContractError::NotAdmin))
    );
}
