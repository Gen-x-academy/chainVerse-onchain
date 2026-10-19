//! Deterministic tests for mentorship session booking (#1252, MENTOR-056).

use crate::{
    ContractError, MentorshipSchedulingContract, MentorshipSchedulingContractClient, SessionMode,
    SessionStatus,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

const BASE: u64 = 1_700_000_000;
const LEAD: u32 = 3_600;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    registrar: Address,
    mentor: Address,
    mentee: Address,
    agreement: BytesN<32>,
}

fn fixture() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(BASE);
    let contract_id = env.register(MentorshipSchedulingContract, ());
    let c = MentorshipSchedulingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    c.initialize(&admin, &LEAD);
    let registrar = admin.clone();
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    let agreement = hash(&env, 0xA1);
    c.register_agreement(&registrar, &agreement, &mentor, &mentee);
    Fixture {
        env,
        contract_id,
        admin,
        registrar,
        mentor,
        mentee,
        agreement,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipSchedulingContractClient<'a> {
    MentorshipSchedulingContractClient::new(env, contract_id)
}

/// Book a standard 60-minute session starting `lead` seconds after base.
fn book(f: &Fixture, session_byte: u8, start: u64, duration: u32) -> BytesN<32> {
    let c = client(&f.env, &f.contract_id);
    let session_id = hash(&f.env, session_byte);
    c.book_session(
        &f.mentor,
        &f.agreement,
        &session_id,
        &hash(&f.env, session_byte ^ 0x5A),
        &start,
        &0,
        &duration,
        &SessionMode::Video,
        &hash(&f.env, 0xEE),
        &hash(&f.env, 0xFE),
    );
    session_id
}

// ── initialization / configuration ─────────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_initialize(&f.admin, &LEAD),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_rejects_out_of_bounds_lead_time() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipSchedulingContract, ());
    let c = client(&env, &contract_id);
    let admin = Address::generate(&env);
    // 31 days is above MAX_LEAD_TIME_SECONDS.
    assert_eq!(
        c.try_initialize(&admin, &(31 * 86_400)),
        Err(Ok(ContractError::InvalidLeadTime))
    );
    // Uninitialized getters fail closed with a stable error.
    assert_eq!(c.try_registrar(), Err(Ok(ContractError::NotInitialized)));
}

#[test]
fn test_admin_actions_reject_non_admin() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let attacker = Address::generate(&f.env);
    assert_eq!(
        c.try_set_registrar(&attacker, &attacker),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_set_min_lead_time(&attacker, &7_200),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_upgrade(&attacker, &hash(&f.env, 0xB0)),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_admin_can_update_lead_time() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.set_min_lead_time(&f.admin, &7_200);
    assert_eq!(c.min_lead_time_seconds(), 7_200);
    // The new policy is enforced: a slot 1h out no longer qualifies.
    let start = BASE + 3_600 + 100;
    let result = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &start,
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::LeadTimeNotMet)));
}

#[test]
fn test_version_is_stable_for_upgrade_compatibility() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 7_200, 60);
    // The ABI generation reported to clients does not move with additive
    // entry points, so an upgrade keeps existing sessions readable.
    assert_eq!(c.version(), 1);
    assert_eq!(c.get_session(&sid).session_id, sid);
    // Upgrading remains admin-gated (the wasm swap itself is exercised by
    // the deploy scripts, not the unit host, which has no uploaded wasm).
    let attacker = Address::generate(&f.env);
    assert_eq!(
        c.try_upgrade(&attacker, &hash(&f.env, 0xC0)),
        Err(Ok(ContractError::NotAdmin))
    );
}

// ── agreement registration (deny-by-default registrar) ─────────────────────

#[test]
fn test_non_registrar_cannot_register_agreement() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_register_agreement(&outsider, &hash(&f.env, 0xB2), &f.mentor, &f.mentee),
        Err(Ok(ContractError::NotRegistrar))
    );
}

#[test]
fn test_register_agreement_rejects_duplicate() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_register_agreement(&f.registrar, &f.agreement, &f.mentor, &f.mentee),
        Err(Ok(ContractError::AgreementAlreadyRegistered))
    );
}

#[test]
fn test_register_agreement_rejects_self_pairing() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let alone = Address::generate(&f.env);
    assert_eq!(
        c.try_register_agreement(&f.registrar, &hash(&f.env, 0xB3), &alone, &alone),
        Err(Ok(ContractError::NotAuthorized))
    );
}

#[test]
fn test_registrar_role_can_be_moved() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let new_registrar = Address::generate(&f.env);
    c.set_registrar(&f.admin, &new_registrar);
    assert_eq!(c.registrar(), new_registrar);
    // The former registrar is locked out (deny-by-default).
    assert_eq!(
        c.try_register_agreement(&f.registrar, &hash(&f.env, 0xB4), &f.mentor, &f.mentee),
        Err(Ok(ContractError::NotRegistrar))
    );
}

#[test]
fn test_inactive_agreement_cannot_book() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.set_agreement_active(&f.registrar, &f.agreement, &false);
    let result = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::AgreementNotActive)));
}

// ── booking: permissions, lead time, bounds ────────────────────────────────

#[test]
fn test_book_session_success() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let session_id = hash(&f.env, 7);
    let start = BASE + 86_400;
    c.book_session(
        &f.mentee,
        &f.agreement,
        &session_id,
        &hash(&f.env, 8),
        &start,
        &120,
        &45,
        &SessionMode::InPerson,
        &hash(&f.env, 9),
        &hash(&f.env, 10),
    );

    // Exactly one canonical event, from this contract, keyed by the session.
    // Read immediately: `Events::all()` reports the most recent invocation.
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "booking emits one canonical event");
    let (publisher, topics, _) = events.get(0).unwrap();
    assert_eq!(publisher, f.contract_id);
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("BOOKED"));
    let topic_id = BytesN::<32>::try_from_val(&f.env, &topics.get(1).unwrap()).unwrap();
    assert_eq!(topic_id, session_id);

    let session = c.get_session(&session_id);
    assert_eq!(session.session_id, session_id);
    assert_eq!(session.agreement_id, f.agreement);
    assert_eq!(session.mentor, f.mentor);
    assert_eq!(session.mentee, f.mentee);
    assert_eq!(session.start_utc, start);
    assert_eq!(session.local_offset_minutes, 120);
    assert_eq!(session.duration_minutes, 45);
    assert_eq!(session.mode, SessionMode::InPerson);
    assert_eq!(session.status, SessionStatus::Booked);
    assert_eq!(session.booked_at, BASE);
    assert_eq!(session.mentor_confirmed_at, 0);
    assert_eq!(session.mentee_confirmed_at, 0);

    // Indexed under both participants.
    assert_eq!(c.participant_sessions(&f.mentor).len(), 1);
    assert_eq!(c.participant_sessions(&f.mentee).len(), 1);
}

#[test]
fn test_book_session_requires_participation() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let outsider = Address::generate(&f.env);
    let result = c.try_book_session(
        &outsider,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::NotAuthorized)));
}

#[test]
fn test_book_session_unknown_agreement() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let result = c.try_book_session(
        &f.mentor,
        &hash(&f.env, 0xDE),
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::AgreementNotFound)));
}

#[test]
fn test_book_session_start_in_past() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let result = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &BASE, // == now
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::StartTimeInPast)));
}

#[test]
fn test_book_session_lead_time_boundary() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // One second inside the lead window fails...
    let too_soon = BASE + u64::from(LEAD) - 1;
    let result = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &too_soon,
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::LeadTimeNotMet)));
    // ...and exactly the lead time succeeds.
    let exact = BASE + u64::from(LEAD);
    let booked = c.book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 5),
        &hash(&f.env, 6),
        &exact,
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(c.get_session(&booked).start_utc, exact);
}

#[test]
fn test_book_session_duration_bounds() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    for (byte, duration) in [(1u8, 14u32), (2, 481)] {
        let result = c.try_book_session(
            &f.mentor,
            &f.agreement,
            &hash(&f.env, byte),
            &hash(&f.env, byte + 10),
            &(BASE + 7_200 + u64::from(byte) * 10_000),
            &0,
            &duration,
            &SessionMode::Video,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        );
        assert_eq!(result, Err(Ok(ContractError::InvalidDuration)));
    }
    // The accepted edges book successfully.
    book(&f, 3, BASE + 90_000, 15);
    book(&f, 4, BASE + 190_000, 480);
}

#[test]
fn test_book_session_timezone_offset_bounds() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    for (byte, offset) in [(1u8, -721i32), (2, 841)] {
        let result = c.try_book_session(
            &f.mentor,
            &f.agreement,
            &hash(&f.env, byte),
            &hash(&f.env, byte + 10),
            &(BASE + 7_200 + u64::from(byte) * 10_000),
            &offset,
            &60,
            &SessionMode::Video,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        );
        assert_eq!(result, Err(Ok(ContractError::InvalidTimezoneOffset)));
    }
    book(&f, 3, BASE + 90_000, 60); // offset 0 is valid
}

#[test]
fn test_book_session_duplicate_id() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    book(&f, 1, BASE + 7_200, 60);
    let result = c.try_book_session(
        &f.mentee,
        &f.agreement,
        &hash(&f.env, 1), // taken
        &hash(&f.env, 2),
        &(BASE + 40_000),
        &0,
        &60,
        &SessionMode::Audio,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::SessionAlreadyExists)));
}

// ── booking: no double-booking ─────────────────────────────────────────────

#[test]
fn test_book_session_conflict_on_overlap() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // [BASE+7200, BASE+10800)
    book(&f, 1, BASE + 7_200, 60);

    // Partial overlap on the left, containment, and on the right all fail.
    for (byte, start, duration) in [
        (2u8, BASE + 5_400u64, 60u32), // ends inside the first session
        (3, BASE + 8_100, 15),         // contained in the first session
        (4, BASE + 9_000, 120),        // starts inside, ends after
    ] {
        let result = c.try_book_session(
            &f.mentor,
            &f.agreement,
            &hash(&f.env, byte),
            &hash(&f.env, byte + 10),
            &start,
            &0,
            &duration,
            &SessionMode::Video,
            &hash(&f.env, 3),
            &hash(&f.env, 4),
        );
        assert_eq!(result, Err(Ok(ContractError::ScheduleConflict)));
    }

    // Back-to-back (end == next start) is not a conflict.
    book(&f, 5, BASE + 10_800, 60);
    assert_eq!(c.participant_sessions(&f.mentor).len(), 2);
}

#[test]
fn test_book_session_conflict_through_either_participant() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // The mentor is already busy in a session with a different mentee.
    let other_mentee = Address::generate(&f.env);
    let other_agreement = hash(&f.env, 0xA2);
    c.register_agreement(&f.registrar, &other_agreement, &f.mentor, &other_mentee);
    c.book_session(
        &f.mentor,
        &other_agreement,
        &hash(&f.env, 1),
        &hash(&f.env, 2),
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );

    // Booking the same window under the original agreement conflicts via the
    // mentor's index even though the mentee is free.
    let result = c.try_book_session(
        &f.mentee,
        &f.agreement,
        &hash(&f.env, 5),
        &hash(&f.env, 6),
        &(BASE + 9_000),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::ScheduleConflict)));

    // A free slot succeeds and both participants see it.
    book(&f, 7, BASE + 20_000, 60);
    assert_eq!(c.participant_sessions(&f.mentor).len(), 2);
    assert_eq!(c.participant_sessions(&other_mentee).len(), 1);
}

// ── booking: safe retries (idempotency) ────────────────────────────────────

#[test]
fn test_book_session_retry_is_idempotent() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let idem = hash(&f.env, 0x11);
    let first = c.book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &idem,
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    // Retry with the same session id, key, and payload: same id back, and
    // the per-participant index does not grow.
    let second = c.book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 1),
        &idem,
        &(BASE + 7_200),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(first, second);
    assert_eq!(c.participant_sessions(&f.mentor).len(), 1);
    assert_eq!(c.participant_sessions(&f.mentee).len(), 1);

    // Reusing the key for a different payload is a stable failure, not a
    // silent rebooking.
    let conflict = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 2),
        &idem,
        &(BASE + 9_000),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(conflict, Err(Ok(ContractError::IdempotencyConflict)));
}

#[test]
fn test_book_session_storage_bound() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Fill one participant's index to capacity with back-to-back sessions.
    for i in 0..64u64 {
        book(&f, i as u8, BASE + 7_200 + i * 3_700, 60);
    }
    assert_eq!(c.participant_sessions(&f.mentor).len(), 64);
    // The next booking is refused rather than growing the index.
    let result = c.try_book_session(
        &f.mentor,
        &f.agreement,
        &hash(&f.env, 200),
        &hash(&f.env, 201),
        &(BASE + 7_200 + 64 * 3_700),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    assert_eq!(result, Err(Ok(ContractError::SessionLimitReached)));
}

// ── confirmations: local time plus UTC ─────────────────────────────────────

#[test]
fn test_confirm_session_flow() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 7_200, 60);

    // A non-participant cannot confirm.
    let outsider = Address::generate(&f.env);
    assert_eq!(
        c.try_confirm_session(&outsider, &sid),
        Err(Ok(ContractError::NotAuthorized))
    );

    // First confirmation is partial; the state machine stays Booked.
    assert!(!c.confirm_session(&f.mentor, &sid));
    let session = c.get_session(&sid);
    assert_eq!(session.status, SessionStatus::Booked);
    assert_eq!(session.mentor_confirmed_at, BASE);
    assert_eq!(session.mentee_confirmed_at, 0);

    // A repeated confirmation by the same participant changes nothing.
    assert!(!c.confirm_session(&f.mentor, &sid));
    assert_eq!(c.get_session(&sid).mentor_confirmed_at, BASE);

    // The second participant's confirmation completes the session.
    assert!(c.confirm_session(&f.mentee, &sid));
    let session = c.get_session(&sid);
    assert_eq!(session.status, SessionStatus::Confirmed);
    assert_eq!(session.mentee_confirmed_at, BASE);
}

#[test]
fn test_confirm_session_unknown_id() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_confirm_session(&f.mentor, &hash(&f.env, 0xFF)),
        Err(Ok(ContractError::SessionNotFound))
    );
}

#[test]
fn test_session_times_render_local_and_utc() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = hash(&f.env, 1);
    let start = BASE + 7_200;
    c.book_session(
        &f.mentor,
        &f.agreement,
        &sid,
        &hash(&f.env, 2),
        &start,
        &120, // UTC+2
        &90,
        &SessionMode::Video,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    let times = c.session_times(&sid);
    assert_eq!(times.start_utc, start);
    assert_eq!(times.end_utc, start + 5_400);
    assert_eq!(times.local_offset_minutes, 120);
    assert_eq!(times.start_local, start + 7_200);
    assert_eq!(times.end_local, start + 5_400 + 7_200);

    // West-of-UTC offsets subtract without underflow.
    let sid_west = hash(&f.env, 5);
    c.book_session(
        &f.mentee,
        &f.agreement,
        &sid_west,
        &hash(&f.env, 6),
        &(BASE + 90_000),
        &-300, // UTC-5
        &60,
        &SessionMode::Audio,
        &hash(&f.env, 3),
        &hash(&f.env, 4),
    );
    let west = c.session_times(&sid_west);
    assert_eq!(west.start_local, BASE + 90_000 - 18_000);
}

// ── conflict view ──────────────────────────────────────────────────────────

#[test]
fn test_has_conflict_view() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 7_200, 60);
    assert!(c.has_conflict(&f.mentor, &(BASE + 9_000), &30));
    assert!(c.has_conflict(&f.mentee, &(BASE + 7_200), &60));
    assert!(!c.has_conflict(&f.mentor, &(BASE + 10_800), &60));
    // A participant with no sessions never conflicts.
    assert!(!c.has_conflict(&Address::generate(&f.env), &(BASE + 9_000), &30));
    let _ = sid;
}

#[test]
fn test_get_session_unknown_id_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    assert_eq!(
        c.try_get_session(&hash(&f.env, 0x99)),
        Err(Ok(ContractError::SessionNotFound))
    );
    assert_eq!(
        c.try_get_agreement(&hash(&f.env, 0x99)),
        Err(Ok(ContractError::AgreementNotFound))
    );
}

// ── #1253 reschedule: propose / approve / decline ──────────────────────────

use crate::{DataKey, RescheduleReason, RescheduleStatus};

/// A session far enough out that the default 24h notice policy permits a
/// change.
const FAR: u64 = 300_000;

fn propose(
    f: &Fixture,
    session_id: &BytesN<32>,
    proposal_byte: u8,
    start: u64,
    duration: u32,
) -> BytesN<32> {
    let c = client(&f.env, &f.contract_id);
    let proposal_id = hash(&f.env, proposal_byte);
    c.propose_reschedule(
        &f.mentor,
        session_id,
        &proposal_id,
        &RescheduleReason::AvailabilityChange,
        &hash(&f.env, proposal_byte ^ 0x33),
        &start,
        &0,
        &duration,
    );
    proposal_id
}

#[test]
fn test_reschedule_flow_applies_and_keeps_history() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);
    c.confirm_session(&f.mentor, &sid);

    let proposal_id = propose(&f, &sid, 0x40, BASE + FAR + 7_200, 90);

    // One canonical proposal event, keyed by the proposal, carrying the
    // public reason code.
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "proposal emits one event");
    let (publisher, topics, _) = events.get(0).unwrap();
    assert_eq!(publisher, f.contract_id);
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RSPROP"));
    let proposal = c.get_reschedule(&proposal_id);
    assert_eq!(proposal.status, RescheduleStatus::Proposed);
    assert_eq!(proposal.proposed_by, f.mentor);
    assert_eq!(proposal.session_revision, 0);
    assert_eq!(proposal.new_start_utc, BASE + FAR + 7_200);
    assert_eq!(proposal.new_duration_minutes, 90);

    // Payment consequences are previewed at proposal time.
    assert_eq!(proposal.payment_impact.current_minutes, 60);
    assert_eq!(proposal.payment_impact.proposed_minutes, 90);
    assert_eq!(proposal.payment_impact.delta_minutes, 30);
    assert!(proposal.payment_impact.settlement_adjustment_required);

    // The counterparty approves; the session moves and both confirmations
    // are reset.
    let session = c.approve_reschedule(&f.mentee, &proposal_id);
    assert_eq!(session.start_utc, BASE + FAR + 7_200);
    assert_eq!(session.duration_minutes, 90);
    assert_eq!(session.revision, 1);
    assert_eq!(session.status, SessionStatus::Booked);
    assert_eq!(session.mentor_confirmed_at, 0);
    assert_eq!(session.mentee_confirmed_at, 0);

    // The apply emits exactly one RESCHED event (read before any further
    // invocation, since `Events::all()` only reports the most recent one).
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "apply emits one event");
    let (_, topics, _) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("RESCHED"));
    assert_eq!(
        c.get_reschedule(&proposal_id).status,
        RescheduleStatus::Applied
    );

    // The original time remains traceable.
    let history = c.session_reschedules(&sid);
    assert_eq!(history.len(), 1);
    let change = history.get(0).unwrap();
    assert_eq!(change.proposal_id, proposal_id);
    assert_eq!(change.previous_start_utc, BASE + FAR);
    assert_eq!(change.previous_duration_minutes, 60);
    assert_eq!(change.new_start_utc, BASE + FAR + 7_200);
    assert_eq!(change.new_duration_minutes, 90);
    assert_eq!(change.changed_at, BASE);

    // Calendar views converge on the new slot; the session id and the
    // participant indexes do not change.
    let times = c.session_times(&sid);
    assert_eq!(times.start_utc, BASE + FAR + 7_200);
    assert_eq!(times.end_utc, BASE + FAR + 7_200 + 5_400);
    assert_eq!(c.participant_sessions(&f.mentor).len(), 1);
    assert_eq!(c.participant_sessions(&f.mentee).len(), 1);
}

#[test]
fn test_reschedule_requires_participant_and_counterparty_consent() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);
    let outsider = Address::generate(&f.env);

    // A non-participant cannot propose...
    assert_eq!(
        c.try_propose_reschedule(
            &outsider,
            &sid,
            &hash(&f.env, 0x41),
            &RescheduleReason::Personal,
            &hash(&f.env, 0x42),
            &(BASE + FAR + 7_200),
            &0,
            &60,
        ),
        Err(Ok(ContractError::NotAuthorized))
    );

    let proposal_id = propose(&f, &sid, 0x43, BASE + FAR + 7_200, 60);
    // ...and neither approval nor decline is open to outsiders.
    assert_eq!(
        c.try_approve_reschedule(&outsider, &proposal_id),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_decline_reschedule(&outsider, &proposal_id),
        Err(Ok(ContractError::NotAuthorized))
    );
    // The proposing party cannot approve or decline its own change: consent
    // must come from the counterparty.
    assert_eq!(
        c.try_approve_reschedule(&f.mentor, &proposal_id),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_decline_reschedule(&f.mentor, &proposal_id),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(c.get_session(&sid).start_utc, BASE + FAR);
}

#[test]
fn test_reschedule_notice_policy_is_enforced_and_configurable() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    // Booked just two hours out: inside the default 24h notice window.
    let sid = book(&f, 1, BASE + 7_200, 60);
    assert_eq!(
        c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &hash(&f.env, 0x44),
            &RescheduleReason::AvailabilityChange,
            &hash(&f.env, 0x45),
            &(BASE + 100_000),
            &0,
            &60,
        ),
        Err(Ok(ContractError::RescheduleNoticeNotMet))
    );

    // Only the admin can change the policy, and only within bounds.
    let attacker = Address::generate(&f.env);
    assert_eq!(
        c.try_set_reschedule_policy(&attacker, &0, &3_600),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        c.try_set_reschedule_policy(&f.admin, &0, &59),
        Err(Ok(ContractError::InvalidReschedulePolicy))
    );
    c.set_reschedule_policy(&f.admin, &0, &3_600);
    assert_eq!(c.reschedule_notice_seconds(), 0);
    assert_eq!(c.approval_window_seconds(), 3_600);

    // With a zero notice period the change can be proposed.
    propose(&f, &sid, 0x46, BASE + 100_000, 60);
}

#[test]
fn test_reschedule_decline_leaves_session_untouched() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);
    let proposal_id = propose(&f, &sid, 0x47, BASE + FAR + 7_200, 60);

    c.decline_reschedule(&f.mentee, &proposal_id);
    let events = f.env.events().all();
    assert_eq!(events.len(), 1, "declining emits one event");
    assert_eq!(
        c.get_reschedule(&proposal_id).status,
        RescheduleStatus::Declined
    );
    assert_eq!(c.get_session(&sid).start_utc, BASE + FAR);
    assert_eq!(c.get_session(&sid).revision, 0);
    assert!(c.session_reschedules(&sid).is_empty());

    // Declining twice is a no-op.
    c.decline_reschedule(&f.mentee, &proposal_id);
    // An applied or declined proposal cannot be approved.
    assert_eq!(
        c.try_approve_reschedule(&f.mentee, &proposal_id),
        Err(Ok(ContractError::RescheduleNotOpen))
    );
    // A declined proposal frees the session for a new proposal.
    propose(&f, &sid, 0x48, BASE + FAR + 7_200, 60);
}

#[test]
fn test_reschedule_proposal_is_idempotent_and_single_open() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);
    let proposal_id = propose(&f, &sid, 0x49, BASE + FAR + 7_200, 60);

    // Replaying the same proposal is safe and does not duplicate state.
    let replay = c.propose_reschedule(
        &f.mentor,
        &sid,
        &proposal_id,
        &RescheduleReason::AvailabilityChange,
        &hash(&f.env, 0x49 ^ 0x33),
        &(BASE + FAR + 7_200),
        &0,
        &60,
    );
    assert_eq!(replay.proposal_id, proposal_id);
    assert_eq!(replay.created_at, BASE);

    // Reusing the proposal id for a different payload fails loudly.
    assert_eq!(
        c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &proposal_id,
            &RescheduleReason::Personal,
            &hash(&f.env, 0x4A),
            &(BASE + FAR + 7_200),
            &0,
            &60,
        ),
        Err(Ok(ContractError::IdempotencyConflict))
    );

    // Only one open proposal per session.
    assert_eq!(
        c.try_propose_reschedule(
            &f.mentee,
            &sid,
            &hash(&f.env, 0x4B),
            &RescheduleReason::Personal,
            &hash(&f.env, 0x4C),
            &(BASE + FAR + 9_000),
            &0,
            &60,
        ),
        Err(Ok(ContractError::RescheduleAlreadyPending))
    );

    // Approving twice returns the same session without a duplicate trace.
    let first = c.approve_reschedule(&f.mentee, &proposal_id);
    let second = c.approve_reschedule(&f.mentee, &proposal_id);
    assert_eq!(first.start_utc, second.start_utc);
    assert_eq!(c.session_reschedules(&sid).len(), 1);
    assert_eq!(c.get_session(&sid).revision, 1);
}

#[test]
fn test_reschedule_expiry_blocks_stale_approval() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    c.set_reschedule_policy(&f.admin, &86_400, &60);
    let sid = book(&f, 1, BASE + FAR, 60);
    let proposal_id = propose(&f, &sid, 0x4D, BASE + FAR + 7_200, 60);
    assert_eq!(c.get_reschedule(&proposal_id).expires_at, BASE + 60);

    f.env.ledger().set_timestamp(BASE + 61);
    assert_eq!(
        c.try_approve_reschedule(&f.mentee, &proposal_id),
        Err(Ok(ContractError::RescheduleExpired))
    );
    assert_eq!(c.get_session(&sid).start_utc, BASE + FAR);

    // The expired proposal no longer blocks a fresh one.
    propose(&f, &sid, 0x4E, BASE + FAR + 7_200, 60);
}

#[test]
fn test_stale_reschedule_approval_fails() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);
    let proposal_id = propose(&f, &sid, 0x4F, BASE + FAR + 7_200, 60);

    // Simulate the session having changed after the proposal (e.g. a future
    // reschedule path): bump the revision in contract storage.
    f.env.as_contract(&f.contract_id, || {
        let key = DataKey::Session(sid.clone());
        let mut session: crate::Session = f.env.storage().persistent().get(&key).unwrap();
        session.revision = 7;
        f.env.storage().persistent().set(&key, &session);
    });

    assert_eq!(
        c.try_approve_reschedule(&f.mentee, &proposal_id),
        Err(Ok(ContractError::StaleRescheduleApproval))
    );
}

#[test]
fn test_reschedule_rechecks_conflicts_at_apply_time() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 100_000, 60);
    let proposal_id = propose(&f, &sid, 0x50, BASE + FAR, 60);

    // Another agreement books the mentor into the proposed slot after the
    // proposal was made but before it is approved.
    let other_mentee = Address::generate(&f.env);
    let other_agreement = hash(&f.env, 0xA3);
    c.register_agreement(&f.registrar, &other_agreement, &f.mentor, &other_mentee);
    c.book_session(
        &f.mentor,
        &other_agreement,
        &hash(&f.env, 0x51),
        &hash(&f.env, 0x52),
        &(BASE + FAR),
        &0,
        &60,
        &SessionMode::Video,
        &hash(&f.env, 0x53),
        &hash(&f.env, 0x54),
    );

    assert_eq!(
        c.try_approve_reschedule(&f.mentee, &proposal_id),
        Err(Ok(ContractError::ScheduleConflict))
    );
    assert_eq!(c.get_session(&sid).start_utc, BASE + 100_000);
}

#[test]
fn test_reschedule_rejects_conflict_at_proposal_time() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 100_000, 60);
    book(&f, 2, BASE + FAR, 60);

    assert_eq!(
        c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &hash(&f.env, 0x55),
            &RescheduleReason::ScheduleConflict,
            &hash(&f.env, 0x56),
            &(BASE + FAR + 60),
            &0,
            &60,
        ),
        Err(Ok(ContractError::ScheduleConflict))
    );
}

#[test]
fn test_reschedule_validates_slot_and_bounds() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);

    // Same slot is not a change.
    assert_eq!(
        c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &hash(&f.env, 0x57),
            &RescheduleReason::Other,
            &hash(&f.env, 0x58),
            &(BASE + FAR),
            &0,
            &60,
        ),
        Err(Ok(ContractError::InvalidReschedule))
    );
    // Duration and offset bounds still apply.
    for (byte, offset, duration) in [(0x59u8, 0i32, 14u32), (0x5A, 0, 481), (0x5B, 841, 60)] {
        let result = c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &hash(&f.env, byte),
            &RescheduleReason::Other,
            &hash(&f.env, 0x5C),
            &(BASE + FAR + 7_200),
            &offset,
            &duration,
        );
        assert!(
            result == Err(Ok(ContractError::InvalidDuration))
                || result == Err(Ok(ContractError::InvalidTimezoneOffset))
        );
    }
}

#[test]
fn test_reschedule_history_is_bounded() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + 100_000, 60);

    for i in 0..16u8 {
        let proposal_id = propose(
            &f,
            &sid,
            0x60 + i,
            BASE + 100_000 + u64::from(i + 1) * 3_700,
            60,
        );
        c.approve_reschedule(&f.mentee, &proposal_id);
    }
    assert_eq!(c.session_reschedules(&sid).len(), 16);
    assert_eq!(c.get_session(&sid).revision, 16);

    // The 17th change is refused rather than growing the trace.
    assert_eq!(
        c.try_propose_reschedule(
            &f.mentor,
            &sid,
            &hash(&f.env, 0x7F),
            &RescheduleReason::Other,
            &hash(&f.env, 0x7E),
            &(BASE + 200_000),
            &0,
            &60,
        ),
        Err(Ok(ContractError::RescheduleLimitReached))
    );
}

#[test]
fn test_reschedule_payment_impact_preview() {
    let f = fixture();
    let c = client(&f.env, &f.contract_id);
    let sid = book(&f, 1, BASE + FAR, 60);

    let shorter = c.reschedule_payment_impact(&sid, &30);
    assert_eq!(shorter.delta_minutes, -30);
    assert!(shorter.settlement_adjustment_required);
    let same = c.reschedule_payment_impact(&sid, &60);
    assert_eq!(same.delta_minutes, 0);
    assert!(!same.settlement_adjustment_required);
    assert_eq!(
        c.try_reschedule_payment_impact(&sid, &14),
        Err(Ok(ContractError::InvalidDuration))
    );
    assert_eq!(
        c.try_reschedule_payment_impact(&hash(&f.env, 0x99), &60),
        Err(Ok(ContractError::SessionNotFound))
    );
    assert_eq!(
        c.try_get_reschedule(&hash(&f.env, 0x98)),
        Err(Ok(ContractError::RescheduleNotFound))
    );
}
