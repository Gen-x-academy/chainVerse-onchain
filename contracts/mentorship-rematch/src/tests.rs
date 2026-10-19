//! Deterministic tests for the mentorship rematch workflow (#1238,
//! MENTOR-042).

use crate::{
    ContractError, MatchStatus, MentorshipRematchContract, MentorshipRematchContractClient,
    RematchReason, RematchStats, RequestStatus,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

const BASE: u64 = 1_700_000_000;
const WINDOW: u32 = 86_400;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    mentor: Address,
    mentee: Address,
    coordinator: Address,
    match_id: BytesN<32>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(MentorshipRematchContract, ());
    let c = MentorshipRematchContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    c.initialize(&admin, &WINDOW);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let coordinator = Address::generate(&env);
    c.register_coordinator(&admin, &coordinator);
    let match_id = hash(&env, 0xA1);
    c.register_match(&admin, &match_id, &hash(&env, 0xB1), &mentor, &mentee);
    Fixture {
        env,
        contract_id,
        admin,
        mentor,
        mentee,
        coordinator,
        match_id,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipRematchContractClient<'a> {
    MentorshipRematchContractClient::new(env, contract_id)
}

fn request(f: &Fixture, byte: u8) -> BytesN<32> {
    let c = client(&f.env, &f.contract_id);
    let request_id = hash(&f.env, byte);
    c.request_rematch(
        &f.mentee,
        &f.match_id,
        &request_id,
        &RematchReason::Incompatibility,
        &hash(&f.env, byte ^ 0x11),
        &hash(&f.env, byte ^ 0x55),
    );
    request_id
}

/// Drive the request all the way to `ApprovedForClosure`.
fn approved(f: &Fixture, request_id: &BytesN<32>) -> Address {
    let c = client(&f.env, &f.contract_id);
    c.start_review(&f.coordinator, request_id);
    let successor = Address::generate(&f.env);
    c.propose_successor(&f.coordinator, request_id, &successor, &hash(&f.env, 0xC1));
    c.approve_successor(&f.mentee, request_id);
    successor
}

// ── initialization and configuration ───────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_initialize(&f.admin, &WINDOW),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_rejects_out_of_bounds_window() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipRematchContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    // 59 seconds is below MIN_PROPOSAL_WINDOW_SECONDS.
    assert_eq!(
        c.try_initialize(&admin, &59),
        Err(Ok(ContractError::InvalidConfig))
    );
    // Getters fail closed before initialization.
    assert_eq!(c.try_stats(), Err(Ok(ContractError::NotInitialized)));
    assert_eq!(c.try_registrar(), Err(Ok(ContractError::NotInitialized)));
}

#[test]
fn test_admin_actions_reject_non_admin() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let attacker = Address::generate(&f.env);
    assert_eq!(
        c.try_register_coordinator(&attacker, &attacker),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_revoke_coordinator(&attacker, &f.coordinator),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_set_registrar(&attacker, &attacker),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_set_proposal_window(&attacker, &3_600),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_upgrade(&attacker, &hash(&f.env, 0xEE)),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_coordinator_set_is_bounded_and_revocable() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Re-registering an existing coordinator is a no-op, not a duplicate.
    c.register_coordinator(&f.admin, &f.coordinator);
    assert_eq!(c.coordinators().len(), 2);

    // The admin is the first coordinator, so 14 more fill the set of 16.
    for _ in 0..14 {
        c.register_coordinator(&f.admin, &Address::generate(&f.env));
    }
    assert!(c.is_coordinator(&f.admin));
    assert_eq!(c.coordinators().len(), 16);
    assert_eq!(
        c.try_register_coordinator(&f.admin, &Address::generate(&f.env)),
        Err(Ok(ContractError::CoordinatorLimitReached))
    );

    // Revocation removes the role (deny-by-default afterwards).
    c.revoke_coordinator(&f.admin, &f.coordinator);
    assert!(!c.is_coordinator(&f.coordinator));
    assert_eq!(
        c.try_revoke_coordinator(&f.admin, &f.coordinator),
        Err(Ok(ContractError::CoordinatorNotFound))
    );
    let request_id = request(&f, 1);
    assert_eq!(
        c.try_start_review(&f.coordinator, &request_id),
        Err(Ok(ContractError::NotCoordinator))
    );
}

#[test]
fn test_registrar_role_is_required_and_movable() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_register_match(
            &outsider,
            &hash(&f.env, 0xB2),
            &hash(&f.env, 0xB3),
            &f.mentor,
            &f.mentee
        ),
        Err(Ok(ContractError::NotRegistrar))
    );

    let new_registrar = Address::generate(&f.env);
    c.set_registrar(&f.admin, &new_registrar);
    assert_eq!(
        c.try_register_match(
            &f.admin,
            &hash(&f.env, 0xB4),
            &hash(&f.env, 0xB5),
            &f.mentor,
            &f.mentee
        ),
        Err(Ok(ContractError::NotRegistrar))
    );
    c.register_match(
        &new_registrar,
        &hash(&f.env, 0xB4),
        &hash(&f.env, 0xB5),
        &f.mentor,
        &f.mentee,
    );
    // Duplicate match ids are rejected rather than overwritten.
    assert_eq!(
        c.try_register_match(
            &new_registrar,
            &hash(&f.env, 0xB4),
            &hash(&f.env, 0xB5),
            &f.mentor,
            &f.mentee
        ),
        Err(Ok(ContractError::MatchAlreadyRegistered))
    );
}

#[test]
fn test_register_match_rejects_self_pairing() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_register_match(
            &f.admin,
            &hash(&f.env, 0xB6),
            &hash(&f.env, 0xB7),
            &f.mentor,
            &f.mentor
        ),
        Err(Ok(ContractError::NotAuthorized))
    );
}

// ── rematch requests: authorization, scoping, replay ───────────────────────

#[test]
fn test_request_rematch_flow_and_scoped_reads() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);

    // One canonical event, keyed by the request and carrying only the
    // public reason code.
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "one event per request");
    let (publisher, topics, _) = events.get(0).unwrap();
    assert_eq!(publisher, f.contract_id);
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RMREQ"));

    let stored = c.get_request(&f.mentor, &request_id);
    assert_eq!(stored.status, RequestStatus::Open);
    assert_eq!(stored.requested_by, f.mentee);
    assert_eq!(stored.reason, RematchReason::Incompatibility);
    assert_eq!(stored.reason_hash, hash(&f.env, 0x10));
    assert_eq!(stored.closure_plan_hash, hash(&f.env, 0x54));

    // Coordinators can also read it; outsiders cannot.
    c.get_request(&f.coordinator, &request_id);
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_get_request(&outsider, &request_id),
        Err(Ok(ContractError::NotAuthorized))
    );

    assert_eq!(c.match_requests(&f.match_id).len(), 1);
    assert_eq!(
        c.stats(),
        RematchStats {
            total_matches: 1,
            open_requests: 1,
            closed_requests: 0,
            superseded_matches: 0,
        }
    );
}

#[test]
fn test_request_rematch_requires_participation_and_active_match() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_request_rematch(
            &outsider,
            &f.match_id,
            &hash(&f.env, 2),
            &RematchReason::Other,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        ),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_request_rematch(
            &f.mentee,
            &hash(&f.env, 0xDE),
            &hash(&f.env, 2),
            &RematchReason::Other,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        ),
        Err(Ok(ContractError::MatchNotFound))
    );

    // A closed match cannot be rematched.
    let request_id = request(&f, 5);
    let c2 = client(&f.env, &f.contract_id);
    c2.start_review(&f.coordinator, &request_id);
    c2.close_engagement(&f.coordinator, &request_id);
    assert_eq!(
        c.try_request_rematch(
            &f.mentee,
            &f.match_id,
            &hash(&f.env, 6),
            &RematchReason::Other,
            &hash(&f.env, 7),
            &hash(&f.env, 8),
        ),
        Err(Ok(ContractError::MatchNotActive))
    );
}

#[test]
fn test_request_rematch_requires_closure_plan() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_request_rematch(
            &f.mentee,
            &f.match_id,
            &hash(&f.env, 0x0A),
            &RematchReason::SafetyConcern,
            &hash(&f.env, 0x0B),
            &BytesN::from_array(&f.env, &[0u8; 32]),
        ),
        Err(Ok(ContractError::ClosurePlanMissing))
    );
}

#[test]
fn test_request_retry_is_idempotent_and_single_open() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);

    // Same request id and payload: same request, no duplicate index entry.
    c.request_rematch(
        &f.mentee,
        &f.match_id,
        &request_id,
        &RematchReason::Incompatibility,
        &hash(&f.env, 1 ^ 0x11),
        &hash(&f.env, 1 ^ 0x55),
    );
    assert_eq!(c.match_requests(&f.match_id).len(), 1);
    assert_eq!(c.stats().open_requests, 1);

    // Same id, different payload: stable failure.
    assert_eq!(
        c.try_request_rematch(
            &f.mentee,
            &f.match_id,
            &request_id,
            &RematchReason::Other,
            &hash(&f.env, 0x0C),
            &hash(&f.env, 0x0D),
        ),
        Err(Ok(ContractError::IdempotencyConflict))
    );

    // One open request per match, whoever asks.
    assert_eq!(
        c.try_request_rematch(
            &f.mentor,
            &f.match_id,
            &hash(&f.env, 2),
            &RematchReason::AvailabilityChange,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        ),
        Err(Ok(ContractError::RematchAlreadyOpen))
    );
}

#[test]
fn test_request_history_is_bounded() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Eight rejected requests fill the per-match history.
    for i in 0..8u8 {
        let request_id = request(&f, 0x20 + i);
        c.start_review(&f.coordinator, &request_id);
        c.reject_request(&f.coordinator, &request_id);
    }
    assert_eq!(c.match_requests(&f.match_id).len(), 8);
    assert_eq!(
        c.try_request_rematch(
            &f.mentee,
            &f.match_id,
            &hash(&f.env, 0x2A),
            &RematchReason::Other,
            &hash(&f.env, 0x2B),
            &hash(&f.env, 0x2C),
        ),
        Err(Ok(ContractError::RequestLimitReached))
    );
}

// ── review and rejection ───────────────────────────────────────────────────

#[test]
fn test_review_is_coordinator_only_and_transitions_once() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);

    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_start_review(&outsider, &request_id),
        Err(Ok(ContractError::NotCoordinator))
    );
    c.start_review(&f.coordinator, &request_id);
    assert_eq!(
        c.get_request(&f.mentor, &request_id).status,
        RequestStatus::InReview
    );
    assert_eq!(
        c.try_start_review(&f.coordinator, &request_id),
        Err(Ok(ContractError::InvalidRequestState))
    );

    c.reject_request(&f.coordinator, &request_id);
    let rejected = c.get_request(&f.mentor, &request_id);
    assert_eq!(rejected.status, RequestStatus::Rejected);
    assert_eq!(
        c.try_reject_request(&f.coordinator, &request_id),
        Err(Ok(ContractError::InvalidRequestState))
    );
    assert_eq!(c.stats().open_requests, 0);
    assert_eq!(c.stats().closed_requests, 1);
    // The match itself stays active after a rejection.
    assert_eq!(c.get_match(&f.match_id).status, MatchStatus::Active);

    // Rejected frees the match for a new request.
    request(&f, 2);
}

// ── successor proposal and approval ────────────────────────────────────────

#[test]
fn test_successor_requires_review_and_a_new_person() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);
    let successor = Address::generate(&f.env);

    // Cannot propose before review starts.
    assert_eq!(
        c.try_propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 2)),
        Err(Ok(ContractError::InvalidRequestState))
    );
    c.start_review(&f.coordinator, &request_id);

    // Only coordinators propose.
    assert_eq!(
        c.try_propose_successor(&f.mentee, &request_id, &successor, &hash(&f.env, 2)),
        Err(Ok(ContractError::NotCoordinator))
    );
    // The successor cannot be a current participant.
    assert_eq!(
        c.try_propose_successor(&f.coordinator, &request_id, &f.mentor, &hash(&f.env, 2)),
        Err(Ok(ContractError::InvalidSuccessor))
    );
    assert_eq!(
        c.try_propose_successor(&f.coordinator, &request_id, &f.mentee, &hash(&f.env, 2)),
        Err(Ok(ContractError::InvalidSuccessor))
    );
    c.propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 2));
    let request = c.get_request(&f.mentor, &request_id);
    assert_eq!(request.status, RequestStatus::SuccessorProposed);
    assert_eq!(request.successor, Some(successor.clone()));
    assert_eq!(request.proposal_expires_at, BASE + u64::from(WINDOW));
}

#[test]
fn test_successor_approval_is_mentee_only_and_expires() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);
    c.start_review(&f.coordinator, &request_id);
    let successor = Address::generate(&f.env);
    c.propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 2));

    // The outgoing mentor cannot approve; nor can an outsider.
    assert_eq!(
        c.try_approve_successor(&f.mentor, &request_id),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_approve_successor(&Address::generate(&f.env), &request_id),
        Err(Ok(ContractError::NotAuthorized))
    );

    // A stale approval after the proposal window fails.
    f.env.ledger().set_timestamp(BASE + u64::from(WINDOW) + 1);
    assert_eq!(
        c.try_approve_successor(&f.mentee, &request_id),
        Err(Ok(ContractError::ProposalExpired))
    );

    // A fresh proposal takes effect, and re-approval is idempotent.
    f.env.ledger().set_timestamp(BASE + u64::from(WINDOW) + 2);
    c.set_proposal_window(&f.admin, &WINDOW);
    c.propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 3));
    c.approve_successor(&f.mentee, &request_id);
    c.approve_successor(&f.mentee, &request_id);
    assert_eq!(
        c.get_request(&f.mentor, &request_id).status,
        RequestStatus::ApprovedForClosure
    );
}

// ── closure, supersession and continuity ───────────────────────────────────

#[test]
fn test_close_engagement_preserves_the_record() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let before = c.get_match(&f.match_id);
    let request_id = request(&f, 1);
    c.start_review(&f.coordinator, &request_id);
    c.close_engagement(&f.coordinator, &request_id);

    let after = c.get_match(&f.match_id);
    assert_eq!(after.status, MatchStatus::Closed);
    assert_eq!(after.closed_at, BASE);
    // Nothing was deleted or rewritten.
    assert_eq!(after.match_id, before.match_id);
    assert_eq!(after.mentor, before.mentor);
    assert_eq!(after.mentee, before.mentee);
    assert_eq!(after.agreement_id, before.agreement_id);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.successor_match_id, None);
    assert_eq!(
        c.get_request(&f.mentor, &request_id).status,
        RequestStatus::Closed
    );
    assert_eq!(c.stats().closed_requests, 1);
    assert_eq!(c.stats().open_requests, 0);

    // Closing twice is an invalid transition.
    assert_eq!(
        c.try_close_engagement(&f.coordinator, &request_id),
        Err(Ok(ContractError::InvalidRequestState))
    );
}

#[test]
fn test_apply_successor_records_continuity_in_both_directions() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);
    let successor = approved(&f, &request_id);
    let new_match = hash(&f.env, 0xD1);

    // Only an approved request can be applied.
    c.apply_successor(&f.coordinator, &request_id, &new_match);
    let old = c.get_match(&f.match_id);
    let new = c.get_match(&new_match);
    assert_eq!(old.status, MatchStatus::Superseded);
    assert_eq!(old.successor_match_id, Some(new_match.clone()));
    assert_eq!(old.mentor, f.mentor);
    assert_eq!(new.status, MatchStatus::Active);
    assert_eq!(new.superseded_match_id, Some(f.match_id.clone()));
    assert_eq!(new.mentor, successor);
    assert_eq!(new.mentee, f.mentee);

    let request_record = c.get_request(&f.mentor, &request_id);
    assert_eq!(request_record.status, RequestStatus::Superseded);
    assert_eq!(request_record.closed_at, BASE);

    let stats = c.stats();
    assert_eq!(stats.total_matches, 2);
    assert_eq!(stats.superseded_matches, 1);
    assert_eq!(stats.open_requests, 0);
    assert_eq!(stats.closed_requests, 1);

    // Re-applying the same request is an invalid transition, never a
    // second supersession.
    assert_eq!(
        c.try_apply_successor(&f.coordinator, &request_id, &hash(&f.env, 0xD2)),
        Err(Ok(ContractError::InvalidRequestState))
    );
}

#[test]
fn test_apply_successor_requires_approval_and_a_free_match_id() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);
    c.start_review(&f.coordinator, &request_id);
    let successor = Address::generate(&f.env);
    c.propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 2));

    // Not approved yet.
    assert_eq!(
        c.try_apply_successor(&f.coordinator, &request_id, &hash(&f.env, 0xD3)),
        Err(Ok(ContractError::InvalidRequestState))
    );
    c.approve_successor(&f.mentee, &request_id);

    // The successor match id must be unused.
    assert_eq!(
        c.try_apply_successor(&f.coordinator, &request_id, &f.match_id),
        Err(Ok(ContractError::MatchAlreadyRegistered))
    );
}

#[test]
fn test_open_cases_block_unsafe_closure() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let request_id = request(&f, 1);
    c.start_review(&f.coordinator, &request_id);

    // Only coordinators may flag a case.
    assert_eq!(
        c.try_set_case_flags(&f.mentor, &f.match_id, &true, &false),
        Err(Ok(ContractError::NotCoordinator))
    );
    c.set_case_flags(&f.coordinator, &f.match_id, &true, &false);
    let flags = c.get_case_flags(&f.match_id);
    assert!(flags.financial_open);
    assert!(!flags.safety_open);

    assert_eq!(
        c.try_close_engagement(&f.coordinator, &request_id),
        Err(Ok(ContractError::UnsafeClosureBlocked))
    );

    // Safety cases block supersession too.
    let successor = Address::generate(&f.env);
    c.propose_successor(&f.coordinator, &request_id, &successor, &hash(&f.env, 2));
    c.approve_successor(&f.mentee, &request_id);
    c.set_case_flags(&f.coordinator, &f.match_id, &false, &true);
    assert_eq!(
        c.try_apply_successor(&f.coordinator, &request_id, &hash(&f.env, 0xD4)),
        Err(Ok(ContractError::UnsafeClosureBlocked))
    );

    // Once the cases are cleared, closure is allowed again.
    c.set_case_flags(&f.coordinator, &f.match_id, &false, &false);
    c.apply_successor(&f.coordinator, &request_id, &hash(&f.env, 0xD4));
}

#[test]
fn test_metrics_are_aggregate_only() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.stats(),
        RematchStats {
            total_matches: 1,
            open_requests: 0,
            closed_requests: 0,
            superseded_matches: 0,
        }
    );
    let request_id = request(&f, 1);
    let successor = approved(&f, &request_id);
    c.apply_successor(&f.coordinator, &request_id, &hash(&f.env, 0xD5));
    let stats = c.stats();
    // Counters only: no address, match id or per-person score is exposed by
    // the type at all.
    assert_eq!(stats.total_matches, 2);
    assert_eq!(stats.superseded_matches, 1);
    let _ = successor;
}

// ── upgrade compatibility ──────────────────────────────────────────────────

#[test]
fn test_version_is_stable_for_upgrade_compatibility() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(c.version(), 1);
    assert_eq!(c.admin(), f.admin);
    assert_eq!(c.registrar(), f.admin);
}
