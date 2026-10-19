#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    AgreementStatus, AmendmentStatus, MatchingAllowed, MentorshipAgreementsContract,
    MentorshipAgreementsContractClient, MentorshipError,
};

fn create_test_contract() -> (Env, MentorshipAgreementsContractClient, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, MentorshipAgreementsContract);
    let client = MentorshipAgreementsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    (env, client, admin)
}

fn create_test_hash(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

// ========== INITIALIZATION TESTS ==========

#[test]
fn test_init_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, MentorshipAgreementsContract);
    let client = MentorshipAgreementsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let stored_admin = client.get_admin();
    assert_eq!(stored_admin, admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_double_init_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, MentorshipAgreementsContract);
    let client = MentorshipAgreementsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);
    client.init(&admin); // Should fail
}

// ========== BLOCKING TESTS (MENTOR-044) ==========

#[test]
fn test_block_user_succeeds() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    let reason_hash = Some(create_test_hash(&env, 1));
    client.block_user(&user1, &user2, &reason_hash);

    let block = client.get_block(&user1, &user2);
    assert!(block.is_some());

    let block = block.unwrap();
    assert_eq!(block.blocker, user1);
    assert_eq!(block.blocked, user2);
    assert_eq!(block.reason_hash, reason_hash);
}

#[test]
#[should_panic(expected = "Error(Contract, #24)")]
fn test_block_self_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user = Address::generate(&env);
    client.block_user(&user, &user, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_duplicate_block_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.block_user(&user1, &user2, &None);
    client.block_user(&user1, &user2, &None); // Should fail
}

#[test]
fn test_unblock_user_succeeds() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    client.block_user(&user1, &user2, &None);
    assert!(client.get_block(&user1, &user2).is_some());

    client.unblock_user(&user1, &user2);
    assert!(client.get_block(&user1, &user2).is_none());
}

#[test]
fn test_check_matching_allowed() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    // Initially allowed
    let result = client.check_matching_allowed(&user1, &user2);
    assert_eq!(result, MatchingAllowed::Allowed);

    // User1 blocks user2
    client.block_user(&user1, &user2, &None);
    let result = client.check_matching_allowed(&user1, &user2);
    assert_eq!(result, MatchingAllowed::BlockedByFirst);

    // User2 also blocks user1 (mutual)
    client.block_user(&user2, &user1, &None);
    let result = client.check_matching_allowed(&user1, &user2);
    assert_eq!(result, MatchingAllowed::MutualBlock);
}

// ========== AGREEMENT CREATION TESTS (MENTOR-045) ==========

#[test]
fn test_create_agreement_succeeds() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),  // program_hash
        &create_test_hash(&env, 2),  // scope_hash
        &1000,                        // start_time
        &2000,                        // end_time
        &604800,                      // cadence_seconds (weekly)
        &1000000,                     // price_per_session
        &token,
        &create_test_hash(&env, 3),  // cancellation_policy_hash
        &create_test_hash(&env, 4),  // conduct_hash
        &create_test_hash(&env, 5),  // evidence_hash
        &1,                           // terms_version
    );

    assert_eq!(agreement_id, 1);

    let agreement = client.get_agreement(&agreement_id);
    assert_eq!(agreement.id, 1);
    assert_eq!(agreement.mentor, mentor);
    assert_eq!(agreement.mentee, mentee);
    assert_eq!(agreement.status, AgreementStatus::Pending);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_create_agreement_same_participant_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let user = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_agreement(
        &user,
        &user, // Same as mentor
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #21)")]
fn test_create_agreement_with_blocked_user_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    // Block the mentee
    client.block_user(&mentor, &mentee, &None);

    // Try to create agreement - should fail
    client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_create_agreement_invalid_time_range_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &2000, // start_time after end_time
        &1000, // end_time
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );
}

// ========== ACCEPTANCE TESTS (MENTOR-046) ==========

#[test]
fn test_accept_agreement_activates_when_both_accept() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);

    // Get nonces
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;
    let mentee_nonce = client.get_user_nonce(&mentee) + 1;

    // Mentor accepts
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    assert!(client.has_accepted(&agreement_id, &mentor));

    let agreement = client.get_agreement(&agreement_id);
    assert_eq!(agreement.status, AgreementStatus::Pending); // Still pending

    // Mentee accepts
    client.accept_agreement(&mentee, &agreement_id, &agreement_hash, &mentee_nonce);
    assert!(client.has_accepted(&agreement_id, &mentee));

    // Now should be active
    let agreement = client.get_agreement(&agreement_id);
    assert_eq!(agreement.status, AgreementStatus::Active);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_double_acceptance_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;

    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    
    let mentor_nonce2 = client.get_user_nonce(&mentor) + 1;
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce2); // Should fail
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_invalid_nonce_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let wrong_nonce = 999;

    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &wrong_nonce);
}

// ========== AMENDMENT TESTS (MENTOR-048) ==========

#[test]
fn test_propose_amendment_succeeds() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    // Create and activate agreement
    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;
    let mentee_nonce = client.get_user_nonce(&mentee) + 1;
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    client.accept_agreement(&mentee, &agreement_id, &agreement_hash, &mentee_nonce);

    // Propose amendment
    let amendment_id = client.propose_amendment(
        &mentor,
        &agreement_id,
        &Some(1209600), // new cadence (bi-weekly)
        &None,
        &None,
        &None,
        &Some(1500000), // new price
        &None,
        &create_test_hash(&env, 50), // impact_hash
    );

    assert_eq!(amendment_id, 1);

    let amendment = client.get_amendment(&amendment_id);
    assert_eq!(amendment.status, AmendmentStatus::Proposed);
    assert!(amendment.mentor_approved); // Proposer auto-approves
    assert!(!amendment.mentee_approved);
}

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_propose_amendment_on_pending_agreement_fails() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    // Try to propose amendment on pending agreement
    client.propose_amendment(
        &mentor,
        &agreement_id,
        &Some(1209600),
        &None,
        &None,
        &None,
        &None,
        &None,
        &create_test_hash(&env, 50),
    );
}

#[test]
fn test_amendment_approval_workflow() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    // Create and activate agreement
    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;
    let mentee_nonce = client.get_user_nonce(&mentee) + 1;
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    client.accept_agreement(&mentee, &agreement_id, &agreement_hash, &mentee_nonce);

    // Mentee proposes amendment
    let amendment_id = client.propose_amendment(
        &mentee,
        &agreement_id,
        &Some(1209600),
        &None,
        &None,
        &None,
        &Some(1500000),
        &None,
        &create_test_hash(&env, 50),
    );

    let amendment = client.get_amendment(&amendment_id);
    assert!(amendment.mentee_approved);
    assert!(!amendment.mentor_approved);
    assert_eq!(amendment.status, AmendmentStatus::Proposed);

    // Mentor approves
    client.approve_amendment(&mentor, &amendment_id);

    let amendment = client.get_amendment(&amendment_id);
    assert!(amendment.mentor_approved);
    assert!(amendment.mentee_approved);
    assert_eq!(amendment.status, AmendmentStatus::Approved);
}

#[test]
fn test_activate_amendment_applies_changes() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    // Create and activate agreement
    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;
    let mentee_nonce = client.get_user_nonce(&mentee) + 1;
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    client.accept_agreement(&mentee, &agreement_id, &agreement_hash, &mentee_nonce);

    // Propose and approve amendment
    let new_cadence = 1209600u64;
    let new_price = 1500000i128;
    let amendment_id = client.propose_amendment(
        &mentor,
        &agreement_id,
        &Some(new_cadence),
        &None,
        &None,
        &None,
        &Some(new_price),
        &None,
        &create_test_hash(&env, 50),
    );

    client.approve_amendment(&mentee, &amendment_id);

    // Activate amendment
    client.activate_amendment(&mentor, &amendment_id);

    // Verify changes applied
    let agreement = client.get_agreement(&agreement_id);
    assert_eq!(agreement.cadence_seconds, new_cadence);
    assert_eq!(agreement.price_per_session, new_price);

    let amendment = client.get_amendment(&amendment_id);
    assert_eq!(amendment.status, AmendmentStatus::Activated);
    assert!(amendment.activated_at.is_some());
}

#[test]
fn test_reject_amendment() {
    let (_env, client, _admin) = create_test_contract();
    let env = _env;

    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let token = Address::generate(&env);

    // Create and activate agreement
    let agreement_id = client.create_agreement(
        &mentor,
        &mentee,
        &create_test_hash(&env, 1),
        &create_test_hash(&env, 2),
        &1000,
        &2000,
        &604800,
        &1000000,
        &token,
        &create_test_hash(&env, 3),
        &create_test_hash(&env, 4),
        &create_test_hash(&env, 5),
        &1,
    );

    let agreement_hash = create_test_hash(&env, 100);
    let mentor_nonce = client.get_user_nonce(&mentor) + 1;
    let mentee_nonce = client.get_user_nonce(&mentee) + 1;
    client.accept_agreement(&mentor, &agreement_id, &agreement_hash, &mentor_nonce);
    client.accept_agreement(&mentee, &agreement_id, &agreement_hash, &mentee_nonce);

    // Propose amendment
    let amendment_id = client.propose_amendment(
        &mentor,
        &agreement_id,
        &Some(1209600),
        &None,
        &None,
        &None,
        &None,
        &None,
        &create_test_hash(&env, 50),
    );

    // Mentee rejects
    client.reject_amendment(&mentee, &amendment_id);

    let amendment = client.get_amendment(&amendment_id);
    assert_eq!(amendment.status, AmendmentStatus::Rejected);

    // Original agreement unchanged
    let agreement = client.get_agreement(&agreement_id);
    assert_eq!(agreement.cadence_seconds, 604800);
}
