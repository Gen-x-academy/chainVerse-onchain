#![no_std]

//! Mentorship recurring availability contract (work item MENTOR-053, issue
//! #1249).
//!
//! A participant (mentor or mentee) owns a recurring weekly availability
//! schedule. The schedule defines:
//!
//! - **Weekly windows** in the participant's local wall clock (weekday plus
//!   start/end minute). Input order is validated and overlapping or touching
//!   windows on the same weekday are **merged deterministically** into a
//!   canonical, sorted list before storage, so every caller sees the same
//!   normalized schedule.
//! - **Time zone** as a `timezone_hash` commitment (the IANA name stays
//!   off-chain) plus a base UTC offset and a bounded list of **UTC offset
//!   changes**. This is how DST is represented: the off-chain scheduler
//!   publishes the exact offset transitions it obtained from its tz database,
//!   and the contract converts local windows to UTC with the offset in effect
//!   at each instant. A spring-forward gap therefore yields no range, and an
//!   ambiguous fall-back hour resolves to the **earliest** matching UTC
//!   instant. No implicit locale rules are ever guessed on-chain.
//! - **Booking guard rails**: effective range, minimum lead time, buffers,
//!   and a booking horizon. Materialized ranges are clipped to them.
//! - **Exceptions**: bounded UTC ranges that either black out availability or
//!   add extra availability (for example a one-off session).
//!
//! `available_ranges` materializes the schedule into sorted, merged UTC
//! ranges. It is filtered by the participant's effective range, lead time,
//! booking horizon, and unavailable exceptions, and it is paginated so a
//! large result can never exceed a bounded response size.
//!
//! Existing bookings are never touched: this contract stores no bookings and
//! every mutation bumps the schedule `version`, which the scheduling contract
//! uses to detect and revalidate stale candidates before booking.
//!
//! See `contracts/docs/mentorship-availability.md` for the off-chain
//! boundary, privacy notes, and rollout guidance.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const SECONDS_PER_DAY: u64 = 86_400;
const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_DAY: u32 = 1_440;

/// Bounded storage: a week can never hold more than this many windows before
/// normalization (28 = four windows per day).
const MAX_WEEKLY_WINDOWS: u32 = 28;
/// Bounded storage: at most this many explicit offset transitions are kept
/// (four years of a twice-yearly DST rule).
const MAX_OFFSET_CHANGES: u32 = 8;
/// Bounded storage: at most this many exceptions per participant.
const MAX_EXCEPTIONS: u32 = 64;
/// An exception may cover at most 31 days.
const MAX_EXCEPTION_SPAN_SECONDS: u64 = 31 * SECONDS_PER_DAY;
/// Lead time may not exceed 30 days.
const MAX_LEAD_TIME_SECONDS: u64 = 30 * SECONDS_PER_DAY;
/// Each buffer may not exceed 24 hours.
const MAX_BUFFER_SECONDS: u64 = SECONDS_PER_DAY;
/// Booking horizon is at least one day and at most one year.
const MAX_BOOKING_HORIZON_DAYS: u32 = 365;
/// One availability query may cover at most 90 days; callers page across a
/// wider span with repeated calls.
const MAX_QUERY_SPAN_SECONDS: u64 = 90 * SECONDS_PER_DAY;
const MAX_PAGE_SIZE: u32 = 64;
const MAX_PAGE: u32 = 4_096;

/// Real-world UTC offsets range from -12:00 to +14:00.
const MIN_UTC_OFFSET_SECONDS: i64 = -12 * 3_600;
const MAX_UTC_OFFSET_SECONDS: i64 = 14 * 3_600;

/// 1970-01-01 was a Thursday, so `day_index % 7` is Thursday-based.
/// Adding this constant makes `weekday` Sunday-based (0 = Sunday .. 6 =
/// Saturday), matching the `WeeklyWindow` documentation and RFC 5545's
/// `WKST`-independent weekday numbering used by the off-chain calendar UI.
const EPOCH_WEEKDAY_OFFSET: i64 = 4;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    ScheduleNotFound = 1,
    /// A window's weekday must be 0..=6 and its minutes must satisfy
    /// `0 <= start < end <= 1440`.
    InvalidWeeklyWindow = 2,
    TooManyWindows = 3,
    /// Windows must be supplied sorted by (weekday, start_minute).
    WindowsNotOrdered = 4,
    InvalidEffectiveRange = 5,
    InvalidLeadTime = 6,
    InvalidBuffer = 7,
    InvalidBookingHorizon = 8,
    TooManyOffsetChanges = 9,
    /// Offset changes must be strictly increasing and must actually change
    /// the offset.
    InvalidOffsetChange = 10,
    InvalidOffset = 11,
    TooManyExceptions = 12,
    ExceptionAlreadyExists = 13,
    ExceptionNotFound = 14,
    InvalidExceptionRange = 15,
    ExceptionSpanTooLarge = 16,
    /// The query interval is empty or inverted.
    InvalidQueryRange = 17,
    QuerySpanTooLarge = 18,
    InvalidPage = 19,
    VersionOverflow = 20,
    /// The time-zone commitment must not be all zeroes.
    InvalidTimezoneRef = 21,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// A participant's availability schedule.
    Schedule(Address),
    /// The exception ids of a participant, in insertion order.
    Exceptions(Address),
    /// One exception, keyed by (participant, id).
    Exception(Address, BytesN<32>),
}

/// One recurring window, in the participant's local wall clock.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WeeklyWindow {
    /// 0 = Sunday .. 6 = Saturday.
    pub weekday: u32,
    /// Local minutes from midnight; `0 <= start_minute < 1_440`.
    pub start_minute: u32,
    /// Local minutes from midnight; `start_minute < end_minute <= 1_440`.
    pub end_minute: u32,
}

/// An explicit UTC offset transition, as resolved off-chain from the tz
/// database. `effective_from` is the first UTC instant at which the new
/// offset applies.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OffsetChange {
    pub effective_from: u64,
    pub utc_offset_seconds: i64,
}

/// Caller-supplied schedule definition.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleInput {
    /// Commitment over the off-chain IANA time-zone identifier.
    pub timezone_hash: BytesN<32>,
    /// Offset used before the first `offset_changes` entry.
    pub utc_offset_seconds: i64,
    pub offset_changes: Vec<OffsetChange>,
    pub weekly: Vec<WeeklyWindow>,
    pub lead_time_seconds: u64,
    pub buffer_before_seconds: u64,
    pub buffer_after_seconds: u64,
    pub booking_horizon_days: u32,
    pub effective_from: u64,
    /// `0` means open-ended.
    pub effective_until: u64,
}

/// The normalized schedule as stored on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilitySchedule {
    pub participant: Address,
    pub timezone_hash: BytesN<32>,
    pub utc_offset_seconds: i64,
    pub offset_changes: Vec<OffsetChange>,
    /// Canonical, sorted, non-overlapping weekly windows.
    pub weekly: Vec<WeeklyWindow>,
    pub lead_time_seconds: u64,
    pub buffer_before_seconds: u64,
    pub buffer_after_seconds: u64,
    pub booking_horizon_days: u32,
    pub effective_from: u64,
    pub effective_until: u64,
    pub enabled: bool,
    /// Increments on every schedule mutation; consumers compare versions to
    /// detect stale candidate slots.
    pub version: u32,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExceptionKind {
    /// Remove availability in this UTC range.
    Unavailable,
    /// Add availability in this UTC range.
    Available,
}

/// Caller-supplied exception. `id` is the caller's idempotency key.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExceptionInput {
    pub id: BytesN<32>,
    pub kind: ExceptionKind,
    pub from_ts: u64,
    pub until_ts: u64,
    /// Commitment over any off-chain note; no free text is stored.
    pub reference_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilityException {
    pub id: BytesN<32>,
    pub kind: ExceptionKind,
    pub from_ts: u64,
    pub until_ts: u64,
    pub reference_hash: BytesN<32>,
    pub created_at: u64,
}

/// A materialized UTC availability range: `start_ts <= t < end_ts`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailableRange {
    pub start_ts: u64,
    pub end_ts: u64,
}

/// The guard rails and identity of a participant's schedule, without the
/// weekly windows and offset transitions.
///
/// This is the narrow cross-contract view: consumers (for example
/// `contracts/mentorship-slots`) read the summary plus `available_ranges`
/// instead of the full schedule, so the shared surface stays small. Field
/// order is part of the ABI — mirrors must match it exactly.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailabilitySummary {
    pub participant: Address,
    pub enabled: bool,
    pub version: u32,
    pub lead_time_seconds: u64,
    pub buffer_before_seconds: u64,
    pub buffer_after_seconds: u64,
    pub booking_horizon_days: u32,
    pub effective_from: u64,
    pub effective_until: u64,
}

#[contract]
pub struct MentorshipAvailabilityContract;

#[contractimpl]
impl MentorshipAvailabilityContract {
    // ── helpers ──────────────────────────────────────────────────────────

    fn put_schedule(env: &Env, schedule: &AvailabilitySchedule) {
        let key = DataKey::Schedule(schedule.participant.clone());
        env.storage().persistent().set(&key, schedule);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_schedule(
        env: &Env,
        participant: &Address,
    ) -> Result<AvailabilitySchedule, ContractError> {
        let key = DataKey::Schedule(participant.clone());
        let schedule: AvailabilitySchedule = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ScheduleNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(schedule)
    }

    fn load_exception_ids(env: &Env, participant: &Address) -> Vec<BytesN<32>> {
        let key = DataKey::Exceptions(participant.clone());
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if !ids.is_empty() {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        ids
    }

    fn put_exception_ids(env: &Env, participant: &Address, ids: &Vec<BytesN<32>>) {
        let key = DataKey::Exceptions(participant.clone());
        env.storage().persistent().set(&key, ids);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_exceptions(env: &Env, participant: &Address) -> Vec<AvailabilityException> {
        let ids = Self::load_exception_ids(env, participant);
        let mut exceptions: Vec<AvailabilityException> = Vec::new(env);
        for id in ids.iter() {
            let key = DataKey::Exception(participant.clone(), id);
            if let Some(exception) = env
                .storage()
                .persistent()
                .get::<DataKey, AvailabilityException>(&key)
            {
                exceptions.push_back(exception);
            }
        }
        exceptions
    }

    fn validate_offset(offset: i64) -> Result<(), ContractError> {
        if !(MIN_UTC_OFFSET_SECONDS..=MAX_UTC_OFFSET_SECONDS).contains(&offset) {
            return Err(ContractError::InvalidOffset);
        }
        Ok(())
    }

    /// Validate ordering and merge overlapping/touching windows into a
    /// canonical sorted list.
    fn normalize_windows(
        env: &Env,
        windows: &Vec<WeeklyWindow>,
    ) -> Result<Vec<WeeklyWindow>, ContractError> {
        let count = windows.len();
        if count == 0 {
            return Err(ContractError::InvalidWeeklyWindow);
        }
        if count > MAX_WEEKLY_WINDOWS {
            return Err(ContractError::TooManyWindows);
        }

        let mut normalized: Vec<WeeklyWindow> = Vec::new(env);
        let mut previous: Option<WeeklyWindow> = None;
        for index in 0..count {
            let window = windows
                .get(index)
                .ok_or(ContractError::InvalidWeeklyWindow)?;
            if window.weekday > 6
                || window.start_minute >= window.end_minute
                || window.start_minute >= MINUTES_PER_DAY
                || window.end_minute > MINUTES_PER_DAY
            {
                return Err(ContractError::InvalidWeeklyWindow);
            }
            if let Some(prev) = previous {
                if window.weekday < prev.weekday
                    || (window.weekday == prev.weekday && window.start_minute < prev.start_minute)
                {
                    return Err(ContractError::WindowsNotOrdered);
                }
            }
            previous = Some(window);

            let merge_into_last = match normalized.last() {
                Some(last) => {
                    last.weekday == window.weekday && window.start_minute <= last.end_minute
                }
                None => false,
            };
            if merge_into_last {
                let last = normalized
                    .get(normalized.len() - 1)
                    .ok_or(ContractError::InvalidWeeklyWindow)?;
                normalized.set(
                    normalized.len() - 1,
                    WeeklyWindow {
                        weekday: last.weekday,
                        start_minute: last.start_minute,
                        end_minute: if window.end_minute > last.end_minute {
                            window.end_minute
                        } else {
                            last.end_minute
                        },
                    },
                );
            } else {
                normalized.push_back(window);
            }
        }
        Ok(normalized)
    }

    // ── MENTOR-053 — schedule management ─────────────────────────────────

    /// Replace the caller's availability schedule. Only the participant can
    /// mutate it; a retry with the same input is a new version and emits an
    /// event, but never changes any booking (this contract holds none).
    pub fn set_schedule(
        env: Env,
        participant: Address,
        input: ScheduleInput,
    ) -> Result<AvailabilitySchedule, ContractError> {
        participant.require_auth();

        if input.timezone_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidTimezoneRef);
        }
        Self::validate_offset(input.utc_offset_seconds)?;

        let changes = input.offset_changes.len();
        if changes > MAX_OFFSET_CHANGES {
            return Err(ContractError::TooManyOffsetChanges);
        }
        let mut previous_from: u64 = 0;
        let mut previous_offset = input.utc_offset_seconds;
        for index in 0..changes {
            let change = input
                .offset_changes
                .get(index)
                .ok_or(ContractError::InvalidOffsetChange)?;
            if change.effective_from == 0 || change.effective_from <= previous_from {
                return Err(ContractError::InvalidOffsetChange);
            }
            Self::validate_offset(change.utc_offset_seconds)?;
            if change.utc_offset_seconds == previous_offset {
                // A redundant transition would make normalization ambiguous.
                return Err(ContractError::InvalidOffsetChange);
            }
            previous_from = change.effective_from;
            previous_offset = change.utc_offset_seconds;
        }

        let weekly = Self::normalize_windows(&env, &input.weekly)?;

        if input.lead_time_seconds > MAX_LEAD_TIME_SECONDS {
            return Err(ContractError::InvalidLeadTime);
        }
        if input.buffer_before_seconds > MAX_BUFFER_SECONDS
            || input.buffer_after_seconds > MAX_BUFFER_SECONDS
        {
            return Err(ContractError::InvalidBuffer);
        }
        if input.booking_horizon_days == 0 || input.booking_horizon_days > MAX_BOOKING_HORIZON_DAYS
        {
            return Err(ContractError::InvalidBookingHorizon);
        }
        if input.effective_from == 0
            || (input.effective_until != 0 && input.effective_until <= input.effective_from)
        {
            return Err(ContractError::InvalidEffectiveRange);
        }

        let version = match env
            .storage()
            .persistent()
            .get::<DataKey, AvailabilitySchedule>(&DataKey::Schedule(participant.clone()))
        {
            Some(existing) => existing
                .version
                .checked_add(1)
                .ok_or(ContractError::VersionOverflow)?,
            None => 1,
        };

        let schedule = AvailabilitySchedule {
            participant: participant.clone(),
            timezone_hash: input.timezone_hash,
            utc_offset_seconds: input.utc_offset_seconds,
            offset_changes: input.offset_changes,
            weekly,
            lead_time_seconds: input.lead_time_seconds,
            buffer_before_seconds: input.buffer_before_seconds,
            buffer_after_seconds: input.buffer_after_seconds,
            booking_horizon_days: input.booking_horizon_days,
            effective_from: input.effective_from,
            effective_until: input.effective_until,
            enabled: true,
            version,
            updated_at: env.ledger().timestamp(),
        };
        Self::put_schedule(&env, &schedule);

        env.events()
            .publish((symbol_short!("SCHSET"),), (participant, version));
        Ok(schedule)
    }

    /// Enable or disable a schedule without deleting it. Idempotent.
    pub fn set_enabled(env: Env, participant: Address, enabled: bool) -> Result<(), ContractError> {
        participant.require_auth();
        let mut schedule = Self::load_schedule(&env, &participant)?;
        if schedule.enabled == enabled {
            return Ok(());
        }
        schedule.enabled = enabled;
        schedule.version = schedule
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        schedule.updated_at = env.ledger().timestamp();
        Self::put_schedule(&env, &schedule);
        env.events()
            .publish((symbol_short!("SCHENB"),), (participant, enabled));
        Ok(())
    }

    pub fn get_schedule(
        env: Env,
        participant: Address,
    ) -> Result<AvailabilitySchedule, ContractError> {
        Self::load_schedule(&env, &participant)
    }

    /// Narrow cross-contract view: the schedule's guard rails, status, and
    /// version without the weekly windows or offset transitions.
    pub fn availability_summary(
        env: Env,
        participant: Address,
    ) -> Result<AvailabilitySummary, ContractError> {
        let schedule = Self::load_schedule(&env, &participant)?;
        Ok(AvailabilitySummary {
            participant: schedule.participant,
            enabled: schedule.enabled,
            version: schedule.version,
            lead_time_seconds: schedule.lead_time_seconds,
            buffer_before_seconds: schedule.buffer_before_seconds,
            buffer_after_seconds: schedule.buffer_after_seconds,
            booking_horizon_days: schedule.booking_horizon_days,
            effective_from: schedule.effective_from,
            effective_until: schedule.effective_until,
        })
    }

    // ── MENTOR-053 — exceptions and blackouts ────────────────────────────

    /// Add a UTC exception. The caller-supplied id makes submission
    /// idempotent: a duplicate is rejected, so a retry cannot create a second
    /// exception.
    pub fn add_exception(
        env: Env,
        participant: Address,
        input: ExceptionInput,
    ) -> Result<(), ContractError> {
        participant.require_auth();

        if input.from_ts == 0 || input.from_ts >= input.until_ts {
            return Err(ContractError::InvalidExceptionRange);
        }
        if input.until_ts - input.from_ts > MAX_EXCEPTION_SPAN_SECONDS {
            return Err(ContractError::ExceptionSpanTooLarge);
        }

        let key = DataKey::Exception(participant.clone(), input.id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ExceptionAlreadyExists);
        }

        let mut ids = Self::load_exception_ids(&env, &participant);
        if ids.len() >= MAX_EXCEPTIONS {
            return Err(ContractError::TooManyExceptions);
        }

        let exception = AvailabilityException {
            id: input.id.clone(),
            kind: input.kind,
            from_ts: input.from_ts,
            until_ts: input.until_ts,
            reference_hash: input.reference_hash,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &exception);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        ids.push_back(input.id.clone());
        Self::put_exception_ids(&env, &participant, &ids);

        env.events().publish(
            (symbol_short!("EXCSET"),),
            (participant, input.id, input.kind),
        );
        Ok(())
    }

    /// Remove an exception. Fails clearly if it does not exist.
    pub fn remove_exception(
        env: Env,
        participant: Address,
        id: BytesN<32>,
    ) -> Result<(), ContractError> {
        participant.require_auth();
        let key = DataKey::Exception(participant.clone(), id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::ExceptionNotFound);
        }
        env.storage().persistent().remove(&key);

        let ids = Self::load_exception_ids(&env, &participant);
        let mut remaining: Vec<BytesN<32>> = Vec::new(&env);
        for existing in ids.iter() {
            if existing != id {
                remaining.push_back(existing);
            }
        }
        Self::put_exception_ids(&env, &participant, &remaining);

        env.events()
            .publish((symbol_short!("EXCREM"),), (participant, id));
        Ok(())
    }

    pub fn get_exception(
        env: Env,
        participant: Address,
        id: BytesN<32>,
    ) -> Result<AvailabilityException, ContractError> {
        let key = DataKey::Exception(participant, id);
        let exception: AvailabilityException = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ExceptionNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(exception)
    }

    pub fn exception_ids(env: Env, participant: Address) -> Vec<BytesN<32>> {
        Self::load_exception_ids(&env, &participant)
    }

    // ── MENTOR-053 — DST-safe local -> UTC conversion ────────────────────

    fn offset_at(schedule: &AvailabilitySchedule, ts: u64) -> i64 {
        let mut offset = schedule.utc_offset_seconds;
        for change in schedule.offset_changes.iter() {
            if change.effective_from <= ts {
                offset = change.utc_offset_seconds;
            } else {
                break;
            }
        }
        offset
    }

    /// Convert a local wall-clock instant to UTC.
    ///
    /// For each distinct offset the schedule knows about, the candidate
    /// `local - offset` is kept only when that offset is actually in effect
    /// at the resulting instant. When two candidates are valid (the ambiguous
    /// hour when clocks fall back) the **earliest** one wins; when none is
    /// valid (the skipped hour when clocks spring forward) the local instant
    /// simply does not exist and `None` is returned.
    fn local_to_utc(schedule: &AvailabilitySchedule, local_ts: i64) -> Option<u64> {
        if local_ts < 0 {
            return None;
        }
        let count = schedule.offset_changes.len();
        let mut best: Option<u64> = None;
        for index in 0..=count {
            let candidate = if index == 0 {
                schedule.utc_offset_seconds
            } else {
                match schedule.offset_changes.get(index - 1) {
                    Some(change) => change.utc_offset_seconds,
                    None => break,
                }
            };
            let utc = local_ts - candidate;
            if utc < 0 {
                continue;
            }
            let utc_ts = utc as u64;
            if Self::offset_at(schedule, utc_ts) != candidate {
                continue;
            }
            best = match best {
                None => Some(utc_ts),
                Some(current) => Some(if utc_ts < current { utc_ts } else { current }),
            };
        }
        best
    }

    // ── MENTOR-053 — materialized UTC ranges ─────────────────────────────

    fn clip(start: u64, end: u64, lower: u64, upper: u64) -> Option<AvailableRange> {
        let clipped_start = if start > lower { start } else { lower };
        let clipped_end = if end < upper { end } else { upper };
        if clipped_end <= clipped_start {
            None
        } else {
            Some(AvailableRange {
                start_ts: clipped_start,
                end_ts: clipped_end,
            })
        }
    }

    /// Compute sorted, merged UTC ranges for `[start, end)`, including
    /// available exceptions and excluding unavailable ones.
    fn compute_ranges(
        env: &Env,
        schedule: &AvailabilitySchedule,
        exceptions: &Vec<AvailabilityException>,
        start: u64,
        end: u64,
    ) -> Vec<AvailableRange> {
        let mut merged: Vec<AvailableRange> = Vec::new(env);

        let first_day = start / SECONDS_PER_DAY;
        let last_day = (end - 1) / SECONDS_PER_DAY;
        let mut day = first_day;
        // The query span is bounded to 90 days, so this loop is bounded too.
        while day <= last_day {
            let day_start = day * SECONDS_PER_DAY;
            let day_end = day_start + SECONDS_PER_DAY;

            let mut candidates: Vec<AvailableRange> = Vec::new(env);

            // Weekly windows, considered for every local day that can map
            // into this UTC day.
            let local_min = day_start as i64 + MIN_UTC_OFFSET_SECONDS;
            let local_max = day_end as i64 + MAX_UTC_OFFSET_SECONDS;
            let first_local_day =
                local_min.div_euclid(SECONDS_PER_DAY as i64) * SECONDS_PER_DAY as i64;
            let last_local_day =
                (local_max - 1).div_euclid(SECONDS_PER_DAY as i64) * SECONDS_PER_DAY as i64;
            let mut local_day = first_local_day;
            while local_day <= last_local_day {
                let weekday = ((local_day.div_euclid(SECONDS_PER_DAY as i64)
                    + EPOCH_WEEKDAY_OFFSET)
                    % 7) as u32;
                for window in schedule.weekly.iter() {
                    if window.weekday != weekday {
                        continue;
                    }
                    let local_start =
                        local_day + (window.start_minute as i64) * SECONDS_PER_MINUTE as i64;
                    let local_end =
                        local_day + (window.end_minute as i64) * SECONDS_PER_MINUTE as i64;
                    let utc_start = match Self::local_to_utc(schedule, local_start) {
                        Some(value) => value,
                        None => continue,
                    };
                    let utc_end = match Self::local_to_utc(schedule, local_end) {
                        Some(value) => value,
                        None => continue,
                    };
                    if let Some(clipped) = Self::clip(utc_start, utc_end, day_start, day_end) {
                        if let Some(clipped) =
                            Self::clip(clipped.start_ts, clipped.end_ts, start, end)
                        {
                            candidates.push_back(clipped);
                        }
                    }
                }
                local_day += SECONDS_PER_DAY as i64;
            }

            // Extra availability exceptions are already in UTC.
            for exception in exceptions.iter() {
                if exception.kind != ExceptionKind::Available {
                    continue;
                }
                if let Some(clipped) =
                    Self::clip(exception.from_ts, exception.until_ts, day_start, day_end)
                {
                    if let Some(clipped) = Self::clip(clipped.start_ts, clipped.end_ts, start, end)
                    {
                        candidates.push_back(clipped);
                    }
                }
            }

            // Deterministic ordering within the day (insertion-free selection
            // sort; the candidate count is bounded by the weekly window cap
            // plus the exception cap).
            let count = candidates.len();
            for i in 0..count {
                let mut min_index = i;
                let mut min_value = match candidates.get(i) {
                    Some(value) => value,
                    None => continue,
                };
                for j in (i + 1)..count {
                    if let Some(value) = candidates.get(j) {
                        if value.start_ts < min_value.start_ts {
                            min_index = j;
                            min_value = value;
                        }
                    }
                }
                if min_index != i {
                    if let Some(left) = candidates.get(i) {
                        if let Some(right) = candidates.get(min_index) {
                            candidates.set(i, right);
                            candidates.set(min_index, left);
                        }
                    }
                }
            }

            // Merge overlapping or touching candidates and fold them into the
            // global result (day buckets are disjoint and ordered).
            let count = candidates.len();
            for index in 0..count {
                let candidate = match candidates.get(index) {
                    Some(value) => value,
                    None => continue,
                };
                match merged.last() {
                    Some(last) if candidate.start_ts <= last.end_ts => {
                        if candidate.end_ts > last.end_ts {
                            merged.set(
                                merged.len() - 1,
                                AvailableRange {
                                    start_ts: last.start_ts,
                                    end_ts: candidate.end_ts,
                                },
                            );
                        }
                    }
                    _ => merged.push_back(candidate),
                }
            }

            day += 1;
        }

        // Subtract unavailable exceptions from the merged ranges.
        let mut blocked: Vec<AvailabilityException> = Vec::new(env);
        for exception in exceptions.iter() {
            if exception.kind == ExceptionKind::Unavailable {
                blocked.push_back(exception);
            }
        }
        if blocked.is_empty() {
            return merged;
        }

        let mut result: Vec<AvailableRange> = Vec::new(env);
        for range in merged.iter() {
            let mut segments: Vec<AvailableRange> = Vec::new(env);
            segments.push_back(range);
            for blackout in blocked.iter() {
                let mut next: Vec<AvailableRange> = Vec::new(env);
                for segment in segments.iter() {
                    if blackout.until_ts <= segment.start_ts || blackout.from_ts >= segment.end_ts {
                        next.push_back(segment);
                        continue;
                    }
                    if blackout.from_ts > segment.start_ts {
                        next.push_back(AvailableRange {
                            start_ts: segment.start_ts,
                            end_ts: blackout.from_ts,
                        });
                    }
                    if blackout.until_ts < segment.end_ts {
                        next.push_back(AvailableRange {
                            start_ts: blackout.until_ts,
                            end_ts: segment.end_ts,
                        });
                    }
                }
                segments = next;
            }
            for segment in segments.iter() {
                result.push_back(segment);
            }
        }
        result
    }

    /// Clamp a requested interval to the schedule's effective range, lead
    /// time, and booking horizon.
    fn bookable_window(
        env: &Env,
        schedule: &AvailabilitySchedule,
        from_ts: u64,
        until_ts: u64,
    ) -> Option<(u64, u64)> {
        if !schedule.enabled {
            return None;
        }
        let now = env.ledger().timestamp();
        let earliest = now
            .checked_add(schedule.lead_time_seconds)
            .unwrap_or(u64::MAX);
        let horizon_end = now
            .checked_add(
                (schedule.booking_horizon_days as u64)
                    .checked_mul(SECONDS_PER_DAY)
                    .unwrap_or(u64::MAX),
            )
            .unwrap_or(u64::MAX);

        let mut start = from_ts;
        if start < schedule.effective_from {
            start = schedule.effective_from;
        }
        if start < earliest {
            start = earliest;
        }
        let mut end = until_ts;
        if end > horizon_end {
            end = horizon_end;
        }
        if schedule.effective_until != 0 && end > schedule.effective_until {
            end = schedule.effective_until;
        }
        if end <= start {
            None
        } else {
            Some((start, end))
        }
    }

    /// Materialized, sorted, non-overlapping UTC availability ranges for
    /// `[from_ts, until_ts)`, paginated after filtering.
    pub fn available_ranges(
        env: Env,
        participant: Address,
        from_ts: u64,
        until_ts: u64,
        page: u32,
        limit: u32,
    ) -> Result<Vec<AvailableRange>, ContractError> {
        if from_ts >= until_ts {
            return Err(ContractError::InvalidQueryRange);
        }
        if until_ts - from_ts > MAX_QUERY_SPAN_SECONDS {
            return Err(ContractError::QuerySpanTooLarge);
        }
        if limit == 0 || limit > MAX_PAGE_SIZE || page > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }

        let schedule = Self::load_schedule(&env, &participant)?;
        let (start, end) = match Self::bookable_window(&env, &schedule, from_ts, until_ts) {
            Some(window) => window,
            None => return Ok(Vec::new(&env)),
        };

        let exceptions = Self::load_exceptions(&env, &participant);
        let ranges = Self::compute_ranges(&env, &schedule, &exceptions, start, end);

        let skip = (page as u64) * (limit as u64);
        let mut out: Vec<AvailableRange> = Vec::new(&env);
        for (index, range) in ranges.iter().enumerate() {
            if (index as u64) < skip {
                continue;
            }
            if out.len() >= limit {
                break;
            }
            out.push_back(range);
        }
        Ok(out)
    }

    /// True when the whole `[start_ts, end_ts)` interval is available.
    pub fn is_available(
        env: Env,
        participant: Address,
        start_ts: u64,
        end_ts: u64,
    ) -> Result<bool, ContractError> {
        if start_ts >= end_ts {
            return Err(ContractError::InvalidQueryRange);
        }
        if end_ts - start_ts > MAX_QUERY_SPAN_SECONDS {
            return Err(ContractError::QuerySpanTooLarge);
        }
        let schedule = Self::load_schedule(&env, &participant)?;
        let (start, end) = match Self::bookable_window(&env, &schedule, start_ts, end_ts) {
            Some(window) => window,
            None => return Ok(false),
        };
        // A slot touching the requested boundary is fine; require full
        // coverage of the requested interval clamped to the bookable window.
        if start > start_ts || end < end_ts {
            return Ok(false);
        }

        let exceptions = Self::load_exceptions(&env, &participant);
        let ranges = Self::compute_ranges(&env, &schedule, &exceptions, start, end);
        for range in ranges.iter() {
            if range.start_ts <= start && range.end_ts >= end {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
