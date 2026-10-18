use crate::{
    AvailabilityException, ContractError, ExceptionInput, ExceptionKind,
    MentorshipAvailabilityContract, MentorshipAvailabilityContractClient, OffsetChange,
    ScheduleInput, WeeklyWindow,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Vec,
};

/// 2026-01-05T00:00:00Z — a Monday, which is weekday 1 with 0 = Sunday.
const MONDAY: u64 = 1_767_571_200;
const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Setup {
    env: Env,
    contract_id: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    // A week before MONDAY, so the default 30-day booking horizon covers the
    // queried week while lead-time and horizon tests still have slack.
    env.ledger().set_timestamp(MONDAY - 7 * DAY);
    let contract_id = env.register(MentorshipAvailabilityContract, ());
    Setup { env, contract_id }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipAvailabilityContractClient<'a> {
    MentorshipAvailabilityContractClient::new(env, contract_id)
}

fn window(weekday: u32, start_minute: u32, end_minute: u32) -> WeeklyWindow {
    WeeklyWindow {
        weekday,
        start_minute,
        end_minute,
    }
}

fn windows(env: &Env, specs: &[(u32, u32, u32)]) -> Vec<WeeklyWindow> {
    let mut v = Vec::new(env);
    for (weekday, start, end) in specs.iter() {
        v.push_back(window(*weekday, *start, *end));
    }
    v
}

fn changes(env: &Env, specs: &[(u64, i64)]) -> Vec<OffsetChange> {
    let mut v = Vec::new(env);
    for (effective_from, offset) in specs.iter() {
        v.push_back(OffsetChange {
            effective_from: *effective_from,
            utc_offset_seconds: *offset,
        });
    }
    v
}

/// A UTC schedule with the given windows and no offset changes.
fn input(env: &Env, weekly: Vec<WeeklyWindow>) -> ScheduleInput {
    ScheduleInput {
        timezone_hash: hash(env, 1),
        utc_offset_seconds: 0,
        offset_changes: Vec::new(env),
        weekly,
        lead_time_seconds: 0,
        buffer_before_seconds: 0,
        buffer_after_seconds: 0,
        booking_horizon_days: 30,
        effective_from: 1,
        effective_until: 0,
    }
}

fn set_schedule(s: &Setup, participant: &Address, spec: ScheduleInput) {
    client(&s.env, &s.contract_id).set_schedule(participant, &spec);
}

// ── schedule validation ────────────────────────────────────────────────────

#[test]
fn test_normalizes_overlapping_and_touching_windows() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    // Ordered input, but windows 1 and 2 overlap and window 3 touches 2.
    let schedule = c.set_schedule(
        &participant,
        &input(
            &s.env,
            windows(
                &s.env,
                &[
                    (1, 540, 660), // Monday 09:00-11:00
                    (1, 630, 720), // Monday 10:30-12:00 (overlaps previous)
                    (1, 720, 780), // Monday 12:00-13:00 (touches previous)
                    (3, 540, 600), // Wednesday 09:00-10:00 (separate)
                ],
            ),
        ),
    );

    assert_eq!(schedule.weekly.len(), 2);
    let first = schedule.weekly.get(0).unwrap();
    assert_eq!(first.weekday, 1);
    assert_eq!(first.start_minute, 540);
    assert_eq!(first.end_minute, 780);
    let second = schedule.weekly.get(1).unwrap();
    assert_eq!(second.weekday, 3);
    assert_eq!(second.start_minute, 540);
    assert_eq!(second.end_minute, 600);
}

#[test]
fn test_rejects_unordered_and_invalid_windows() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    let unordered = input(&s.env, windows(&s.env, &[(3, 540, 600), (1, 540, 600)]));
    assert_eq!(
        c.try_set_schedule(&participant, &unordered),
        Err(Ok(ContractError::WindowsNotOrdered))
    );

    let same_day_descending = input(&s.env, windows(&s.env, &[(1, 660, 720), (1, 540, 600)]));
    assert_eq!(
        c.try_set_schedule(&participant, &same_day_descending),
        Err(Ok(ContractError::WindowsNotOrdered))
    );

    let empty = input(&s.env, Vec::new(&s.env));
    assert_eq!(
        c.try_set_schedule(&participant, &empty),
        Err(Ok(ContractError::InvalidWeeklyWindow))
    );

    for bad in [
        (7u32, 540u32, 600u32),
        (1, 600, 600),
        (1, 600, 540),
        (1, 1_440, 1_500),
    ] {
        let spec = input(&s.env, windows(&s.env, &[bad]));
        assert_eq!(
            c.try_set_schedule(&participant, &spec),
            Err(Ok(ContractError::InvalidWeeklyWindow)),
            "window {:?} should be rejected",
            bad
        );
    }

    let too_many: Vec<WeeklyWindow> = {
        let mut v = Vec::new(&s.env);
        for weekday in 0..7u32 {
            for slot in 0..5u32 {
                let start = slot * 200;
                v.push_back(window(weekday, start, start + 100));
            }
        }
        v
    };
    let spec = input(&s.env, too_many);
    assert_eq!(
        c.try_set_schedule(&participant, &spec),
        Err(Ok(ContractError::TooManyWindows))
    );
}

#[test]
fn test_rejects_invalid_offset_changes() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    let mut out_of_range = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    out_of_range.utc_offset_seconds = 15 * 3_600;
    assert_eq!(
        c.try_set_schedule(&participant, &out_of_range),
        Err(Ok(ContractError::InvalidOffset))
    );

    let mut unordered = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    unordered.offset_changes = changes(&s.env, &[(10_000, 3_600), (5_000, 7_200)]);
    assert_eq!(
        c.try_set_schedule(&participant, &unordered),
        Err(Ok(ContractError::InvalidOffsetChange))
    );

    let mut redundant = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    redundant.offset_changes = changes(&s.env, &[(10_000, 0)]);
    assert_eq!(
        c.try_set_schedule(&participant, &redundant),
        Err(Ok(ContractError::InvalidOffsetChange))
    );

    let mut too_many = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    too_many.offset_changes = changes(
        &s.env,
        &[
            (10_000, 3_600),
            (20_000, 0),
            (30_000, 3_600),
            (40_000, 0),
            (50_000, 3_600),
            (60_000, 0),
            (70_000, 3_600),
            (80_000, 0),
            (90_000, 3_600),
        ],
    );
    assert_eq!(
        c.try_set_schedule(&participant, &too_many),
        Err(Ok(ContractError::TooManyOffsetChanges))
    );

    let mut zero_hash = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    zero_hash.timezone_hash = hash(&s.env, 0);
    assert_eq!(
        c.try_set_schedule(&participant, &zero_hash),
        Err(Ok(ContractError::InvalidTimezoneRef))
    );
}

#[test]
fn test_rejects_invalid_guard_rails() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    let base = input(&s.env, windows(&s.env, &[(1, 540, 600)]));

    let mut lead = base.clone();
    lead.lead_time_seconds = 31 * DAY;
    assert_eq!(
        c.try_set_schedule(&participant, &lead),
        Err(Ok(ContractError::InvalidLeadTime))
    );

    let mut buffer = base.clone();
    buffer.buffer_after_seconds = DAY + 1;
    assert_eq!(
        c.try_set_schedule(&participant, &buffer),
        Err(Ok(ContractError::InvalidBuffer))
    );

    let mut horizon = base.clone();
    horizon.booking_horizon_days = 0;
    assert_eq!(
        c.try_set_schedule(&participant, &horizon),
        Err(Ok(ContractError::InvalidBookingHorizon))
    );

    let mut horizon = base.clone();
    horizon.booking_horizon_days = 366;
    assert_eq!(
        c.try_set_schedule(&participant, &horizon),
        Err(Ok(ContractError::InvalidBookingHorizon))
    );

    let mut effective = base.clone();
    effective.effective_until = effective.effective_from;
    assert_eq!(
        c.try_set_schedule(&participant, &effective),
        Err(Ok(ContractError::InvalidEffectiveRange))
    );

    let mut effective = base;
    effective.effective_from = 0;
    assert_eq!(
        c.try_set_schedule(&participant, &effective),
        Err(Ok(ContractError::InvalidEffectiveRange))
    );
}

#[test]
fn test_schedule_missing_and_version_progression() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    assert_eq!(
        c.try_get_schedule(&participant),
        Err(Ok(ContractError::ScheduleNotFound))
    );

    let first = c.set_schedule(
        &participant,
        &input(&s.env, windows(&s.env, &[(1, 540, 600)])),
    );
    assert_eq!(first.version, 1);
    assert!(first.enabled);

    let second = c.set_schedule(
        &participant,
        &input(&s.env, windows(&s.env, &[(2, 540, 600)])),
    );
    assert_eq!(second.version, 2);

    c.set_enabled(&participant, &false);
    c.set_enabled(&participant, &false); // idempotent, no extra version
    let disabled = c.get_schedule(&participant);
    assert!(!disabled.enabled);
    assert_eq!(disabled.version, 3);
}

/// The narrow cross-contract view carries the guard rails and version that
/// `contracts/mentorship-slots` mirrors, so its field set is asserted here.
#[test]
fn test_availability_summary_reports_guard_rails() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    assert_eq!(
        c.try_availability_summary(&participant),
        Err(Ok(ContractError::ScheduleNotFound))
    );

    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.lead_time_seconds = 3_600;
    spec.buffer_before_seconds = 300;
    spec.buffer_after_seconds = 900;
    spec.booking_horizon_days = 45;
    spec.effective_until = MONDAY + 30 * DAY;
    set_schedule(&s, &participant, spec);

    let summary = c.availability_summary(&participant);
    assert_eq!(summary.participant, participant);
    assert!(summary.enabled);
    assert_eq!(summary.version, 1);
    assert_eq!(summary.lead_time_seconds, 3_600);
    assert_eq!(summary.buffer_before_seconds, 300);
    assert_eq!(summary.buffer_after_seconds, 900);
    assert_eq!(summary.booking_horizon_days, 45);
    assert_eq!(summary.effective_from, 1);
    assert_eq!(summary.effective_until, MONDAY + 30 * DAY);

    c.set_enabled(&participant, &false);
    let summary = c.availability_summary(&participant);
    assert!(!summary.enabled);
    assert_eq!(summary.version, 2);
}

// ── materialized ranges ────────────────────────────────────────────────────

#[test]
fn test_available_ranges_materializes_weekly_windows() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 660), (1, 780, 900)])),
    );

    // Query the whole Monday.
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 2);
    let first = ranges.get(0).unwrap();
    assert_eq!(first.start_ts, MONDAY + 9 * HOUR);
    assert_eq!(first.end_ts, MONDAY + 11 * HOUR);
    let second = ranges.get(1).unwrap();
    assert_eq!(second.start_ts, MONDAY + 13 * HOUR);
    assert_eq!(second.end_ts, MONDAY + 15 * HOUR);

    // The pattern repeats on the next Monday. The query end is exclusive, so
    // span into the following week to capture it.
    let week = c.available_ranges(&participant, &MONDAY, &(MONDAY + 8 * DAY), &0, &32);
    assert_eq!(week.len(), 4);
    assert_eq!(week.get(2).unwrap().start_ts, MONDAY + 7 * DAY + 9 * HOUR);
}

#[test]
fn test_ranges_are_clamped_to_effective_range_lead_time_and_horizon() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.effective_from = MONDAY + 10 * HOUR;
    set_schedule(&s, &participant, spec);

    // The window starts at 09:00 but is only effective from 10:00.
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges.get(0).unwrap().start_ts, MONDAY + 10 * HOUR);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 11 * HOUR);

    // A horizon shorter than the query caps the answer.
    s.env.ledger().set_timestamp(MONDAY - 2 * DAY);
    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.booking_horizon_days = 1;
    set_schedule(&s, &participant, spec);
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + 7 * DAY), &0, &10);
    assert_eq!(ranges.len(), 0);

    // The lead time pushes the earliest bookable instant past the window.
    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.lead_time_seconds = 4 * DAY;
    set_schedule(&s, &participant, spec);
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 0);

    // Effective-until closes the schedule.
    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 660)]));
    spec.effective_until = MONDAY + 9 * HOUR + 1_800;
    set_schedule(&s, &participant, spec);
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 9 * HOUR + 1_800);
}

#[test]
fn test_disabled_schedule_has_no_ranges() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );
    c.set_enabled(&participant, &false);
    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 0);
}

#[test]
fn test_query_validation() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );

    assert_eq!(
        c.try_available_ranges(&participant, &MONDAY, &MONDAY, &0, &10),
        Err(Ok(ContractError::InvalidQueryRange))
    );
    assert_eq!(
        c.try_available_ranges(&participant, &MONDAY, &(MONDAY + 91 * DAY), &0, &10),
        Err(Ok(ContractError::QuerySpanTooLarge))
    );
    assert_eq!(
        c.try_available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &0),
        Err(Ok(ContractError::InvalidPage))
    );
    assert_eq!(
        c.try_available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &65),
        Err(Ok(ContractError::InvalidPage))
    );
    let missing = Address::generate(&s.env);
    assert_eq!(
        c.try_available_ranges(&missing, &MONDAY, &(MONDAY + DAY), &0, &10),
        Err(Ok(ContractError::ScheduleNotFound))
    );
}

#[test]
fn test_pagination_is_stable() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(
            &s.env,
            windows(&s.env, &[(1, 540, 600), (1, 660, 720), (1, 780, 840)]),
        ),
    );

    let first = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &2);
    assert_eq!(first.len(), 2);
    let second = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &1, &2);
    assert_eq!(second.len(), 1);
    assert_eq!(second.get(0).unwrap().start_ts, MONDAY + 13 * HOUR);
    // Pages never overlap and are ordered.
    assert!(first.get(1).unwrap().end_ts <= second.get(0).unwrap().start_ts);
    let third = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &2, &2);
    assert_eq!(third.len(), 0);
}

#[test]
fn test_is_available_checks_full_coverage() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );

    assert!(c.is_available(&participant, &(MONDAY + 9 * HOUR), &(MONDAY + 10 * HOUR)));
    assert!(c.is_available(&participant, &(MONDAY + 9 * HOUR), &(MONDAY + 11 * HOUR)));
    // Straddling the end of the window is not fully covered.
    assert!(!c.is_available(&participant, &(MONDAY + 10 * HOUR), &(MONDAY + 12 * HOUR)));
    // A Tuesday has no window.
    assert!(!c.is_available(
        &participant,
        &(MONDAY + DAY + 9 * HOUR),
        &(MONDAY + DAY + 10 * HOUR)
    ));
    assert_eq!(
        c.try_is_available(&participant, &MONDAY, &MONDAY),
        Err(Ok(ContractError::InvalidQueryRange))
    );
}

// ── DST correctness ────────────────────────────────────────────────────────

#[test]
fn test_spring_forward_gap_removes_the_skipped_local_hour() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    // Base offset +1h; at Wednesday 02:00 local (01:00 UTC) the offset jumps
    // to +2h, so local 02:00-02:30 does not exist that day.
    let mut spec = input(&s.env, windows(&s.env, &[(3, 120, 150), (3, 540, 600)]));
    spec.utc_offset_seconds = HOUR as i64;
    spec.offset_changes = changes(&s.env, &[(MONDAY + 2 * DAY + HOUR, 2 * HOUR as i64)]);
    set_schedule(&s, &participant, spec);

    // Wednesday 02:00-02:30 local does not exist after the jump, so only the
    // 09:00-10:00 window is materialized.
    let ranges = c.available_ranges(
        &participant,
        &(MONDAY + 2 * DAY),
        &(MONDAY + 3 * DAY),
        &0,
        &10,
    );
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges.get(0).unwrap().start_ts, MONDAY + 2 * DAY + 7 * HOUR);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 2 * DAY + 8 * HOUR);
}

#[test]
fn test_fall_back_ambiguous_hour_uses_the_earliest_instant() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    // Offset +1h until MONDAY+25h, then 0 (clocks go back at 02:00 local).
    let mut spec = input(&s.env, windows(&s.env, &[(2, 60, 90)])); // Tuesday 01:00-01:30
    spec.utc_offset_seconds = HOUR as i64;
    spec.offset_changes = changes(&s.env, &[(MONDAY + 25 * HOUR, 0)]);
    set_schedule(&s, &participant, spec);

    // Local 01:00 exists twice on Tuesday; the earliest UTC instant wins.
    let ranges = c.available_ranges(&participant, &(MONDAY + DAY), &(MONDAY + 2 * DAY), &0, &10);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges.get(0).unwrap().start_ts, MONDAY + 24 * HOUR);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 24 * HOUR + 30 * 60);
}

#[test]
fn test_dst_shifts_are_deterministic_across_the_transition_week() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 600)])); // Monday 09:00-10:00
    spec.utc_offset_seconds = 0;
    spec.offset_changes = changes(&s.env, &[(MONDAY + 3 * DAY, HOUR as i64)]);
    set_schedule(&s, &participant, spec);

    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + 8 * DAY), &0, &10);
    assert_eq!(ranges.len(), 2);
    // Before the change the window is 09:00 UTC ...
    assert_eq!(ranges.get(0).unwrap().start_ts, MONDAY + 9 * HOUR);
    // ... and after it, 08:00 UTC, which is still 09:00 local.
    assert_eq!(ranges.get(1).unwrap().start_ts, MONDAY + 7 * DAY + 8 * HOUR);
    // Repeated calls return identical results.
    let again = c.available_ranges(&participant, &MONDAY, &(MONDAY + 8 * DAY), &0, &10);
    assert_eq!(again, ranges);
}

// ── exceptions ─────────────────────────────────────────────────────────────

#[test]
fn test_unavailable_exception_blackouts_ranges() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );

    c.add_exception(
        &participant,
        &ExceptionInput {
            id: hash(&s.env, 9),
            kind: ExceptionKind::Unavailable,
            from_ts: MONDAY + 9 * HOUR + 1_800,
            until_ts: MONDAY + 10 * HOUR + 900,
            reference_hash: hash(&s.env, 10),
        },
    );

    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 9 * HOUR + 1_800);
    assert_eq!(ranges.get(1).unwrap().start_ts, MONDAY + 10 * HOUR + 900);
    assert_eq!(ranges.get(1).unwrap().end_ts, MONDAY + 11 * HOUR);

    // Removing the exception restores the full window.
    c.remove_exception(&participant, &hash(&s.env, 9));
    let restored = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored.get(0).unwrap().start_ts, MONDAY + 9 * HOUR);
    assert_eq!(restored.get(0).unwrap().end_ts, MONDAY + 11 * HOUR);
}

#[test]
fn test_available_exception_adds_a_one_off_range() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 600)])),
    );

    c.add_exception(
        &participant,
        &ExceptionInput {
            id: hash(&s.env, 11),
            kind: ExceptionKind::Available,
            from_ts: MONDAY + 14 * HOUR,
            until_ts: MONDAY + 15 * HOUR,
            reference_hash: hash(&s.env, 12),
        },
    );

    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges.get(1).unwrap().start_ts, MONDAY + 14 * HOUR);
    assert_eq!(ranges.get(1).unwrap().end_ts, MONDAY + 15 * HOUR);
    // A one-off exception does not repeat the following week.
    let next_week = c.available_ranges(
        &participant,
        &(MONDAY + 7 * DAY),
        &(MONDAY + 8 * DAY),
        &0,
        &10,
    );
    assert_eq!(next_week.len(), 1);
}

#[test]
fn test_exception_idempotency_and_validation() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 600)])),
    );

    let exception = ExceptionInput {
        id: hash(&s.env, 13),
        kind: ExceptionKind::Unavailable,
        from_ts: MONDAY + 9 * HOUR,
        until_ts: MONDAY + 10 * HOUR,
        reference_hash: hash(&s.env, 14),
    };
    c.add_exception(&participant, &exception);
    assert_eq!(
        c.try_add_exception(&participant, &exception),
        Err(Ok(ContractError::ExceptionAlreadyExists))
    );
    assert_eq!(c.exception_ids(&participant).len(), 1);

    let stored = c.get_exception(&participant, &hash(&s.env, 13));
    assert_eq!(stored.kind, ExceptionKind::Unavailable);
    assert_eq!(stored.reference_hash, hash(&s.env, 14));

    c.remove_exception(&participant, &hash(&s.env, 13));
    assert_eq!(c.exception_ids(&participant).len(), 0);
    assert_eq!(
        c.try_remove_exception(&participant, &hash(&s.env, 13)),
        Err(Ok(ContractError::ExceptionNotFound))
    );
    assert_eq!(
        c.try_get_exception(&participant, &hash(&s.env, 13)),
        Err(Ok(ContractError::ExceptionNotFound))
    );

    let inverted = ExceptionInput {
        id: hash(&s.env, 15),
        kind: ExceptionKind::Unavailable,
        from_ts: MONDAY + 10 * HOUR,
        until_ts: MONDAY + 9 * HOUR,
        reference_hash: hash(&s.env, 16),
    };
    assert_eq!(
        c.try_add_exception(&participant, &inverted),
        Err(Ok(ContractError::InvalidExceptionRange))
    );

    let too_long = ExceptionInput {
        id: hash(&s.env, 17),
        kind: ExceptionKind::Unavailable,
        from_ts: MONDAY,
        until_ts: MONDAY + 32 * DAY,
        reference_hash: hash(&s.env, 18),
    };
    assert_eq!(
        c.try_add_exception(&participant, &too_long),
        Err(Ok(ContractError::ExceptionSpanTooLarge))
    );
}

#[test]
fn test_exception_storage_is_bounded() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(
        &s,
        &participant,
        input(&s.env, windows(&s.env, &[(1, 540, 600)])),
    );

    let mut last: Option<AvailabilityException> = None;
    for index in 0..64u32 {
        c.add_exception(
            &participant,
            &ExceptionInput {
                id: BytesN::from_array(&s.env, &[index as u8; 32]),
                kind: ExceptionKind::Unavailable,
                from_ts: MONDAY + (index as u64) * HOUR,
                until_ts: MONDAY + (index as u64) * HOUR + 60,
                reference_hash: hash(&s.env, 20),
            },
        );
        last = None;
    }
    assert!(last.is_none());
    assert_eq!(c.exception_ids(&participant).len(), 64);

    let overflow = ExceptionInput {
        id: hash(&s.env, 99),
        kind: ExceptionKind::Unavailable,
        from_ts: MONDAY + 70 * HOUR,
        until_ts: MONDAY + 71 * HOUR,
        reference_hash: hash(&s.env, 20),
    };
    assert_eq!(
        c.try_add_exception(&participant, &overflow),
        Err(Ok(ContractError::TooManyExceptions))
    );
}

#[test]
fn test_exceptions_do_not_leak_between_participants() {
    let s = setup();
    let first = Address::generate(&s.env);
    let second = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);
    set_schedule(&s, &first, input(&s.env, windows(&s.env, &[(1, 540, 660)])));
    set_schedule(
        &s,
        &second,
        input(&s.env, windows(&s.env, &[(1, 540, 660)])),
    );

    c.add_exception(
        &first,
        &ExceptionInput {
            id: hash(&s.env, 30),
            kind: ExceptionKind::Unavailable,
            from_ts: MONDAY + 9 * HOUR,
            until_ts: MONDAY + 10 * HOUR,
            reference_hash: hash(&s.env, 31),
        },
    );

    assert_eq!(c.exception_ids(&first).len(), 1);
    assert_eq!(c.exception_ids(&second).len(), 0);
    let untouched = c.available_ranges(&second, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(untouched.len(), 1);
    assert_eq!(untouched.get(0).unwrap().start_ts, MONDAY + 9 * HOUR);
}

#[test]
fn test_negative_offset_shifts_utc_start() {
    let s = setup();
    let participant = Address::generate(&s.env);
    let c = client(&s.env, &s.contract_id);

    // UTC-05:00: Monday 09:00 local is 14:00 UTC.
    let mut spec = input(&s.env, windows(&s.env, &[(1, 540, 600)]));
    spec.utc_offset_seconds = -5 * HOUR as i64;
    set_schedule(&s, &participant, spec);

    let ranges = c.available_ranges(&participant, &MONDAY, &(MONDAY + DAY), &0, &10);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges.get(0).unwrap().start_ts, MONDAY + 14 * HOUR);
    assert_eq!(ranges.get(0).unwrap().end_ts, MONDAY + 15 * HOUR);
}

#[test]
fn test_version() {
    let s = setup();
    assert_eq!(client(&s.env, &s.contract_id).version(), 1);
}
