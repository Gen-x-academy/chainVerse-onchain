#![no_std]

//! Mentorship session-slot contract (work item MENTOR-055, issue #1251).
//!
//! Given two participants, a program's session policy, and the participant
//! availability held by `contracts/mentorship-availability`, this contract
//! computes deterministic candidate session slots and books them safely:
//!
//! - **Deterministic.** Candidate starts are aligned to a granularity grid
//!   measured from the Unix epoch, so the same inputs and ledger time always
//!   produce the same slots regardless of the queried range.
//! - **DST-safe.** All arithmetic is on absolute UTC instants; local-time
//!   resolution and DST handling stay in the availability contract, whose
//!   schedule version is echoed on every returned slot so a client can tell
//!   whether its candidate list is stale.
//! - **Capacity-aware.** A participant cannot be double-booked, buffers from
//!   their availability schedule are respected around existing bookings, and
//!   a program policy caps how many sessions a participant has per UTC day.
//! - **Bounded and paginated.** `find_slots` only examines a bounded number
//!   of candidate instants and returns one page, so a wide or busy schedule
//!   cannot make the call unbounded.
//! - **Stale candidates are revalidated at booking.** `book_slot` re-reads
//!   availability, blackouts, existing bookings, and capacity and refuses a
//!   candidate that has since become invalid.
//!
//! Privacy boundary: only program ids, participant addresses, instants, and
//! booking state are stored. Session topic, notes, or any participant content
//! stays off-chain. See `contracts/docs/mentorship-slots.md`.

// The generated contract client adds `&self` to every entry point, so a
// multi-argument query or booking call trips this lint even though the ABI is
// intentionally explicit about its inputs.
#![allow(clippy::too_many_arguments)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    IntoVal, Symbol, Val, Vec,
};

/// Mirror of `mentorship_availability::AvailabilitySummary`.
///
/// The availability contract is a *dev*-dependency only: linking another
/// `#[contractimpl]` crate into this contract's WASM would duplicate that
/// crate's exported entry points (wasm-ld rejects the duplicate symbols). This
/// contract therefore decodes the availability ABI into local mirror types,
/// which **must stay field-for-field in sync** with
/// `contracts/mentorship-availability`. Field order is part of the encoding; a
/// mismatch surfaces as `AvailabilityUnavailable` at runtime, and the
/// integration tests here exercise every mirrored field through a real
/// availability instance.
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

/// Mirror of `mentorship_availability::AvailableRange`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailableRange {
    pub start_ts: u64,
    pub end_ts: u64,
}

/// Error type the availability contract returns; decoded only to map any
/// failure onto `AvailabilityUnavailable`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AvailabilityError {
    ScheduleNotFound = 1,
    InvalidWeeklyWindow = 2,
    TooManyWindows = 3,
    WindowsNotOrdered = 4,
    InvalidEffectiveRange = 5,
    InvalidLeadTime = 6,
    InvalidBuffer = 7,
    InvalidBookingHorizon = 8,
    TooManyOffsetChanges = 9,
    InvalidOffsetChange = 10,
    InvalidOffset = 11,
    TooManyExceptions = 12,
    ExceptionAlreadyExists = 13,
    ExceptionNotFound = 14,
    InvalidExceptionRange = 15,
    ExceptionSpanTooLarge = 16,
    InvalidQueryRange = 17,
    QuerySpanTooLarge = 18,
    InvalidPage = 19,
    VersionOverflow = 20,
    InvalidTimezoneRef = 21,
}

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const SECONDS_PER_DAY: u64 = 86_400;

/// Bounded storage: at most this many blackouts in total.
const MAX_BLACKOUTS: u32 = 64;
/// Bounded storage: at most this many bookings per participant.
const MAX_BOOKINGS_PER_PARTICIPANT: u32 = 128;
/// One search may cover at most 31 days.
const MAX_SEARCH_SPAN_SECONDS: u64 = 31 * SECONDS_PER_DAY;
/// Page size for returned slots.
const MAX_PAGE_SIZE: u32 = 64;
const MAX_PAGE: u32 = 64;
/// Candidate instants examined in a single search.
const MAX_COLLECTED_CANDIDATES: u64 = 256;
/// A blackout may cover at most one year.
const MAX_BLACKOUT_SPAN_SECONDS: u64 = 366 * SECONDS_PER_DAY;

const MIN_SESSION_SECONDS: u64 = 300;
const MAX_SESSION_SECONDS: u64 = 8 * 3_600;
const MIN_GRANULARITY_SECONDS: u64 = 60;
const MAX_GRANULARITY_SECONDS: u64 = 3_600;
const MAX_SESSIONS_PER_DAY: u32 = 16;

/// Page size used when reading participant availability.
const AVAILABILITY_PAGE_SIZE: u32 = 64;
/// Bounded number of availability pages read per search.
const MAX_AVAILABILITY_PAGES: u32 = 4;
const MAX_AVAILABILITY_RANGES: u32 = 192;

/// A program id of all zeroes targets every program (used by blackouts).
fn global_program(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The caller is not one of the two booking parties.
    NotAuthorized = 4,
    ProgramPolicyNotFound = 5,
    /// Duration or granularity out of range, or duration is not a multiple of
    /// the granularity.
    InvalidSessionPolicy = 6,
    InvalidSessionsPerDay = 7,
    InvalidBlackoutRange = 8,
    BlackoutAlreadyExists = 9,
    BlackoutNotFound = 10,
    TooManyBlackouts = 11,
    BookingNotFound = 12,
    BookingAlreadyExists = 13,
    BookingNotActive = 14,
    /// A mentor cannot book a session with themselves.
    InvalidParties = 15,
    InvalidQueryRange = 16,
    QuerySpanTooLarge = 17,
    InvalidPage = 18,
    /// The requested page would require examining more candidates than the
    /// bounded search allows; narrow the range.
    TooManyCandidates = 19,
    /// The participant already holds the maximum number of bookings.
    ParticipantScheduleFull = 20,
    /// The availability contract could not be read.
    AvailabilityUnavailable = 21,
    /// The slot is not bookable: outside availability, lead time, horizon,
    /// buffers, blackout, or capacity.
    SlotUnavailable = 22,
    /// The slot overlaps a blackout period.
    BlackoutOverlap = 23,
    VersionOverflow = 24,
    /// Booking ids must not be all zeroes.
    InvalidBookingId = 25,
    /// Availability for a participant is too fragmented for the bounded
    /// search; narrow the range.
    AvailabilityTooFragmented = 26,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Address of the mentorship-availability contract.
    Availability,
    /// Session policy of a program.
    ProgramPolicy(BytesN<32>),
    /// Blackout ids, in creation order.
    Blackouts,
    /// One blackout.
    Blackout(BytesN<32>),
    /// Booking ids of a participant, in creation order.
    ParticipantBookings(Address),
    /// One booking.
    Booking(BytesN<32>),
}

/// Session policy for a program: how long a session is, how candidate starts
/// are spaced, and how many sessions a participant may hold per UTC day.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramPolicy {
    pub program_id: BytesN<32>,
    pub duration_seconds: u64,
    pub granularity_seconds: u64,
    pub max_sessions_per_day: u32,
    pub version: u32,
    pub updated_at: u64,
}

/// A period during which no session may be booked. A zero `program_id`
/// applies to every program.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Blackout {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub from_ts: u64,
    pub until_ts: u64,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookingState {
    Active,
    Cancelled,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Booking {
    pub id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub program_id: BytesN<32>,
    pub start_ts: u64,
    pub end_ts: u64,
    pub state: BookingState,
    pub created_at: u64,
}

/// A candidate session slot. The availability versions let a client detect
/// that its candidate list predates a schedule change; `book_slot` revalidates
/// regardless.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slot {
    pub start_ts: u64,
    pub end_ts: u64,
    pub mentor_availability_version: u32,
    pub mentee_availability_version: u32,
}

#[contract]
pub struct MentorshipSlotsContract;

#[contractimpl]
impl MentorshipSlotsContract {
    /// Initialize the contract admin and the availability contract it reads.
    pub fn initialize(
        env: Env,
        admin: Address,
        availability: Address,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Availability, &availability);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn availability_address(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Availability)
            .ok_or(ContractError::NotInitialized)
    }

    /// Admin-only: point the contract at a new availability deployment.
    pub fn set_availability_contract(
        env: Env,
        admin: Address,
        availability: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Availability, &availability);
        env.events()
            .publish((symbol_short!("AVAILSET"),), (availability,));
        Ok(())
    }

    // ── cross-contract reads ─────────────────────────────────────────────

    fn read_summary(
        env: &Env,
        participant: &Address,
    ) -> Result<AvailabilitySummary, ContractError> {
        let contract = Self::availability_address(env)?;
        let mut args: Vec<Val> = Vec::new(env);
        args.push_back(participant.into_val(env));
        match env.try_invoke_contract::<AvailabilitySummary, AvailabilityError>(
            &contract,
            &Symbol::new(env, "availability_summary"),
            args,
        ) {
            Ok(Ok(summary)) => Ok(summary),
            _ => Err(ContractError::AvailabilityUnavailable),
        }
    }

    fn read_ranges(
        env: &Env,
        participant: &Address,
        from_ts: u64,
        until_ts: u64,
    ) -> Result<Vec<AvailableRange>, ContractError> {
        let contract = Self::availability_address(env)?;
        let mut ranges: Vec<AvailableRange> = Vec::new(env);
        let mut page: u32 = 0;
        loop {
            let mut args: Vec<Val> = Vec::new(env);
            args.push_back(participant.into_val(env));
            args.push_back(from_ts.into_val(env));
            args.push_back(until_ts.into_val(env));
            args.push_back(page.into_val(env));
            args.push_back(AVAILABILITY_PAGE_SIZE.into_val(env));
            let page_ranges = match env
                .try_invoke_contract::<Vec<AvailableRange>, AvailabilityError>(
                    &contract,
                    &Symbol::new(env, "available_ranges"),
                    args,
                ) {
                Ok(Ok(value)) => value,
                _ => return Err(ContractError::AvailabilityUnavailable),
            };
            let count = page_ranges.len();
            for range in page_ranges.iter() {
                if ranges.len() >= MAX_AVAILABILITY_RANGES {
                    return Err(ContractError::AvailabilityTooFragmented);
                }
                ranges.push_back(range);
            }
            if count < AVAILABILITY_PAGE_SIZE {
                return Ok(ranges);
            }
            page += 1;
            if page >= MAX_AVAILABILITY_PAGES {
                return Err(ContractError::AvailabilityTooFragmented);
            }
        }
    }

    // ── storage helpers ──────────────────────────────────────────────────

    fn put_policy(env: &Env, policy: &ProgramPolicy) {
        let key = DataKey::ProgramPolicy(policy.program_id.clone());
        env.storage().persistent().set(&key, policy);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_policy(env: &Env, program_id: &BytesN<32>) -> Result<ProgramPolicy, ContractError> {
        let key = DataKey::ProgramPolicy(program_id.clone());
        let policy: ProgramPolicy = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramPolicyNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(policy)
    }

    fn load_blackout_ids(env: &Env) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::Blackouts)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn load_blackouts(env: &Env) -> Vec<Blackout> {
        let ids = Self::load_blackout_ids(env);
        let mut out: Vec<Blackout> = Vec::new(env);
        for id in ids.iter() {
            let key = DataKey::Blackout(id);
            if let Some(blackout) = env.storage().persistent().get::<DataKey, Blackout>(&key) {
                out.push_back(blackout);
            }
        }
        out
    }

    fn load_booking_ids(env: &Env, participant: &Address) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::ParticipantBookings(participant.clone()))
            .unwrap_or_else(|| Vec::new(env))
    }

    fn put_booking_ids(env: &Env, participant: &Address, ids: &Vec<BytesN<32>>) {
        let key = DataKey::ParticipantBookings(participant.clone());
        env.storage().persistent().set(&key, ids);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_booking(env: &Env, id: &BytesN<32>) -> Result<Booking, ContractError> {
        let key = DataKey::Booking(id.clone());
        let booking: Booking = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BookingNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(booking)
    }

    fn put_booking(env: &Env, booking: &Booking) {
        let key = DataKey::Booking(booking.id.clone());
        env.storage().persistent().set(&key, booking);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn participant_bookings(env: &Env, participant: &Address) -> Vec<Booking> {
        let ids = Self::load_booking_ids(env, participant);
        let mut out: Vec<Booking> = Vec::new(env);
        for id in ids.iter() {
            let key = DataKey::Booking(id);
            if let Some(booking) = env.storage().persistent().get::<DataKey, Booking>(&key) {
                out.push_back(booking);
            }
        }
        out
    }

    // ── program policies ─────────────────────────────────────────────────

    /// Admin-only: publish the session policy for a program. Versions increase
    /// so clients can detect a policy change.
    pub fn set_program_policy(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        duration_seconds: u64,
        granularity_seconds: u64,
        max_sessions_per_day: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if !(MIN_SESSION_SECONDS..=MAX_SESSION_SECONDS).contains(&duration_seconds)
            || !(MIN_GRANULARITY_SECONDS..=MAX_GRANULARITY_SECONDS).contains(&granularity_seconds)
            || duration_seconds % granularity_seconds != 0
        {
            return Err(ContractError::InvalidSessionPolicy);
        }
        if max_sessions_per_day == 0 || max_sessions_per_day > MAX_SESSIONS_PER_DAY {
            return Err(ContractError::InvalidSessionsPerDay);
        }

        let version = match env
            .storage()
            .persistent()
            .get::<DataKey, ProgramPolicy>(&DataKey::ProgramPolicy(program_id.clone()))
        {
            Some(existing) => existing
                .version
                .checked_add(1)
                .ok_or(ContractError::VersionOverflow)?,
            None => 1,
        };
        let policy = ProgramPolicy {
            program_id: program_id.clone(),
            duration_seconds,
            granularity_seconds,
            max_sessions_per_day,
            version,
            updated_at: env.ledger().timestamp(),
        };
        Self::put_policy(&env, &policy);

        env.events().publish(
            (symbol_short!("PROGPOL"),),
            (program_id, version, duration_seconds),
        );
        Ok(())
    }

    pub fn get_program_policy(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramPolicy, ContractError> {
        Self::load_policy(&env, &program_id)
    }

    // ── blackouts ────────────────────────────────────────────────────────

    /// Admin-only: block bookings in a UTC range. A zero program id blocks
    /// every program.
    pub fn set_blackout(
        env: Env,
        admin: Address,
        id: BytesN<32>,
        program_id: BytesN<32>,
        from_ts: u64,
        until_ts: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if from_ts == 0 || from_ts >= until_ts {
            return Err(ContractError::InvalidBlackoutRange);
        }
        if until_ts - from_ts > MAX_BLACKOUT_SPAN_SECONDS {
            return Err(ContractError::InvalidBlackoutRange);
        }
        let key = DataKey::Blackout(id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::BlackoutAlreadyExists);
        }

        let mut ids = Self::load_blackout_ids(&env);
        if ids.len() >= MAX_BLACKOUTS {
            return Err(ContractError::TooManyBlackouts);
        }

        let blackout = Blackout {
            id: id.clone(),
            program_id: program_id.clone(),
            from_ts,
            until_ts,
            created_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &blackout);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        ids.push_back(id.clone());
        env.storage().persistent().set(&DataKey::Blackouts, &ids);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Blackouts, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("BLKSET"),),
            (id, program_id, from_ts, until_ts),
        );
        Ok(())
    }

    /// Admin-only: remove a blackout.
    pub fn remove_blackout(env: Env, admin: Address, id: BytesN<32>) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Blackout(id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::BlackoutNotFound);
        }
        env.storage().persistent().remove(&key);

        let ids = Self::load_blackout_ids(&env);
        let mut remaining: Vec<BytesN<32>> = Vec::new(&env);
        for existing in ids.iter() {
            if existing != id {
                remaining.push_back(existing);
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::Blackouts, &remaining);
        env.storage()
            .persistent()
            .extend_ttl(&DataKey::Blackouts, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish((symbol_short!("BLKREM"),), (id,));
        Ok(())
    }

    pub fn get_blackout(env: Env, id: BytesN<32>) -> Result<Blackout, ContractError> {
        let key = DataKey::Blackout(id);
        let blackout: Blackout = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BlackoutNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(blackout)
    }

    pub fn blackout_ids(env: Env) -> Vec<BytesN<32>> {
        Self::load_blackout_ids(&env)
    }

    // ── availability intersection ────────────────────────────────────────

    fn ranges_cover(ranges: &Vec<AvailableRange>, start_ts: u64, end_ts: u64) -> bool {
        for range in ranges.iter() {
            if range.start_ts <= start_ts && range.end_ts >= end_ts {
                return true;
            }
        }
        false
    }

    fn blackout_overlaps(
        env: &Env,
        blackouts: &Vec<Blackout>,
        program_id: &BytesN<32>,
        start_ts: u64,
        end_ts: u64,
    ) -> bool {
        let global = global_program(env);
        for blackout in blackouts.iter() {
            if blackout.program_id != *program_id && blackout.program_id != global {
                continue;
            }
            if blackout.from_ts < end_ts && blackout.until_ts > start_ts {
                return true;
            }
        }
        false
    }

    /// True when `bookings` already holds an active booking that overlaps
    /// `[start_ts, end_ts)` once the participant's buffers are applied.
    fn bookings_conflict(
        bookings: &Vec<Booking>,
        start_ts: u64,
        end_ts: u64,
        buffer_before: u64,
        buffer_after: u64,
    ) -> bool {
        for booking in bookings.iter() {
            if booking.state != BookingState::Active {
                continue;
            }
            let guarded_start = booking.start_ts.saturating_sub(buffer_before);
            let guarded_end = booking.end_ts.saturating_add(buffer_after);
            if guarded_start < end_ts && guarded_end > start_ts {
                return true;
            }
        }
        false
    }

    /// Number of active bookings held on the UTC day that contains `instant`.
    fn sessions_on_day(bookings: &Vec<Booking>, instant: u64) -> u32 {
        let day_start = (instant / SECONDS_PER_DAY) * SECONDS_PER_DAY;
        let day_end = day_start + SECONDS_PER_DAY;
        let mut count: u32 = 0;
        for booking in bookings.iter() {
            if booking.state == BookingState::Active
                && booking.start_ts >= day_start
                && booking.start_ts < day_end
            {
                count += 1;
            }
        }
        count
    }

    fn validate_participant_slot(
        env: &Env,
        summary: &AvailabilitySummary,
        bookings: &Vec<Booking>,
        start_ts: u64,
        end_ts: u64,
        ranges: &Vec<AvailableRange>,
        blackouts: &Vec<Blackout>,
        program_id: &BytesN<32>,
        max_sessions_per_day: u32,
    ) -> Result<(), ContractError> {
        if !summary.enabled {
            return Err(ContractError::SlotUnavailable);
        }
        if !Self::ranges_cover(ranges, start_ts, end_ts) {
            return Err(ContractError::SlotUnavailable);
        }
        if Self::blackout_overlaps(env, blackouts, program_id, start_ts, end_ts) {
            return Err(ContractError::BlackoutOverlap);
        }
        if Self::bookings_conflict(
            bookings,
            start_ts,
            end_ts,
            summary.buffer_before_seconds,
            summary.buffer_after_seconds,
        ) {
            return Err(ContractError::SlotUnavailable);
        }
        if Self::sessions_on_day(bookings, start_ts) >= max_sessions_per_day {
            return Err(ContractError::SlotUnavailable);
        }
        Ok(())
    }

    /// Validate a concrete slot for both participants, re-reading availability
    /// so a change since the candidate was computed is caught.
    fn validate_slot(
        env: &Env,
        mentor: &Address,
        mentee: &Address,
        policy: &ProgramPolicy,
        start_ts: u64,
    ) -> Result<u64, ContractError> {
        let mentor_summary = Self::read_summary(env, mentor)?;
        let mentee_summary = Self::read_summary(env, mentee)?;
        let end_ts = start_ts
            .checked_add(policy.duration_seconds)
            .ok_or(ContractError::SlotUnavailable)?;

        let now = env.ledger().timestamp();
        let lead_time = if mentor_summary.lead_time_seconds > mentee_summary.lead_time_seconds {
            mentor_summary.lead_time_seconds
        } else {
            mentee_summary.lead_time_seconds
        };
        if start_ts
            < now
                .checked_add(lead_time)
                .ok_or(ContractError::SlotUnavailable)?
        {
            return Err(ContractError::SlotUnavailable);
        }

        let mentor_horizon = (mentor_summary.booking_horizon_days as u64)
            .checked_mul(SECONDS_PER_DAY)
            .ok_or(ContractError::SlotUnavailable)?;
        let mentee_horizon = (mentee_summary.booking_horizon_days as u64)
            .checked_mul(SECONDS_PER_DAY)
            .ok_or(ContractError::SlotUnavailable)?;
        let horizon = if mentor_horizon < mentee_horizon {
            mentor_horizon
        } else {
            mentee_horizon
        };
        if end_ts
            > now
                .checked_add(horizon)
                .ok_or(ContractError::SlotUnavailable)?
        {
            return Err(ContractError::SlotUnavailable);
        }

        let mentor_ranges = Self::read_ranges(env, mentor, start_ts, end_ts)?;
        let mentee_ranges = Self::read_ranges(env, mentee, start_ts, end_ts)?;
        let blackouts = Self::load_blackouts(env);
        let mentor_bookings = Self::participant_bookings(env, mentor);
        let mentee_bookings = Self::participant_bookings(env, mentee);

        Self::validate_participant_slot(
            env,
            &mentor_summary,
            &mentor_bookings,
            start_ts,
            end_ts,
            &mentor_ranges,
            &blackouts,
            &policy.program_id,
            policy.max_sessions_per_day,
        )?;
        Self::validate_participant_slot(
            env,
            &mentee_summary,
            &mentee_bookings,
            start_ts,
            end_ts,
            &mentee_ranges,
            &blackouts,
            &policy.program_id,
            policy.max_sessions_per_day,
        )?;
        Ok(end_ts)
    }

    // ── MENTOR-055 — candidate slots ─────────────────────────────────────

    /// Deterministic candidate slots for `[from_ts, until_ts)`, paginated.
    ///
    /// The search is bounded: it intersects the two participants' available
    /// ranges, steps a granularity grid, and stops after
    /// `MAX_COLLECTED_CANDIDATES` instants, so a wide or very free schedule
    /// returns `TooManyCandidates` instead of an unbounded result.
    pub fn find_slots(
        env: Env,
        mentor: Address,
        mentee: Address,
        program_id: BytesN<32>,
        from_ts: u64,
        until_ts: u64,
        page: u32,
        limit: u32,
    ) -> Result<Vec<Slot>, ContractError> {
        if mentor == mentee {
            return Err(ContractError::InvalidParties);
        }
        if from_ts >= until_ts {
            return Err(ContractError::InvalidQueryRange);
        }
        if until_ts - from_ts > MAX_SEARCH_SPAN_SECONDS {
            return Err(ContractError::QuerySpanTooLarge);
        }
        if limit == 0 || limit > MAX_PAGE_SIZE || page > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let target = (page as u64 + 1) * (limit as u64);
        if target > MAX_COLLECTED_CANDIDATES {
            return Err(ContractError::InvalidPage);
        }

        let policy = Self::load_policy(&env, &program_id)?;
        let mentor_summary = Self::read_summary(&env, &mentor)?;
        let mentee_summary = Self::read_summary(&env, &mentee)?;

        // Clamp to the intersection of both participants' guard rails.
        let now = env.ledger().timestamp();
        let lead_time = if mentor_summary.lead_time_seconds > mentee_summary.lead_time_seconds {
            mentor_summary.lead_time_seconds
        } else {
            mentee_summary.lead_time_seconds
        };
        let mentor_horizon = (mentor_summary.booking_horizon_days as u64) * SECONDS_PER_DAY;
        let mentee_horizon = (mentee_summary.booking_horizon_days as u64) * SECONDS_PER_DAY;
        let horizon = if mentor_horizon < mentee_horizon {
            mentor_horizon
        } else {
            mentee_horizon
        };

        let mut start = from_ts;
        let earliest = now.checked_add(lead_time).unwrap_or(u64::MAX);
        if start < earliest {
            start = earliest;
        }
        let mut end = until_ts;
        let latest = now.checked_add(horizon).unwrap_or(u64::MAX);
        if end > latest {
            end = latest;
        }
        let mut slots: Vec<Slot> = Vec::new(&env);
        if end <= start {
            return Ok(slots);
        }

        let mentor_ranges = Self::read_ranges(&env, &mentor, start, end)?;
        let mentee_ranges = Self::read_ranges(&env, &mentee, start, end)?;
        let blackouts = Self::load_blackouts(&env);
        let mentor_bookings = Self::participant_bookings(&env, &mentor);
        let mentee_bookings = Self::participant_bookings(&env, &mentee);

        let granularity = policy.granularity_seconds;
        let duration = policy.duration_seconds;
        let mut examined: u64 = 0;
        let mut collected: u64 = 0;
        let skip = (page as u64) * (limit as u64);

        let mut i: u32 = 0;
        let mut j: u32 = 0;
        while i < mentor_ranges.len() && j < mentee_ranges.len() {
            let mentor_range = match mentor_ranges.get(i) {
                Some(value) => value,
                None => break,
            };
            let mentee_range = match mentee_ranges.get(j) {
                Some(value) => value,
                None => break,
            };

            let overlap_start = if mentor_range.start_ts > mentee_range.start_ts {
                mentor_range.start_ts
            } else {
                mentee_range.start_ts
            };
            let overlap_end = if mentor_range.end_ts < mentee_range.end_ts {
                mentor_range.end_ts
            } else {
                mentee_range.end_ts
            };

            if overlap_start < overlap_end {
                // Align candidate starts to the epoch-based granularity grid.
                let mut candidate = overlap_start.div_ceil(granularity) * granularity;
                while candidate + duration <= overlap_end {
                    examined += 1;
                    if examined > MAX_COLLECTED_CANDIDATES * 8 {
                        return Err(ContractError::TooManyCandidates);
                    }
                    if Self::blackout_overlaps(
                        &env,
                        &blackouts,
                        &policy.program_id,
                        candidate,
                        candidate + duration,
                    ) || Self::bookings_conflict(
                        &mentor_bookings,
                        candidate,
                        candidate + duration,
                        mentor_summary.buffer_before_seconds,
                        mentor_summary.buffer_after_seconds,
                    ) || Self::bookings_conflict(
                        &mentee_bookings,
                        candidate,
                        candidate + duration,
                        mentee_summary.buffer_before_seconds,
                        mentee_summary.buffer_after_seconds,
                    ) || Self::sessions_on_day(&mentor_bookings, candidate)
                        >= policy.max_sessions_per_day
                        || Self::sessions_on_day(&mentee_bookings, candidate)
                            >= policy.max_sessions_per_day
                    {
                        candidate += granularity;
                        continue;
                    }

                    if collected >= skip && slots.len() < limit {
                        slots.push_back(Slot {
                            start_ts: candidate,
                            end_ts: candidate + duration,
                            mentor_availability_version: mentor_summary.version,
                            mentee_availability_version: mentee_summary.version,
                        });
                    }
                    collected += 1;
                    if collected >= target {
                        return Ok(slots);
                    }
                    candidate += granularity;
                }
            }

            if mentor_range.end_ts < mentee_range.end_ts {
                i += 1;
            } else {
                j += 1;
            }
        }

        Ok(slots)
    }

    /// True when `start_ts` can be booked for this pair right now, revalidating
    /// every constraint (availability, lead time, horizon, blackout, buffers,
    /// and capacity).
    pub fn is_slot_bookable(
        env: Env,
        mentor: Address,
        mentee: Address,
        program_id: BytesN<32>,
        start_ts: u64,
    ) -> Result<bool, ContractError> {
        if mentor == mentee {
            return Err(ContractError::InvalidParties);
        }
        let policy = Self::load_policy(&env, &program_id)?;
        Ok(Self::validate_slot(&env, &mentor, &mentee, &policy, start_ts).is_ok())
    }

    // ── MENTOR-055 — booking with revalidation ───────────────────────────

    /// Book a session for both participants.
    ///
    /// Both parties must authorize; the caller must be one of them. The slot
    /// is revalidated at booking time, so a candidate computed earlier is
    /// refused with `SlotUnavailable` or `BlackoutOverlap` if it went stale.
    /// Re-submitting the same booking id with the same slot is a no-op.
    pub fn book_slot(
        env: Env,
        caller: Address,
        booking_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
        program_id: BytesN<32>,
        start_ts: u64,
    ) -> Result<u64, ContractError> {
        if caller != mentor && caller != mentee {
            return Err(ContractError::NotAuthorized);
        }
        if mentor == mentee {
            return Err(ContractError::InvalidParties);
        }
        if booking_id == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidBookingId);
        }
        let policy = Self::load_policy(&env, &program_id)?;

        // Idempotent retry of an accepted booking.
        let existing_key = DataKey::Booking(booking_id.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, Booking>(&existing_key)
        {
            if existing.state == BookingState::Active
                && existing.mentor == mentor
                && existing.mentee == mentee
                && existing.program_id == program_id
                && existing.start_ts == start_ts
            {
                return Ok(existing.end_ts);
            }
            return Err(ContractError::BookingAlreadyExists);
        }

        mentor.require_auth();
        mentee.require_auth();

        let end_ts = Self::validate_slot(&env, &mentor, &mentee, &policy, start_ts)?;

        let booking = Booking {
            id: booking_id.clone(),
            mentor: mentor.clone(),
            mentee: mentee.clone(),
            program_id: program_id.clone(),
            start_ts,
            end_ts,
            state: BookingState::Active,
            created_at: env.ledger().timestamp(),
        };
        Self::put_booking(&env, &booking);

        for participant in [mentor.clone(), mentee.clone()] {
            let mut ids = Self::load_booking_ids(&env, &participant);
            if ids.len() >= MAX_BOOKINGS_PER_PARTICIPANT {
                return Err(ContractError::ParticipantScheduleFull);
            }
            ids.push_back(booking_id.clone());
            Self::put_booking_ids(&env, &participant, &ids);
        }

        env.events().publish(
            (symbol_short!("BOOKED"),),
            (booking_id, mentor, mentee, start_ts),
        );
        Ok(end_ts)
    }

    /// Cancel a booking. Either party may cancel; cancelling twice fails
    /// clearly, and a cancelled booking frees its capacity immediately.
    pub fn cancel_booking(
        env: Env,
        caller: Address,
        booking_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let mut booking = Self::load_booking(&env, &booking_id)?;
        if caller != booking.mentor && caller != booking.mentee {
            return Err(ContractError::NotAuthorized);
        }
        if booking.state != BookingState::Active {
            return Err(ContractError::BookingNotActive);
        }
        booking.state = BookingState::Cancelled;
        Self::put_booking(&env, &booking);

        env.events().publish(
            (symbol_short!("BOOKCAN"),),
            (booking_id, booking.mentor, booking.mentee),
        );
        Ok(())
    }

    pub fn get_booking(env: Env, booking_id: BytesN<32>) -> Result<Booking, ContractError> {
        Self::load_booking(&env, &booking_id)
    }

    /// A participant's booking ids, in creation order and paginated.
    pub fn booking_ids(
        env: Env,
        participant: Address,
        page: u32,
        limit: u32,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        if limit == 0 || limit > MAX_PAGE_SIZE || page > 512 {
            return Err(ContractError::InvalidPage);
        }
        let ids = Self::load_booking_ids(&env, &participant);
        let skip = (page as u64) * (limit as u64);
        let mut out: Vec<BytesN<32>> = Vec::new(&env);
        for (index, id) in ids.iter().enumerate() {
            if (index as u64) < skip {
                continue;
            }
            if out.len() >= limit {
                break;
            }
            out.push_back(id);
        }
        Ok(out)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
