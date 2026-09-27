#![cfg(test)]
use crate::{
    AppealStatus, CommitteeRole, ContractError, DecisionOutcome, DecisionStatus,
    ReservationStatus, ScholarshipDecisionsContract, VoteChoice,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipDecisionsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn key(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

/// Initialize, staff a four-member committee (chair + three reviewers) with a
/// quorum of 2, and register one program (budget 1000, 100/award, 100s
/// reservation TTL, 100s appeal window).
fn ready(
    env: &Env,
    client: &crate::ScholarshipDecisionsContractClient,
    admin: &Address,
) -> (BytesN<32>, Address, Address, Address, Address, Address) {
    client.initialize(admin);

    let chair = Address::generate(env);
    let r1 = Address::generate(env);
    let r2 = Address::generate(env);
    let r3 = Address::generate(env);
    client.add_member(admin, &chair, &CommitteeRole::Chair);
    client.add_member(admin, &r1, &CommitteeRole::Reviewer);
    client.add_member(admin, &r2, &CommitteeRole::Reviewer);
    client.add_member(admin, &r3, &CommitteeRole::Reviewer);
    client.set_quorum(admin, &2u32);

    let pid = key(env, 1);
    client.register_program(admin, &pid, &1_000i128, &100i128, &100u64, &100u64);

    (pid, chair, r1, r2, r3, Address::generate(env))
}

/// Opens, rejects unanimously, and finalizes a decision so it can be appealed.
fn rejected_decision(
    env: &Env,
    client: &crate::ScholarshipDecisionsContractClient,
    pid: &BytesN<32>,
    chair: &Address,
    r1: &Address,
    r2: &Address,
    applicant: &Address,
) {
    client.open_decision(chair, pid, applicant, &1u32, &key(env, 7));
    client.vote(r1, pid, applicant, &VoteChoice::Reject);
    client.vote(r2, pid, applicant, &VoteChoice::Reject);
    client.finalize_decision(chair, pid, applicant);
}

// ── #1087 — committee membership, roles and quorum ──────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_non_admin_cannot_add_member() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let attacker = Address::generate(&env);
    let member = Address::generate(&env);
    let result = client.try_add_member(&attacker, &member, &CommitteeRole::Reviewer);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_add_member_sets_role_and_membership() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let chair = Address::generate(&env);
    client.add_member(&admin, &chair, &CommitteeRole::Chair);

    assert!(client.is_member(&chair));
    assert_eq!(client.get_member_role(&chair), CommitteeRole::Chair);
    assert_eq!(client.get_active_member_count(), 1);
}

#[test]
fn test_removed_member_is_deactivated_but_keeps_role() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let reviewer = Address::generate(&env);
    client.add_member(&admin, &reviewer, &CommitteeRole::Reviewer);
    client.remove_member(&admin, &reviewer);

    assert!(!client.is_member(&reviewer));
    assert_eq!(client.get_active_member_count(), 0);
    // The role record is retained so a re-added member keeps their role.
    assert_eq!(client.get_member_role(&reviewer), CommitteeRole::Reviewer);
}

#[test]
fn test_set_quorum_rejects_out_of_range() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let result = client.try_set_quorum(&admin, &0u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidQuorum)));

    let result = client.try_set_quorum(&admin, &33u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidQuorum)));

    client.set_quorum(&admin, &3u32);
    assert_eq!(client.get_quorum(), 3);
}

#[test]
fn test_open_decision_requires_chair() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, _chair, r1, _r2, _r3, applicant) = ready(&env, &client, &admin);

    // A reviewer is a member but not a chair.
    let result = client.try_open_decision(&r1, &pid, &applicant, &1u32, &key(&env, 7));
    assert_eq!(result, Err(Ok(ContractError::NotChair)));

    // An outsider is not a member at all.
    let outsider = Address::generate(&env);
    let result = client.try_open_decision(&outsider, &pid, &applicant, &1u32, &key(&env, 7));
    assert_eq!(result, Err(Ok(ContractError::NotChair)));
}

#[test]
fn test_open_decision_rejects_inactive_program() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, _r1, _r2, _r3, applicant) = ready(&env, &client, &admin);
    client.set_program_active(&admin, &pid, &false);

    let result = client.try_open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    assert_eq!(result, Err(Ok(ContractError::ProgramInactive)));
}

#[test]
fn test_open_decision_rejects_missing_evidence_version() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, _r1, _r2, _r3, applicant) = ready(&env, &client, &admin);

    let result = client.try_open_decision(&chair, &pid, &applicant, &0u32, &key(&env, 7));
    assert_eq!(result, Err(Ok(ContractError::InvalidEvidence)));
}

#[test]
fn test_open_decision_once_per_application() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, _r1, _r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &3u32, &key(&env, 7));

    let result = client.try_open_decision(&chair, &pid, &applicant, &3u32, &key(&env, 7));
    assert_eq!(result, Err(Ok(ContractError::DecisionAlreadyExists)));

    // The evidence reference is recorded for auditability.
    let decision = client.get_decision(&pid, &applicant);
    assert_eq!(decision.evidence_version, 3);
    assert_eq!(decision.status, DecisionStatus::Open);
    assert_eq!(decision.outcome, DecisionOutcome::Pending);
}

#[test]
fn test_outsider_cannot_vote() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, _r1, _r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));

    let outsider = Address::generate(&env);
    let result = client.try_vote(&outsider, &pid, &applicant, &VoteChoice::Award);
    assert_eq!(result, Err(Ok(ContractError::NotCommitteeMember)));
}

#[test]
fn test_member_votes_at_most_once() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);

    let result = client.try_vote(&r1, &pid, &applicant, &VoteChoice::Reject);
    assert_eq!(result, Err(Ok(ContractError::AlreadyVoted)));

    // The recorded votes are the audit trail.
    let decision = client.get_decision(&pid, &applicant);
    assert_eq!(decision.votes.len(), 1);
    assert_eq!(
        decision.votes.get(0).map(|vote| vote.choice),
        Some(VoteChoice::Award)
    );

    // A different member still can.
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    assert_eq!(client.get_decision(&pid, &applicant).votes.len(), 2);
}

#[test]
fn test_finalize_requires_quorum_and_excludes_recusals() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));

    // Nothing cast yet.
    let result = client.try_finalize_decision(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::QuorumNotMet)));

    // A recusal does not count toward quorum.
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.recuse(&r2, &pid, &applicant);
    let result = client.try_finalize_decision(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::QuorumNotMet)));
}

#[test]
fn test_reviewer_cannot_finalize_decision() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);

    // A plain reviewer is neither chair nor admin.
    let result = client.try_finalize_decision(&r1, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::NotAuthorized)));
}

#[test]
fn test_finalize_records_outcome_and_closes_voting() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &2u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    let decision = client.get_decision(&pid, &applicant);
    assert_eq!(decision.status, DecisionStatus::Finalized);
    assert_eq!(decision.outcome, DecisionOutcome::Awarded);
    assert_eq!(decision.outcome_version, 1);
    assert_eq!(decision.evidence_version, 2);

    // No further votes or finalization.
    let result = client.try_vote(&r3, &pid, &applicant, &VoteChoice::Reject);
    assert_eq!(result, Err(Ok(ContractError::DecisionAlreadyFinalized)));
    let result = client.try_finalize_decision(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::DecisionAlreadyFinalized)));
}

#[test]
fn test_tie_policy_prefers_most_conservative_outcome() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));

    // 1 award vs 1 reject is a tie: the conservative outcome wins.
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Reject);
    client.finalize_decision(&chair, &pid, &applicant);
    assert_eq!(
        client.get_decision(&pid, &applicant).outcome,
        DecisionOutcome::Rejected
    );
}

#[test]
fn test_tie_between_award_and_waitlist_waitslists() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));

    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Waitlist);
    client.finalize_decision(&chair, &pid, &applicant);
    assert_eq!(
        client.get_decision(&pid, &applicant).outcome,
        DecisionOutcome::Waitlisted
    );
}

// ── #1088 — budget reservations ─────────────────────────────────────────────

#[test]
fn test_register_program_rejects_invalid_config() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = key(&env, 1);

    let result =
        client.try_register_program(&admin, &pid, &0i128, &100i128, &100u64, &100u64);
    assert_eq!(result, Err(Ok(ContractError::InvalidProgramConfig)));

    // Per-award amount above the total budget cannot cover even one award.
    let result = client.try_register_program(
        &admin,
        &pid,
        &50i128,
        &100i128,
        &100u64,
        &100u64,
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidProgramConfig)));

    let result =
        client.try_register_program(&admin, &pid, &100i128, &100i128, &0u64, &100u64);
    assert_eq!(result, Err(Ok(ContractError::InvalidProgramConfig)));
}

#[test]
fn test_register_program_once() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, _chair, _r1, _r2, _r3, _applicant) = ready(&env, &client, &admin);

    let result =
        client.try_register_program(&admin, &pid, &1_000i128, &100i128, &100u64, &100u64);
    assert_eq!(result, Err(Ok(ContractError::ProgramAlreadyExists)));
}

#[test]
fn test_award_finalization_reserves_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    let config = client.get_program_config(&pid);
    assert_eq!(config.reserved_amount, 100);
    assert_eq!(config.awarded_amount, 0);

    let reservation = client.get_reservation(&pid, &applicant);
    assert_eq!(reservation.amount, 100);
    assert_eq!(reservation.status, ReservationStatus::Reserved);
    assert_eq!(reservation.expires_at, 100);
}

#[test]
fn test_accept_award_claims_reservation() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    client.accept_award(&applicant, &pid);

    let config = client.get_program_config(&pid);
    assert_eq!(config.reserved_amount, 0);
    assert_eq!(config.awarded_amount, 100);
    assert_eq!(
        client.get_reservation(&pid, &applicant).status,
        ReservationStatus::Claimed
    );

    // Claiming twice cannot free or spend capacity again.
    let result = client.try_accept_award(&applicant, &pid);
    assert_eq!(result, Err(Ok(ContractError::ReservationNotActive)));
}

#[test]
fn test_reservation_cannot_exceed_total_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (_pid, chair, r1, r2, _r3, applicant_a) = ready(&env, &client, &admin);
    let applicant_b = Address::generate(&env);

    // The default program's budget cannot be exhausted by two 100 awards, so
    // use a program that can only afford a single award.
    let tight = key(&env, 2);
    client.register_program(&admin, &tight, &100i128, &100i128, &100u64, &100u64);

    for applicant in [&applicant_a, &applicant_b] {
        client.open_decision(&chair, &tight, applicant, &1u32, &key(&env, 7));
        client.vote(&r1, &tight, applicant, &VoteChoice::Award);
        client.vote(&r2, &tight, applicant, &VoteChoice::Award);
    }
    client.finalize_decision(&chair, &tight, &applicant_a);

    // The second award would push committed budget past the total.
    let result = client.try_finalize_decision(&chair, &tight, &applicant_b);
    assert_eq!(result, Err(Ok(ContractError::BudgetExceeded)));

    let config = client.get_program_config(&tight);
    assert_eq!(config.reserved_amount, 100);
    // The failed decision stays open so it can be retried after capacity frees.
    assert_eq!(
        client.get_decision(&tight, &applicant_b).status,
        DecisionStatus::Open
    );

    // Capacity freed by releasing the first reservation can be reused.
    client.release_reservation(&chair, &tight, &applicant_a);
    client.finalize_decision(&chair, &tight, &applicant_b);
    assert_eq!(client.get_program_config(&tight).reserved_amount, 100);
}

#[test]
fn test_release_reservation_frees_capacity_exactly_once() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    client.release_reservation(&chair, &pid, &applicant);
    assert_eq!(client.get_program_config(&pid).reserved_amount, 0);
    assert_eq!(
        client.get_reservation(&pid, &applicant).status,
        ReservationStatus::Released
    );

    let result = client.try_release_reservation(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::ReservationNotActive)));
}

#[test]
fn test_release_reservation_requires_chair_or_admin() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    let result = client.try_release_reservation(&r2, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::NotAuthorized)));
}

#[test]
fn test_reservation_expires_after_policy_ttl() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    // Not yet expired.
    env.ledger().set_timestamp(100);
    let result = client.try_expire_reservation(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::ReservationNotExpired)));

    // Past the TTL it may be expired by anyone.
    env.ledger().set_timestamp(101);
    let anyone = Address::generate(&env);
    client.expire_reservation(&anyone, &pid, &applicant);

    assert_eq!(client.get_program_config(&pid).reserved_amount, 0);
    assert_eq!(
        client.get_reservation(&pid, &applicant).status,
        ReservationStatus::Expired
    );

    // Expiring again cannot free capacity twice.
    let result = client.try_expire_reservation(&anyone, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::ReservationNotActive)));
}

#[test]
fn test_expired_reservation_cannot_be_accepted() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    env.ledger().set_timestamp(101);
    let result = client.try_accept_award(&applicant, &pid);
    assert_eq!(result, Err(Ok(ContractError::ReservationExpired)));
}

// ── #1089 — applicant appeals ───────────────────────────────────────────────

#[test]
fn test_appeal_requires_finalized_decision() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, _r1, _r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));

    let result = client.try_file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    assert_eq!(result, Err(Ok(ContractError::DecisionNotFinalized)));
}

#[test]
fn test_awarded_decision_is_not_appealable() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    client.open_decision(&chair, &pid, &applicant, &1u32, &key(&env, 7));
    client.vote(&r1, &pid, &applicant, &VoteChoice::Award);
    client.vote(&r2, &pid, &applicant, &VoteChoice::Award);
    client.finalize_decision(&chair, &pid, &applicant);

    let result = client.try_file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    assert_eq!(result, Err(Ok(ContractError::DecisionNotAppealable)));
}

#[test]
fn test_appeal_window_is_enforced() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    rejected_decision(&env, &client, &pid, &chair, &r1, &r2, &applicant);

    // finalized_at = 0 and appeal_window = 100, so 101 is too late.
    env.ledger().set_timestamp(101);
    let result = client.try_file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    assert_eq!(result, Err(Ok(ContractError::AppealWindowClosed)));
}

#[test]
fn test_appeal_can_be_filed_at_most_once() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, _r3, applicant) = ready(&env, &client, &admin);
    rejected_decision(&env, &client, &pid, &chair, &r1, &r2, &applicant);

    client.file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    let result = client.try_file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    assert_eq!(result, Err(Ok(ContractError::AppealAlreadyExists)));
}

#[test]
fn test_original_reviewers_are_excluded_from_appeal() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, r3, applicant) = ready(&env, &client, &admin);
    rejected_decision(&env, &client, &pid, &chair, &r1, &r2, &applicant);
    client.file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));

    // Both original reviewers are ineligible.
    let result = client.try_vote_appeal(&r1, &pid, &applicant, &VoteChoice::Award);
    assert_eq!(
        result,
        Err(Ok(ContractError::AppealReviewerIneligible))
    );
    let result = client.try_vote_appeal(&r2, &pid, &applicant, &VoteChoice::Award);
    assert_eq!(
        result,
        Err(Ok(ContractError::AppealReviewerIneligible))
    );

    // A member who did not review the original decision may vote.
    client.vote_appeal(&r3, &pid, &applicant, &VoteChoice::Award);
    assert_eq!(client.get_appeal(&pid, &applicant).votes.len(), 1);
}

#[test]
fn test_finalize_appeal_requires_quorum() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, r3, applicant) = ready(&env, &client, &admin);
    rejected_decision(&env, &client, &pid, &chair, &r1, &r2, &applicant);
    client.file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));

    client.vote_appeal(&r3, &pid, &applicant, &VoteChoice::Award);
    let result = client.try_finalize_appeal(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::QuorumNotMet)));
}

#[test]
fn test_appeal_resolution_supersedes_decision_and_is_final() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    let (pid, chair, r1, r2, r3, applicant) = ready(&env, &client, &admin);
    rejected_decision(&env, &client, &pid, &chair, &r1, &r2, &applicant);
    client.file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));

    // Fresh panel (chair + r3) overturns the rejection into an award.
    client.vote_appeal(&r3, &pid, &applicant, &VoteChoice::Award);
    client.vote_appeal(&chair, &pid, &applicant, &VoteChoice::Award);
    client.finalize_appeal(&chair, &pid, &applicant);

    // The decision's outcome is superseded and the version bumped.
    let decision = client.get_decision(&pid, &applicant);
    assert_eq!(decision.outcome, DecisionOutcome::Awarded);
    assert_eq!(decision.outcome_version, 2);
    assert_eq!(decision.status, DecisionStatus::Finalized);

    // The appeal is stored separately and resolved.
    let appeal = client.get_appeal(&pid, &applicant);
    assert_eq!(appeal.status, AppealStatus::Resolved);
    assert_eq!(appeal.outcome, DecisionOutcome::Awarded);
    assert_eq!(appeal.original_outcome, DecisionOutcome::Rejected);

    // An award won on appeal reserves budget like any other award.
    assert_eq!(client.get_program_config(&pid).reserved_amount, 100);

    // The outcome is final: no second appeal, and the appeal cannot be
    // finalized again.
    let result = client.try_file_appeal(&applicant, &pid, &key(&env, 8), &key(&env, 9));
    assert_eq!(result, Err(Ok(ContractError::AppealAlreadyExists)));
    let result = client.try_finalize_appeal(&chair, &pid, &applicant);
    assert_eq!(result, Err(Ok(ContractError::AppealNotOpen)));
}

#[test]
fn test_version_is_one() {
    let (env, contract_id, _admin) = setup();
    let client = crate::ScholarshipDecisionsContractClient::new(&env, &contract_id);
    assert_eq!(client.version(), 1);
}
