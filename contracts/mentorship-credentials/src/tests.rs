#![cfg(test)]

use crate::{ClaimStatus, ContractError, MentorshipCredentialsContract};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    symbol_short, Address, BytesN, Env, Symbol,
};

fn setup() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipCredentialsContract, ());
    let admin = Address::generate(&env);
    let mentor = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    client.set_reviewer(&admin, &reviewer, &true);
    (env, contract_id, admin, mentor, reviewer)
}

fn hash(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

fn submit(
    env: &Env,
    client: &crate::MentorshipCredentialsContractClient,
    mentor: &Address,
    claim_id: &BytesN<32>,
) {
    client.submit_claim(mentor, claim_id, &1, &8, &hash(env, 2), &hash(env, 3));
}

#[test]
fn submit_claim_records_only_public_commitments_and_emits_submission_event() {
    let (env, contract_id, _, mentor, _) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 1);
    submit(&env, &client, &mentor, &claim_id);

    let claim = client.get_claim(&mentor, &claim_id);
    assert_eq!(claim.mentor, mentor);
    assert_eq!(claim.claim_id, claim_id);
    assert_eq!(claim.category_code, 1);
    assert_eq!(claim.experience_years, 8);
    assert_eq!(claim.claim_hash, hash(&env, 2));
    assert_eq!(claim.evidence_ref_hash, hash(&env, 3));
    assert_eq!(claim.status, ClaimStatus::Submitted);
    assert_eq!(claim.reviewer, None);
    assert_eq!(claim.reviewed_at, 0);

    let events = env.events().all();
    let (_, topics, data) = events.get(1).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CLMSUB"));
    let (event_mentor, event_id, category, years): (Address, BytesN<32>, u32, u32) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_mentor, mentor);
    assert_eq!(event_id, claim_id);
    assert_eq!(category, 1);
    assert_eq!(years, 8);
}

#[test]
fn credential_claim_can_omit_experience_years() {
    let (env, contract_id, _, mentor, _) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 5);
    client.submit_claim(&mentor, &claim_id, &2, &0, &hash(&env, 6), &hash(&env, 7));

    let claim = client.get_claim(&mentor, &claim_id);
    assert_eq!(claim.category_code, 2);
    assert_eq!(claim.experience_years, 0);
}

#[test]
fn reviewer_can_approve_reject_expire_and_revoke_individual_claims() {
    let (env, contract_id, _, mentor, reviewer) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let approved = hash(&env, 10);
    let rejected = hash(&env, 11);
    let expired = hash(&env, 12);
    submit(&env, &client, &mentor, &approved);
    submit(&env, &client, &mentor, &rejected);
    submit(&env, &client, &mentor, &expired);

    client.approve_claim(&reviewer, &mentor, &approved);
    client.reject_claim(&reviewer, &mentor, &rejected, &4);
    client.expire_claim(&reviewer, &mentor, &expired, &5);

    let approved_record = client.get_claim(&mentor, &approved);
    assert_eq!(approved_record.status, ClaimStatus::Approved);
    assert_eq!(approved_record.reviewer, Some(reviewer.clone()));
    assert_eq!(approved_record.reason_code, 0);
    let rejected_record = client.get_claim(&mentor, &rejected);
    assert_eq!(rejected_record.status, ClaimStatus::Rejected);
    assert_eq!(rejected_record.reason_code, 4);
    assert_eq!(client.get_claim(&mentor, &expired).status, ClaimStatus::Expired);

    client.revoke_claim(&reviewer, &mentor, &approved, &6);
    let revoked_record = client.get_claim(&mentor, &approved);
    assert_eq!(revoked_record.status, ClaimStatus::Revoked);
    assert_eq!(revoked_record.reason_code, 6);
    assert_eq!(revoked_record.reviewed_at, env.ledger().timestamp());
}

#[test]
fn only_admin_manages_reviewers_and_only_reviewers_change_status() {
    let (env, contract_id, admin, mentor, reviewer) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 20);
    submit(&env, &client, &mentor, &claim_id);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_set_reviewer(&stranger, &reviewer, &false),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_approve_claim(&stranger, &mentor, &claim_id),
        Err(Ok(ContractError::UnauthorizedReviewer))
    );

    client.set_reviewer(&admin, &reviewer, &false);
    assert_eq!(
        client.try_approve_claim(&reviewer, &mentor, &claim_id),
        Err(Ok(ContractError::UnauthorizedReviewer))
    );
}

#[test]
fn claims_are_immutable_after_review_and_ids_cannot_be_reused() {
    let (env, contract_id, _, mentor, reviewer) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 30);
    submit(&env, &client, &mentor, &claim_id);
    client.reject_claim(&reviewer, &mentor, &claim_id, &1);
    let events_before = env.events().all().len();

    assert_eq!(
        client.try_approve_claim(&reviewer, &mentor, &claim_id),
        Err(Ok(ContractError::InvalidStatus))
    );
    assert_eq!(
        client.try_submit_claim(&mentor, &claim_id, &1, &8, &hash(&env, 8), &hash(&env, 9)),
        Err(Ok(ContractError::ClaimAlreadyExists))
    );
    assert_eq!(env.events().all().len(), events_before);
}

#[test]
fn invalid_claims_reasons_and_missing_records_fail_closed() {
    let (env, contract_id, _, mentor, reviewer) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 40);

    assert_eq!(
        client.try_submit_claim(&mentor, &claim_id, &0, &8, &hash(&env, 2), &hash(&env, 3)),
        Err(Ok(ContractError::InvalidClaim))
    );
    assert_eq!(
        client.try_submit_claim(&mentor, &claim_id, &1, &8, &hash(&env, 0), &hash(&env, 3)),
        Err(Ok(ContractError::InvalidClaim))
    );
    assert_eq!(
        client.try_submit_claim(&mentor, &hash(&env, 0), &1, &8, &hash(&env, 2), &hash(&env, 3)),
        Err(Ok(ContractError::InvalidClaim))
    );
    assert_eq!(
        client.try_get_claim(&mentor, &claim_id),
        Err(Ok(ContractError::ClaimNotFound))
    );
    submit(&env, &client, &mentor, &claim_id);
    assert_eq!(
        client.try_reject_claim(&reviewer, &mentor, &claim_id, &0),
        Err(Ok(ContractError::InvalidReason))
    );
    assert_eq!(
        client.try_revoke_claim(&reviewer, &mentor, &claim_id, &1),
        Err(Ok(ContractError::InvalidStatus))
    );
}

#[test]
fn claims_cannot_be_submitted_before_initialization() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipCredentialsContract, ());
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let mentor = Address::generate(&env);

    assert_eq!(
        client.try_submit_claim(
            &mentor,
            &hash(&env, 1),
            &1,
            &8,
            &hash(&env, 2),
            &hash(&env, 3)
        ),
        Err(Ok(ContractError::NotInitialized))
    );
}

#[test]
fn reviewer_changes_emit_decision_events() {
    let (env, contract_id, _, mentor, reviewer) = setup();
    let client = crate::MentorshipCredentialsContractClient::new(&env, &contract_id);
    let claim_id = hash(&env, 50);
    submit(&env, &client, &mentor, &claim_id);
    client.approve_claim(&reviewer, &mentor, &claim_id);

    let events = env.events().all();
    let (_, topics, data) = events.get(2).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
    assert_eq!(topic, symbol_short!("CLMAPR"));
    let (event_mentor, event_id, event_reviewer, reviewed_at):
        (Address, BytesN<32>, Address, u64) = data.try_into_val(&env).unwrap();
    assert_eq!(event_mentor, mentor);
    assert_eq!(event_id, claim_id);
    assert_eq!(event_reviewer, reviewer);
    assert_eq!(reviewed_at, env.ledger().timestamp());
}
