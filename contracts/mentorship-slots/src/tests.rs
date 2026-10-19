use crate::{BookingState, ContractError, MentorshipSlotsContract, MentorshipSlotsContractClient};
use mentorship_availability::{
    MentorshipAvailabilityContract, MentorshipAvailabilityContractClient, ScheduleInput,
    WeeklyWindow,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

/// 2026-01-05T00:00:00Z — a Monday.
const MONDAY: u64 = 1_767_571_200;
const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Setup {
    env: Env,
    slots_id: Address,
    availability_id: Address,
    admin: Address,
    program_id: BytesN<32>,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(MONDAY - 7 * DAY);
    let availability_id = env.register(MentorshipAvailabilityContract, ());
    let slots_id = env.register(MentorshipSlotsContract, ());
    let admin = Address::generate(&env);
    let program_id = hash(&env, 1);
    MentorshipSlotsContractClient::new(&env, &slots_id).initialize(&admin, &availability_id);
    Setup {
        env,
        slots_id,
        availability_id,
        admin,
        program_id,
    }
}

fn slots<'a>(env: &'a Env, id: &Address) -> MentorshipSlotsContractClient<'a> {
    MentorshipSlotsContractClient::new(env, id)
}

fn availability<'a>(env: &'a Env, id: &Address) -> MentorshipAvailabilityContractClient<'a> {
    MentorshipAvailabilityContractClient::new(env, id)
}

fn windows(env: &Env, specs: &[(u32, u32, u32)]) -> Vec<WeeklyWindow> {
    let mut out = Vec::new(env);
    for (weekday, start, end) in specs.iter() {
        out.push_back(WeeklyWindow {
            weekday: *weekday,
            start_minute: *start,
            end_minute: *end,
        });
    }
    out
}

fn schedule(env: &Env, weekly: Vec<WeeklyWindow>) -> ScheduleInput {
    ScheduleInput {
        timezone_hash: hash(env, 2),
        utc_offset_seconds: 0,
        offset_changes: Vec::new(env),
        weekly,
        lead_time_seconds: 0,
        buffer_before_seconds: 0,
        buffer_after_seconds: 0,
        booking_horizon_days: 365,
        effective_from: 1,
        effective_until: 0,
    }
}

fn set_availability(s: &Setup, participant: &Address, spec: ScheduleInput) {
    availability(&s.env, &s.availability_id).set_schedule(participant, &spec);
}

/// Both participants free Monday 09:00-11:00 and Tuesday 09:00-11:00.
fn set_common_week(s: &Setup, mentor: &Address, mentee: &Address) {
    let spec = || schedule(&s.env, windows(&s.env, &[(1, 540, 660), (2, 540, 660)]));
    set_availability(s, mentor, spec());
    set_availability(s, mentee, spec());
}

fn set_policy(s: &Setup, duration: u64, granularity: u64, per_day: u32) {
    slots(&s.env, &s.slots_id).set_program_policy(
        &s.admin,
        &s.program_id,
        &duration,
        &granularity,
        &per_day,
    );
}

// ── authorization and policy configuration ─────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let s = setup();
    let c = slots(&s.env, &s.slots_id);
    assert_eq!(
        c.try_initialize(&s.admin, &s.availability_id),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_policy_is_admin_only() {
    let s = setup();
    let attacker = Address::generate(&s.env);
    assert_eq!(
        slots(&s.env, &s.slots_id).try_set_program_policy(
            &attacker,
            &s.program_id,
            &1_800u64,
            &900u64,
            &2u32,
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_policy_validation_and_versioning() {
    let s = setup();
    let c = slots(&s.env, &s.slots_id);

    assert_eq!(
        c.try_set_program_policy(&s.admin, &s.program_id, &120u64, &60u64, &2u32),
        Err(Ok(ContractError::InvalidSessionPolicy))
    );
    assert_eq!(
        c.try_set_program_policy(&s.admin, &s.program_id, &1_800u64, &30u64, &2u32),
        Err(Ok(ContractError::InvalidSessionPolicy))
    );
    assert_eq!(
        c.try_set_program_policy(&s.admin, &s.program_id, &1_801u64, &900u64, &2u32),
        Err(Ok(ContractError::InvalidSessionPolicy))
    );
    assert_eq!(
        c.try_set_program_policy(&s.admin, &s.program_id, &1_800u64, &900u64, &0u32),
        Err(Ok(ContractError::InvalidSessionsPerDay))
    );

    c.set_program_policy(&s.admin, &s.program_id, &1_800u64, &900u64, &2u32);
    assert_eq!(c.get_program_policy(&s.program_id).version, 1);
    c.set_program_policy(&s.admin, &s.program_id, &3_600u64, &1_800u64, &3u32);
    let policy = c.get_program_policy(&s.program_id);
    assert_eq!(policy.version, 2);
    assert_eq!(policy.duration_seconds, 3_600);
    assert_eq!(policy.max_sessions_per_day, 3);
    assert_eq!(
        c.try_get_program_policy(&hash(&s.env, 9)),
        Err(Ok(ContractError::ProgramPolicyNotFound))
    );
}

#[test]
fn test_blackout_management() {
    let s = setup();
    let c = slots(&s.env, &s.slots_id);

    c.set_blackout(
        &s.admin,
        &hash(&s.env, 3),
        &s.program_id,
        &MONDAY,
        &(MONDAY + 3 * HOUR),
    );
    assert_eq!(c.blackout_ids().len(), 1);
    let blackout = c.get_blackout(&hash(&s.env, 3));
    assert_eq!(blackout.from_ts, MONDAY);
    assert_eq!(blackout.until_ts, MONDAY + 3 * HOUR);

    assert_eq!(
        c.try_set_blackout(
            &s.admin,
            &hash(&s.env, 3),
            &s.program_id,
            &MONDAY,
            &(MONDAY + 3 * HOUR),
        ),
        Err(Ok(ContractError::BlackoutAlreadyExists))
    );
    assert_eq!(
        c.try_set_blackout(&s.admin, &hash(&s.env, 4), &s.program_id, &MONDAY, &MONDAY,),
        Err(Ok(ContractError::InvalidBlackoutRange))
    );

    c.remove_blackout(&s.admin, &hash(&s.env, 3));
    assert_eq!(c.blackout_ids().len(), 0);
    assert_eq!(
        c.try_get_blackout(&hash(&s.env, 3)),
        Err(Ok(ContractError::BlackoutNotFound))
    );
}

// ── candidate computation ──────────────────────────────────────────────────

#[test]
fn test_find_slots_intersects_windows_on_the_grid() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_availability(
        &s,
        &mentor,
        schedule(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    set_availability(
        &s,
        &mentee,
        schedule(&s.env, windows(&s.env, &[(1, 600, 720)])),
    );
    set_policy(&s, 1_800, 900, 4);

    let c = slots(&s.env, &s.slots_id);
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(found.len(), 3);
    assert_eq!(found.get(0).unwrap().start_ts, MONDAY + 10 * HOUR);
    assert_eq!(found.get(0).unwrap().end_ts, MONDAY + 10 * HOUR + 1_800);
    assert_eq!(found.get(1).unwrap().start_ts, MONDAY + 10 * HOUR + 900);
    assert_eq!(found.get(2).unwrap().start_ts, MONDAY + 10 * HOUR + 1_800);
    assert_eq!(found.get(0).unwrap().mentor_availability_version, 1);
    assert_eq!(found.get(0).unwrap().mentee_availability_version, 1);

    // Deterministic: a repeat call returns the same slots.
    let again = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(again, found);

    // The echoed availability version is the one the ranges came from, so a
    // client can tell its candidate list is stale after a schedule change.
    set_availability(
        &s,
        &mentor,
        schedule(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    let refreshed = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(refreshed.get(0).unwrap().mentor_availability_version, 2);
    assert_eq!(refreshed.get(0).unwrap().mentee_availability_version, 1);
}

#[test]
fn test_find_slots_aligns_to_the_epoch_grid() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_availability(
        &s,
        &mentor,
        schedule(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    set_availability(
        &s,
        &mentee,
        schedule(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    set_policy(&s, 1_800, 900, 8);

    // Starting at an off-grid instant does not shift the grid.
    let found = slots(&s.env, &s.slots_id).find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR + 7 * 60),
        &(MONDAY + 9 * HOUR + 50 * 60),
        &0,
        &10,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found.get(0).unwrap().start_ts, MONDAY + 9 * HOUR + 15 * 60);
    assert_eq!(found.get(0).unwrap().end_ts, MONDAY + 9 * HOUR + 45 * 60);
}

#[test]
fn test_find_slots_paginates() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 8);

    let c = slots(&s.env, &s.slots_id);
    let first = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &2,
    );
    assert_eq!(first.len(), 2);
    let second = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &1,
        &2,
    );
    assert_eq!(second.len(), 2);
    assert_eq!(second.get(0).unwrap().start_ts, MONDAY + 9 * HOUR + 1_800);
    // Candidates are alternatives and may overlap one another; pages are
    // ordered but not disjoint.
    assert!(first.get(1).unwrap().start_ts < second.get(0).unwrap().start_ts);
    // 7 candidates exist for the two-hour window (09:00..10:30 starts), so
    // the fourth page holds the last one and the fifth is empty.
    let last = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &3,
        &2,
    );
    assert_eq!(last.len(), 1);
    assert_eq!(last.get(0).unwrap().start_ts, MONDAY + 10 * HOUR + 1_800);
    let beyond = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &4,
        &2,
    );
    assert_eq!(beyond.len(), 0);
}

#[test]
fn test_find_slots_validation() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);

    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentor,
            &s.program_id,
            &MONDAY,
            &(MONDAY + DAY),
            &0,
            &10,
        ),
        Err(Ok(ContractError::InvalidParties))
    );
    assert_eq!(
        c.try_find_slots(&mentor, &mentee, &s.program_id, &MONDAY, &MONDAY, &0, &10,),
        Err(Ok(ContractError::InvalidQueryRange))
    );
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentee,
            &s.program_id,
            &MONDAY,
            &(MONDAY + 32 * DAY),
            &0,
            &10,
        ),
        Err(Ok(ContractError::QuerySpanTooLarge))
    );
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentee,
            &s.program_id,
            &MONDAY,
            &(MONDAY + DAY),
            &0,
            &0,
        ),
        Err(Ok(ContractError::InvalidPage))
    );
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentee,
            &s.program_id,
            &MONDAY,
            &(MONDAY + DAY),
            &5,
            &64,
        ),
        Err(Ok(ContractError::InvalidPage))
    );
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentee,
            &hash(&s.env, 42),
            &MONDAY,
            &(MONDAY + DAY),
            &0,
            &10,
        ),
        Err(Ok(ContractError::ProgramPolicyNotFound))
    );

    // A participant without a schedule makes availability unreadable.
    let stranger = Address::generate(&s.env);
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &stranger,
            &s.program_id,
            &MONDAY,
            &(MONDAY + DAY),
            &0,
            &10,
        ),
        Err(Ok(ContractError::AvailabilityUnavailable))
    );
}

#[test]
fn test_find_slots_respects_lead_time_and_horizon() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_policy(&s, 1_800, 900, 4);

    let mut spec = schedule(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.lead_time_seconds = 8 * DAY;
    set_availability(&s, &mentor, spec.clone());
    set_availability(&s, &mentee, spec);
    let c = slots(&s.env, &s.slots_id);
    // now is a week before MONDAY, so an 8-day lead time excludes it.
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(found.len(), 0);

    // A 1-day horizon queried two days early excludes it as well.
    s.env.ledger().set_timestamp(MONDAY - 2 * DAY);
    let mut spec = schedule(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.booking_horizon_days = 1;
    set_availability(&s, &mentor, spec.clone());
    set_availability(&s, &mentee, spec);
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(found.len(), 0);
}

#[test]
fn test_blackout_filters_candidates() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 8);
    let c = slots(&s.env, &s.slots_id);

    c.set_blackout(
        &s.admin,
        &hash(&s.env, 3),
        &s.program_id,
        &(MONDAY + 10 * HOUR),
        &(MONDAY + 10 * HOUR + 1_800),
    );

    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
        &(MONDAY + 11 * HOUR),
        &0,
        &10,
    );
    // Starts at 09:00, 09:15, 09:30, 09:45, 10:00, 10:15 and 10:30; the
    // blackout removes 09:45 (ends at 10:15), 10:00 and 10:15.
    assert_eq!(found.len(), 4);
    assert_eq!(found.get(0).unwrap().start_ts, MONDAY + 9 * HOUR);
    assert_eq!(found.get(1).unwrap().start_ts, MONDAY + 9 * HOUR + 900);
    assert_eq!(found.get(2).unwrap().start_ts, MONDAY + 9 * HOUR + 1_800);
    assert_eq!(found.get(3).unwrap().start_ts, MONDAY + 10 * HOUR + 1_800);

    // A blackout for another program does not apply.
    c.set_blackout(
        &s.admin,
        &hash(&s.env, 4),
        &hash(&s.env, 77),
        &(MONDAY + 9 * HOUR),
        &(MONDAY + 11 * HOUR),
    );
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
        &(MONDAY + 11 * HOUR),
        &0,
        &10,
    );
    assert_eq!(found.len(), 4);

    // A global blackout (zero program id) applies everywhere.
    c.set_blackout(
        &s.admin,
        &hash(&s.env, 5),
        &BytesN::from_array(&s.env, &[0u8; 32]),
        &(MONDAY + 9 * HOUR),
        &(MONDAY + 11 * HOUR),
    );
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
        &(MONDAY + 11 * HOUR),
        &0,
        &10,
    );
    assert_eq!(found.len(), 0);
}

// ── bookings, capacity, and buffers ────────────────────────────────────────

#[test]
fn test_book_slot_is_idempotent_and_revalidated() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);

    let start = MONDAY + 9 * HOUR;
    let end = c.book_slot(
        &mentor,
        &hash(&s.env, 10),
        &mentor,
        &mentee,
        &s.program_id,
        &start,
    );
    assert_eq!(end, start + 1_800);
    let booking = c.get_booking(&hash(&s.env, 10));
    assert_eq!(booking.state, BookingState::Active);
    assert_eq!(booking.end_ts, start + 1_800);
    assert_eq!(c.booking_ids(&mentor, &0, &10).len(), 1);
    assert_eq!(c.booking_ids(&mentee, &0, &10).len(), 1);

    // Re-submitting the identical booking is a no-op that returns the end.
    let retried = c.book_slot(
        &mentee,
        &hash(&s.env, 10),
        &mentor,
        &mentee,
        &s.program_id,
        &start,
    );
    assert_eq!(retried, start + 1_800);
    assert_eq!(c.booking_ids(&mentor, &0, &10).len(), 1);

    // The same id for a different slot is a conflict.
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 10),
            &mentor,
            &mentee,
            &s.program_id,
            &(start + 1_800),
        ),
        Err(Ok(ContractError::BookingAlreadyExists))
    );

    // A stale candidate is rejected once a booking occupies the time.
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 11),
            &mentor,
            &mentee,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::SlotUnavailable))
    );
}

#[test]
fn test_booking_validation_and_authorization() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);
    let start = MONDAY + 9 * HOUR;

    let attacker = Address::generate(&s.env);
    assert_eq!(
        c.try_book_slot(
            &attacker,
            &hash(&s.env, 12),
            &mentor,
            &mentee,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &BytesN::from_array(&s.env, &[0u8; 32]),
            &mentor,
            &mentee,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::InvalidBookingId))
    );
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 13),
            &mentor,
            &mentor,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::InvalidParties))
    );
    // Outside the available window.
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 14),
            &mentor,
            &mentee,
            &s.program_id,
            &(MONDAY + 15 * HOUR),
        ),
        Err(Ok(ContractError::SlotUnavailable))
    );
    // A blackout is reported as its own stable error.
    c.set_blackout(
        &s.admin,
        &hash(&s.env, 15),
        &s.program_id,
        &start,
        &(start + HOUR),
    );
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 16),
            &mentor,
            &mentee,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::BlackoutOverlap))
    );
}

#[test]
fn test_capacity_per_day_is_enforced() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 1);
    let c = slots(&s.env, &s.slots_id);

    c.book_slot(
        &mentor,
        &hash(&s.env, 20),
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
    );

    // The daily capacity is exhausted for both participants.
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(found.len(), 0);
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 21),
            &mentor,
            &mentee,
            &s.program_id,
            &(MONDAY + 9 * HOUR + 1_800),
        ),
        Err(Ok(ContractError::SlotUnavailable))
    );

    // Cancelling frees the capacity again.
    c.cancel_booking(&mentee, &hash(&s.env, 20));
    assert_eq!(
        c.get_booking(&hash(&s.env, 20)).state,
        BookingState::Cancelled
    );
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert!(!found.is_empty());
}

#[test]
fn test_buffers_are_respected_around_bookings() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_policy(&s, 1_800, 900, 8);

    let mut spec = schedule(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.buffer_after_seconds = HOUR;
    set_availability(&s, &mentor, spec);
    set_availability(
        &s,
        &mentee,
        schedule(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    let c = slots(&s.env, &s.slots_id);

    c.book_slot(
        &mentor,
        &hash(&s.env, 30),
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
    );

    // The mentor's 1-hour buffer pushes the next candidate to 10:30.
    let found = c.find_slots(
        &mentor,
        &mentee,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found.get(0).unwrap().start_ts, MONDAY + 10 * HOUR + 1_800);

    // The buffer belongs to the mentor, so swapping the pair keeps it.
    let swapped = c.find_slots(
        &mentee,
        &mentor,
        &s.program_id,
        &MONDAY,
        &(MONDAY + DAY),
        &0,
        &10,
    );
    assert_eq!(swapped, found);
}

#[test]
fn test_cancel_booking_validation() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);
    c.book_slot(
        &mentor,
        &hash(&s.env, 40),
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
    );

    let attacker = Address::generate(&s.env);
    assert_eq!(
        c.try_cancel_booking(&attacker, &hash(&s.env, 40)),
        Err(Ok(ContractError::NotAuthorized))
    );
    c.cancel_booking(&mentor, &hash(&s.env, 40));
    assert_eq!(
        c.try_cancel_booking(&mentor, &hash(&s.env, 40)),
        Err(Ok(ContractError::BookingNotActive))
    );
    assert_eq!(
        c.try_get_booking(&hash(&s.env, 99)),
        Err(Ok(ContractError::BookingNotFound))
    );
}

#[test]
fn test_is_slot_bookable_reports_stale_candidates() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);
    let start = MONDAY + 9 * HOUR;

    assert!(c.is_slot_bookable(&mentor, &mentee, &s.program_id, &start));
    assert!(!c.is_slot_bookable(&mentor, &mentee, &s.program_id, &(MONDAY + 3 * HOUR)));

    // Availability changes invalidate the candidate before booking.
    set_availability(
        &s,
        &mentor,
        schedule(&s.env, windows(&s.env, &[(2, 540, 660)])),
    );
    assert!(!c.is_slot_bookable(&mentor, &mentee, &s.program_id, &start));
    assert_eq!(
        c.try_book_slot(
            &mentor,
            &hash(&s.env, 50),
            &mentor,
            &mentee,
            &s.program_id,
            &start,
        ),
        Err(Ok(ContractError::SlotUnavailable))
    );
    assert_eq!(
        c.try_is_slot_bookable(&mentor, &mentor, &s.program_id, &start),
        Err(Ok(ContractError::InvalidParties))
    );
}

#[test]
fn test_bounded_search_rejects_too_many_candidates() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    // All-day availability on every weekday for a whole month.
    let mut all_day: Vec<WeeklyWindow> = Vec::new(&s.env);
    for weekday in 0..7u32 {
        all_day.push_back(WeeklyWindow {
            weekday,
            start_minute: 0,
            end_minute: 1_440,
        });
    }
    set_availability(&s, &mentor, schedule(&s.env, all_day.clone()));
    set_availability(&s, &mentee, schedule(&s.env, all_day));
    set_policy(&s, 300, 60, 16);
    let c = slots(&s.env, &s.slots_id);

    // A month-long blackout rejects every candidate, so the bounded search
    // stops instead of walking the entire month.
    c.set_blackout(
        &s.admin,
        &hash(&s.env, 60),
        &s.program_id,
        &MONDAY,
        &(MONDAY + 31 * DAY),
    );
    assert_eq!(
        c.try_find_slots(
            &mentor,
            &mentee,
            &s.program_id,
            &MONDAY,
            &(MONDAY + 31 * DAY),
            &0,
            &64,
        ),
        Err(Ok(ContractError::TooManyCandidates))
    );
}

#[test]
fn test_booking_ids_paginate() {
    let s = setup();
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    set_common_week(&s, &mentor, &mentee);
    set_policy(&s, 1_800, 900, 4);
    let c = slots(&s.env, &s.slots_id);

    c.book_slot(
        &mentor,
        &hash(&s.env, 70),
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR),
    );
    c.book_slot(
        &mentor,
        &hash(&s.env, 71),
        &mentor,
        &mentee,
        &s.program_id,
        &(MONDAY + 9 * HOUR + 3 * 900),
    );

    let first = c.booking_ids(&mentor, &0, &1);
    assert_eq!(first.len(), 1);
    assert_eq!(first.get(0).unwrap(), hash(&s.env, 70));
    let second = c.booking_ids(&mentor, &1, &1);
    assert_eq!(second.get(0).unwrap(), hash(&s.env, 71));
    assert_eq!(c.booking_ids(&mentor, &2, &1).len(), 0);
    assert_eq!(
        c.try_booking_ids(&mentor, &0, &0),
        Err(Ok(ContractError::InvalidPage))
    );
}

#[test]
fn test_version() {
    let s = setup();
    assert_eq!(slots(&s.env, &s.slots_id).version(), 1);
}
