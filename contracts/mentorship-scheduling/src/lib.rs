#![no_std]

//! Mentorship session booking — issue #1252 (**MENTOR-056**, Scheduling track).
//!
//! ChainVerse advertises 1-on-1 mentorship with smart-contract-backed
//! payments. This contract is the on-chain primitive that turns an active
//! mentorship agreement into concrete, confirmable sessions.
//!
//! Scope of this pass (MENTOR-056):
//!
//! - **Book from an active agreement.** Only the mentor and mentee recorded
//!   on a registered, active agreement may book sessions under it. The
//!   agreement anchor is registered by a registrar address (admin-set,
//!   deny-by-default) so the canonical agreements contract (MENTOR-045/046)
//!   can be handed the registrar role when it lands.
//! - **No double-booking.** Before a session is stored, the proposed slot is
//!   checked for overlap against every existing session of *both*
//!   participants. Back-to-back sessions (end == next start) are allowed.
//! - **Permissions and lead time enforced.** Booking requires the caller to
//!   be a participant of the agreement (`require_auth`), the start time must
//!   be in the future and at least the configured lead time away.
//! - **Confirmations in local time plus UTC.** Sessions store the canonical
//!   UTC start plus a bounded display offset, and both participants confirm
//!   before a session becomes `Confirmed`.
//! - **One canonical event per booking.** A successful `book_session`
//!   publishes exactly one `BOOKED` event carrying the scheduling facts.
//! - **Safe retries.** Booking takes an idempotency key; retrying the same
//!   payload returns the original session id without duplicating state or
//!   events, while reusing the key for a different payload fails with a
//!   stable `IdempotencyConflict`.
//! - **Bounded storage.** Each participant's session index is capped
//!   (`MAX_SESSIONS_PER_PARTICIPANT`), so no record grows without bound.
//!
//! Scope of the second pass (MENTOR-057, #1253) — rescheduling:
//!
//! - **Propose or apply time changes under notice policies.** Either
//!   participant proposes a new slot for an existing session. A proposal
//!   is only accepted while the session start is still at least the
//!   admin-configured notice period away; a session already inside the
//!   notice window can no longer be moved (`RescheduleNoticeNotMet`).
//! - **Participant approval.** The proposal is created by one participant
//!   (recorded as an implicit approval) and only the *other* participant
//!   can approve it — a party cannot unilaterally move a shared meeting.
//!   Declining is likewise reserved to the counterparty.
//! - **Reason visibility.** The public `reason` is a blame-free code; the
//!   free-text explanation stays off-chain behind `reason_hash`. Events
//!   carry only the code, never the commitment.
//! - **Conflict recheck.** The new slot is checked against both
//!   participants at proposal time and *again* at approval time, so a
//!   booking made in between cannot create a double-booking.
//! - **Original time remains traceable.** Every applied change appends a
//!   bounded `ScheduleChange` entry to the session, and a monotonically
//!   increasing `revision` makes stale approvals fail
//!   (`StaleRescheduleApproval`) instead of silently moving a session that
//!   already changed.
//! - **Stale approvals fail.** A proposal expires after the configured
//!   approval window (capped at the session start); approval afterwards
//!   fails with `RescheduleExpired`.
//! - **Payment consequences are previewed.** Each proposal carries a
//!   `PaymentImpact` derived from the billable-minute delta, and
//!   `reschedule_payment_impact` previews the same numbers before a
//!   proposal exists. This contract never moves funds — the preview tells
//!   the settlement contract (MENTOR-070/071) whether an adjustment is
//!   needed.
//! - **Repeated requests are safe.** Proposing with the same proposal id
//!   and payload returns the existing proposal; approving an already
//!   applied proposal returns the current session without a duplicate
//!   event. Only one open proposal per session is allowed.
//! - **Calendar updates converge.** Applying a change mutates the session
//!   in place (same id, same participant indexes) and resets both
//!   confirmations, so every calendar view that derives from
//!   `session_times`/`get_session` converges on the new slot.
//!
//! Privacy boundary: nothing user-authored is stored on-chain. Meetings are
//! referenced by a `location_ref` commitment and agendas by an `agenda_hash`;
//! the readable facts are addresses, time, duration, mode and status. See
//! `contracts/docs/mentorship-scheduling.md` for the storage layout, event
//! schema, off-chain boundary, and rollout notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling
// scholarship/mentorship contract convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1252 — bounded storage: a participant may hold at most this many
/// booked sessions, so the per-participant index cannot grow without bound.
const MAX_SESSIONS_PER_PARTICIPANT: u32 = 64;

/// #1252 — a session must be a usable meeting: at least 15 minutes and at
/// most 8 hours.
const MIN_DURATION_MINUTES: u32 = 15;
const MAX_DURATION_MINUTES: u32 = 480;

/// #1252 — bounds on the local display offset (UTC-12 .. UTC+14), in
/// minutes east of UTC. Used only to render a session in a participant's
/// local time; the UTC timestamp is always canonical.
const MIN_OFFSET_MINUTES: i32 = -720;
const MAX_OFFSET_MINUTES: i32 = 840;

/// #1252 — upper bound on the configured booking lead time: 30 days.
const MAX_LEAD_TIME_SECONDS: u32 = 2_592_000;

/// #1253 — bounded history: one session keeps at most this many applied
/// schedule changes, so the trace cannot grow without bound.
const MAX_RESCHEDULES_PER_SESSION: u32 = 16;

/// #1253 — bounds on the reschedule approval window.
const MIN_APPROVAL_WINDOW_SECONDS: u32 = 60;
const MAX_APPROVAL_WINDOW_SECONDS: u32 = 2_592_000; // 30 days

/// #1253 — the notice period defaults to 24 hours and the approval window
/// to 72 hours; both are admin-configurable within bounds.
const DEFAULT_RESCHEDULE_NOTICE_SECONDS: u32 = 86_400;
const DEFAULT_APPROVAL_WINDOW_SECONDS: u32 = 259_200;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// Only the registrar (admin-set, deny-by-default) may register or
    /// update agreement anchors.
    NotRegistrar = 4,
    /// The caller is not the mentor or mentee of the agreement/session.
    NotAuthorized = 5,
    AgreementNotFound = 6,
    AgreementAlreadyRegistered = 7,
    /// Sessions can only be booked under an active agreement.
    AgreementNotActive = 8,
    /// The supplied `session_id` is already taken.
    SessionAlreadyExists = 9,
    SessionNotFound = 10,
    /// The proposed slot overlaps an existing session of the mentor or
    /// mentee — booking would double-book them.
    ScheduleConflict = 11,
    /// The start time is closer than the configured minimum lead time.
    LeadTimeNotMet = 12,
    /// The start time is not in the future.
    StartTimeInPast = 13,
    /// Session duration is outside [MIN_DURATION_MINUTES, MAX_DURATION_MINUTES].
    InvalidDuration = 14,
    /// The local display offset is outside the UTC-12..UTC+14 bounds.
    InvalidTimezoneOffset = 15,
    /// The participant already holds MAX_SESSIONS_PER_PARTICIPANT sessions.
    SessionLimitReached = 16,
    /// The same idempotency key was reused for a different booking payload.
    IdempotencyConflict = 17,
    /// The configured lead time is above MAX_LEAD_TIME_SECONDS.
    InvalidLeadTime = 18,
    /// No reschedule proposal exists for the supplied id.
    RescheduleNotFound = 19,
    /// The session already has an open reschedule proposal.
    RescheduleAlreadyPending = 20,
    /// The session is already inside the notice period and can no longer
    /// be moved.
    RescheduleNoticeNotMet = 21,
    /// The proposal's approval window elapsed; a fresh proposal is needed.
    RescheduleExpired = 22,
    /// The session changed after the proposal was made; the approval is
    /// stale and must not be applied.
    StaleRescheduleApproval = 23,
    /// The proposed slot is not a valid change (same slot or invalid
    /// bounds).
    InvalidReschedule = 24,
    /// The session already holds MAX_RESCHEDULES_PER_SESSION changes.
    RescheduleLimitReached = 25,
    /// The proposal is not open (already applied or declined).
    RescheduleNotOpen = 26,
    /// The configured notice period or approval window is out of bounds.
    InvalidReschedulePolicy = 27,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Registrar,
    /// Booking policy: minimum lead time in seconds.
    MinLeadTime,
    /// Anchor for one mentorship agreement, registered by the registrar.
    Agreement(BytesN<32>),
    /// One booked session, keyed by its id.
    Session(BytesN<32>),
    /// Bounded index of session ids a participant belongs to.
    ParticipantSessions(Address),
    /// Idempotency record for a booking attempt.
    BookingIdem(BookingIdemKey),
    /// One reschedule proposal, keyed by its id.
    Reschedule(BytesN<32>),
    /// The open proposal for a session, if any (one at a time).
    OpenReschedule(BytesN<32>),
    /// Notice period before a session can be moved, seconds.
    RescheduleNotice,
    /// How long a proposal stays approvable, seconds.
    ApprovalWindow,
}

/// #1252 — idempotency key material: the agreement, the caller, and the
/// caller-supplied key. Scoped so two participants booking under the same
/// agreement cannot collide on equal keys.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookingIdemKey {
    pub agreement_id: BytesN<32>,
    pub caller: Address,
    pub idempotency_key: BytesN<32>,
}

/// #1252 — how the session is held. Stored as an enum for the ABI; the
/// meeting URL or venue itself stays off-chain behind `location_ref`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionMode {
    Video,
    Audio,
    InPerson,
    Chat,
}

/// #1252 — session lifecycle within MENTOR-056. Cancellation (MENTOR-058),
/// no-shows (MENTOR-059) and completion are later work items that extend
/// this state machine; v1 only books and confirms.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    Booked,
    Confirmed,
}

/// #1252 — minimal agreement anchor. Private terms stay off-chain; only the
/// parties and the active flag live here. When the canonical agreements
/// contract (MENTOR-045/046/047) ships, its registrar address takes over
/// this role and this anchor becomes a projection of that contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgreementAnchor {
    pub agreement_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub active: bool,
    pub registered_at: u64,
}

/// #1252 — a booked mentorship session. Times are canonical UTC plus a
/// bounded display offset ("confirmations use local time plus UTC");
/// everything user-authored (meeting link, venue, agenda text) is only ever
/// referenced by commitment.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub session_id: BytesN<32>,
    pub agreement_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    /// Canonical start, as a Unix timestamp (UTC).
    pub start_utc: u64,
    /// Display offset east of UTC, minutes; UTC-12..UTC+14.
    pub local_offset_minutes: i32,
    pub duration_minutes: u32,
    pub mode: SessionMode,
    /// Commitment/reference to the off-chain meeting access or venue.
    pub location_ref: BytesN<32>,
    /// Commitment over the off-chain agenda text.
    pub agenda_hash: BytesN<32>,
    pub status: SessionStatus,
    /// UTC timestamps of each participant's confirmation; 0 = unconfirmed.
    pub mentor_confirmed_at: u64,
    pub mentee_confirmed_at: u64,
    pub booked_at: u64,
    /// Monotonic revision, incremented on every applied reschedule. Used
    /// to reject stale approvals and to let clients detect changes.
    pub revision: u32,
    /// Bounded, append-only trace of applied schedule changes. The
    /// original slot is always the first entry's `previous_*` values.
    pub reschedule_history: Vec<ScheduleChange>,
}

/// #1253 — one applied schedule change, so the original time remains
/// traceable after any number of reschedules.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleChange {
    pub proposal_id: BytesN<32>,
    pub previous_start_utc: u64,
    pub previous_duration_minutes: u32,
    pub previous_local_offset_minutes: i32,
    pub new_start_utc: u64,
    pub new_duration_minutes: u32,
    pub new_local_offset_minutes: i32,
    pub changed_at: u64,
}

/// #1253 — public, blame-free reason codes for a reschedule. The private
/// explanation exists only as a `reason_hash` commitment off-chain.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RescheduleReason {
    AvailabilityChange,
    ScheduleConflict,
    TimezoneChange,
    Personal,
    Other,
}

/// #1253 — preview of the settlement consequences of a time change. The
/// contract never moves funds; it only tells the settlement layer whether
/// an adjustment is required and in which direction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentImpact {
    pub current_minutes: u32,
    pub proposed_minutes: u32,
    /// Proposed minus current billable minutes (negative = shorter).
    pub delta_minutes: i32,
    /// True when the duration changed, i.e. settlement must be adjusted.
    pub settlement_adjustment_required: bool,
}

/// #1253 — the reschedule proposal lifecycle.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RescheduleStatus {
    Proposed,
    Applied,
    Declined,
}

/// #1253 — a proposed time change awaiting the counterparty's approval.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RescheduleProposal {
    pub proposal_id: BytesN<32>,
    pub session_id: BytesN<32>,
    pub proposed_by: Address,
    pub reason: RescheduleReason,
    /// Commitment over the private explanation; never emitted in events.
    pub reason_hash: BytesN<32>,
    pub new_start_utc: u64,
    pub new_local_offset_minutes: i32,
    pub new_duration_minutes: u32,
    /// Payment consequences previewed at proposal time and revalidated by
    /// the stale-approval check at apply time.
    pub payment_impact: PaymentImpact,
    pub created_at: u64,
    pub expires_at: u64,
    /// Session revision the proposal was built against.
    pub session_revision: u32,
    pub status: RescheduleStatus,
    pub responded_by: Option<Address>,
    pub responded_at: u64,
    pub applied_at: u64,
}

/// #1252 — a session's schedule rendered in both UTC and the booked local
/// offset, so confirmations can show participants "18:00 (15:00 UTC)".
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTimes {
    pub start_utc: u64,
    pub end_utc: u64,
    pub local_offset_minutes: i32,
    pub start_local: u64,
    pub end_local: u64,
}

#[contract]
pub struct MentorshipSchedulingContract;

#[contractimpl]
impl MentorshipSchedulingContract {
    /// Initialize the contract admin and booking policy. Run once. The
    /// admin also becomes the initial registrar.
    pub fn initialize(
        env: Env,
        admin: Address,
        min_lead_time_seconds: u32,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        Self::validate_lead_time(min_lead_time_seconds)?;
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Registrar, &admin);
        env.storage()
            .instance()
            .set(&DataKey::MinLeadTime, &min_lead_time_seconds);
        // #1253 — reschedule policy defaults; admin-tunable within bounds.
        env.storage().instance().set(
            &DataKey::RescheduleNotice,
            &DEFAULT_RESCHEDULE_NOTICE_SECONDS,
        );
        env.storage()
            .instance()
            .set(&DataKey::ApprovalWindow, &DEFAULT_APPROVAL_WINDOW_SECONDS);
        Ok(())
    }

    pub fn admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn registrar(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Registrar)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn min_lead_time_seconds(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::MinLeadTime)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    pub fn reschedule_notice_seconds(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::RescheduleNotice)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn approval_window_seconds(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::ApprovalWindow)
            .ok_or(ContractError::NotInitialized)
    }

    /// Admin-only: move the registrar role. Deny-by-default: until this is
    /// called with the canonical agreements contract's address, only the
    /// admin (set at `initialize`) may register agreement anchors.
    pub fn set_registrar(
        env: Env,
        admin: Address,
        registrar: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Registrar, &registrar);
        env.events()
            .publish((symbol_short!("REGISTRAR"),), (registrar,));
        Ok(())
    }

    /// Admin-only: update the booking lead time.
    pub fn set_min_lead_time(
        env: Env,
        admin: Address,
        min_lead_time_seconds: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_lead_time(min_lead_time_seconds)?;
        env.storage()
            .instance()
            .set(&DataKey::MinLeadTime, &min_lead_time_seconds);
        env.events()
            .publish((symbol_short!("LEADTIME"),), (min_lead_time_seconds,));
        Ok(())
    }

    /// Admin-only: set the reschedule notice period and approval window.
    /// Bounds keep the policy from being configured into never-noticing or
    /// never-expiring.
    pub fn set_reschedule_policy(
        env: Env,
        admin: Address,
        notice_seconds: u32,
        approval_window_seconds: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_reschedule_policy(notice_seconds, approval_window_seconds)?;
        env.storage()
            .instance()
            .set(&DataKey::RescheduleNotice, &notice_seconds);
        env.storage()
            .instance()
            .set(&DataKey::ApprovalWindow, &approval_window_seconds);
        env.events().publish(
            (symbol_short!("RESPPOL"),),
            (notice_seconds, approval_window_seconds),
        );
        Ok(())
    }

    /// Registrar-only: anchor a mentorship agreement so its participants can
    /// book sessions under it. Re-registration of the same id is rejected.
    pub fn register_agreement(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Agreement(agreement_id.clone()))
        {
            return Err(ContractError::AgreementAlreadyRegistered);
        }
        if mentor == mentee {
            // A 1-on-1 agreement needs two distinct participants; reject
            // degenerate anchors rather than booking self-sessions.
            return Err(ContractError::NotAuthorized);
        }
        let anchor = AgreementAnchor {
            agreement_id: agreement_id.clone(),
            mentor,
            mentee,
            active: true,
            registered_at: env.ledger().timestamp(),
        };
        let key = DataKey::Agreement(agreement_id.clone());
        env.storage().persistent().set(&key, &anchor);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("AGREG"),), (agreement_id,));
        Ok(())
    }

    /// Registrar-only: activate or deactivate an agreement anchor. Booking
    /// is refused while the anchor is inactive.
    pub fn set_agreement_active(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        let key = DataKey::Agreement(agreement_id.clone());
        let mut anchor: AgreementAnchor = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AgreementNotFound)?;
        anchor.active = active;
        env.storage().persistent().set(&key, &anchor);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((symbol_short!("AGACT"),), (agreement_id, active));
        Ok(())
    }

    // ── #1252 — booking ──────────────────────────────────────────────────

    /// Book a session under an active agreement. The caller must be the
    /// agreement's mentor or mentee. The slot is rechecked against both
    /// participants' existing sessions so concurrent bookings cannot
    /// double-book, the configured lead time is enforced, and the booking
    /// emits exactly one canonical `BOOKED` event.
    ///
    /// Retries are safe: the same `idempotency_key` with the same payload
    /// returns the original session id without duplicating state or events;
    /// reusing the key for a different payload fails with
    /// `IdempotencyConflict`.
    pub fn book_session(
        env: Env,
        caller: Address,
        agreement_id: BytesN<32>,
        session_id: BytesN<32>,
        idempotency_key: BytesN<32>,
        start_utc: u64,
        local_offset_minutes: i32,
        duration_minutes: u32,
        mode: SessionMode,
        location_ref: BytesN<32>,
        agenda_hash: BytesN<32>,
    ) -> Result<BytesN<32>, ContractError> {
        let anchor = Self::load_agreement(&env, &agreement_id)?;
        if !anchor.active {
            return Err(ContractError::AgreementNotActive);
        }
        if caller != anchor.mentor && caller != anchor.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        // Idempotent replay: same key + same payload short-circuits before
        // any other effect, so a retried transaction never double-books.
        let idem_key = DataKey::BookingIdem(BookingIdemKey {
            agreement_id: agreement_id.clone(),
            caller: caller.clone(),
            idempotency_key,
        });
        if let Some(existing_id) = env.storage().persistent().get::<_, BytesN<32>>(&idem_key) {
            env.storage()
                .persistent()
                .extend_ttl(&idem_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            let existing = Self::load_session(&env, &existing_id)?;
            let same_payload = existing.start_utc == start_utc
                && existing.local_offset_minutes == local_offset_minutes
                && existing.duration_minutes == duration_minutes
                && existing.mode == mode
                && existing.location_ref == location_ref
                && existing.agenda_hash == agenda_hash;
            if !same_payload {
                return Err(ContractError::IdempotencyConflict);
            }
            return Ok(existing_id);
        }

        Self::validate_slot(&env, start_utc, local_offset_minutes, duration_minutes)?;

        if env
            .storage()
            .persistent()
            .has(&DataKey::Session(session_id.clone()))
        {
            return Err(ContractError::SessionAlreadyExists);
        }

        // Bounded per-participant storage, then the double-booking check
        // across *both* participants' existing sessions.
        let mentor_sessions = Self::load_participant_sessions(&env, &anchor.mentor);
        let mentee_sessions = Self::load_participant_sessions(&env, &anchor.mentee);
        if mentor_sessions.len() >= MAX_SESSIONS_PER_PARTICIPANT
            || mentee_sessions.len() >= MAX_SESSIONS_PER_PARTICIPANT
        {
            return Err(ContractError::SessionLimitReached);
        }
        let end_utc = start_utc
            .checked_add(
                u64::from(duration_minutes)
                    .checked_mul(60)
                    .ok_or(ContractError::InvalidDuration)?,
            )
            .ok_or(ContractError::InvalidDuration)?;
        for participant_sessions in [mentor_sessions.clone(), mentee_sessions.clone()] {
            for i in 0..participant_sessions.len() {
                let other_id = participant_sessions
                    .get(i)
                    .ok_or(ContractError::SessionNotFound)?;
                let other = Self::load_session(&env, &other_id)?;
                if start_utc
                    < other
                        .start_utc
                        .checked_add(
                            u64::from(other.duration_minutes)
                                .checked_mul(60)
                                .ok_or(ContractError::InvalidDuration)?,
                        )
                        .ok_or(ContractError::InvalidDuration)?
                    && other.start_utc < end_utc
                {
                    return Err(ContractError::ScheduleConflict);
                }
            }
        }

        let now = env.ledger().timestamp();
        let session = Session {
            session_id: session_id.clone(),
            agreement_id: agreement_id.clone(),
            mentor: anchor.mentor.clone(),
            mentee: anchor.mentee.clone(),
            start_utc,
            local_offset_minutes,
            duration_minutes,
            mode,
            location_ref,
            agenda_hash,
            status: SessionStatus::Booked,
            mentor_confirmed_at: 0,
            mentee_confirmed_at: 0,
            booked_at: now,
            revision: 0,
            reschedule_history: Vec::new(&env),
        };
        Self::put_session(&env, &session);
        Self::index_session(&env, &anchor.mentor, &mentor_sessions, &session_id);
        Self::index_session(&env, &anchor.mentee, &mentee_sessions, &session_id);
        env.storage().persistent().set(&idem_key, &session_id);
        env.storage()
            .persistent()
            .extend_ttl(&idem_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        // One canonical event per booking, carrying the scheduling facts.
        env.events().publish(
            (symbol_short!("BOOKED"), session_id.clone()),
            (
                agreement_id,
                anchor.mentor,
                anchor.mentee,
                start_utc,
                duration_minutes,
            ),
        );
        Ok(session_id)
    }

    /// Confirm a session as the calling participant. Idempotent: a repeated
    /// confirmation returns the current state without a duplicate effect.
    /// The session becomes `Confirmed` once both participants have confirmed.
    pub fn confirm_session(
        env: Env,
        caller: Address,
        session_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let mut session = Self::load_session(&env, &session_id)?;
        if caller != session.mentor && caller != session.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        let now = env.ledger().timestamp();
        if session.status == SessionStatus::Confirmed {
            return Ok(true);
        }
        if caller == session.mentor {
            if session.mentor_confirmed_at != 0 {
                return Ok(false);
            }
            session.mentor_confirmed_at = now;
        } else {
            if session.mentee_confirmed_at != 0 {
                return Ok(false);
            }
            session.mentee_confirmed_at = now;
        }
        if session.mentor_confirmed_at != 0 && session.mentee_confirmed_at != 0 {
            session.status = SessionStatus::Confirmed;
        }
        let both_confirmed = session.status == SessionStatus::Confirmed;
        Self::put_session(&env, &session);

        env.events().publish(
            (symbol_short!("CFMD"), session_id),
            (caller, now, both_confirmed),
        );
        Ok(both_confirmed)
    }

    // ── #1253 — reschedule workflow ─────────────────────────────────────

    /// Propose a new time for an existing session. The caller must be a
    /// participant; the change is applied only after the *other*
    /// participant approves it. The slot is rechecked for conflicts here
    /// and again at approval, and the proposal expires after the
    /// configured approval window (capped at the session start).
    ///
    /// Retries are safe: the same proposal id with the same payload
    /// returns the existing proposal without duplicating state or events;
    /// reusing the id for a different payload fails with
    /// `IdempotencyConflict`.
    pub fn propose_reschedule(
        env: Env,
        caller: Address,
        session_id: BytesN<32>,
        proposal_id: BytesN<32>,
        reason: RescheduleReason,
        reason_hash: BytesN<32>,
        new_start_utc: u64,
        new_local_offset_minutes: i32,
        new_duration_minutes: u32,
    ) -> Result<RescheduleProposal, ContractError> {
        let session = Self::load_session(&env, &session_id)?;
        if caller != session.mentor && caller != session.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        // Idempotent replay: same proposal id + same payload short-circuits
        // before any other effect, so a retried transaction is a no-op.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RescheduleProposal>(&DataKey::Reschedule(proposal_id.clone()))
        {
            let same_payload = existing.session_id == session_id
                && existing.proposed_by == caller
                && existing.reason == reason
                && existing.reason_hash == reason_hash
                && existing.new_start_utc == new_start_utc
                && existing.new_local_offset_minutes == new_local_offset_minutes
                && existing.new_duration_minutes == new_duration_minutes;
            if !same_payload {
                return Err(ContractError::IdempotencyConflict);
            }
            return Ok(existing);
        }

        if session.reschedule_history.len() >= MAX_RESCHEDULES_PER_SESSION {
            return Err(ContractError::RescheduleLimitReached);
        }

        let now = env.ledger().timestamp();
        let notice: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RescheduleNotice)
            .ok_or(ContractError::NotInitialized)?;
        // Notice policy: the *original* meeting must still be far enough
        // away for a change to be fair to the counterparty.
        if now
            .checked_add(u64::from(notice))
            .ok_or(ContractError::RescheduleNoticeNotMet)?
            > session.start_utc
        {
            return Err(ContractError::RescheduleNoticeNotMet);
        }

        Self::validate_slot(
            &env,
            new_start_utc,
            new_local_offset_minutes,
            new_duration_minutes,
        )?;
        // A "change" that changes nothing is rejected rather than opening a
        // pointless approval round.
        if new_start_utc == session.start_utc
            && new_duration_minutes == session.duration_minutes
            && new_local_offset_minutes == session.local_offset_minutes
        {
            return Err(ContractError::InvalidReschedule);
        }

        // Continuity control: one open proposal per session. An expired
        // proposal is cleaned up so a fresh one can be opened.
        let open_key = DataKey::OpenReschedule(session_id.clone());
        if let Some(open_id) = env.storage().persistent().get::<_, BytesN<32>>(&open_key) {
            let open = Self::load_reschedule(&env, &open_id)?;
            if open.status == RescheduleStatus::Proposed && now <= open.expires_at {
                return Err(ContractError::RescheduleAlreadyPending);
            }
            env.storage().persistent().remove(&open_key);
            env.storage()
                .persistent()
                .remove(&DataKey::Reschedule(open_id));
        }

        // Conflict recheck: the proposed slot must not double-book either
        // participant against any *other* session they hold.
        let end_utc = Self::session_end(new_start_utc, new_duration_minutes)?;
        if Self::conflicts_with(
            &env,
            &session.mentor,
            new_start_utc,
            end_utc,
            Some(&session_id),
        ) || Self::conflicts_with(
            &env,
            &session.mentee,
            new_start_utc,
            end_utc,
            Some(&session_id),
        ) {
            return Err(ContractError::ScheduleConflict);
        }

        let window: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ApprovalWindow)
            .ok_or(ContractError::NotInitialized)?;
        // The approval window can never outlive the meeting itself: an
        // approval after the original start would be meaningless.
        let mut expires_at = now
            .checked_add(u64::from(window))
            .ok_or(ContractError::RescheduleExpired)?;
        if expires_at > session.start_utc {
            expires_at = session.start_utc;
        }

        let proposal = RescheduleProposal {
            proposal_id: proposal_id.clone(),
            session_id: session_id.clone(),
            proposed_by: caller,
            reason,
            reason_hash,
            new_start_utc,
            new_local_offset_minutes,
            new_duration_minutes,
            payment_impact: Self::payment_impact(session.duration_minutes, new_duration_minutes),
            created_at: now,
            expires_at,
            session_revision: session.revision,
            status: RescheduleStatus::Proposed,
            responded_by: None,
            responded_at: 0,
            applied_at: 0,
        };
        Self::put_reschedule(&env, &proposal);
        env.storage().persistent().set(&open_key, &proposal_id);
        env.storage()
            .persistent()
            .extend_ttl(&open_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Notification: one canonical proposal event carrying the public
        // reason code and the proposed UTC start. The private explanation
        // commitment is never emitted.
        env.events().publish(
            (symbol_short!("RSPROP"), proposal_id),
            (session_id, reason, new_start_utc),
        );
        Ok(proposal)
    }

    /// Approve a pending reschedule as the counterparty. Applying the
    /// change rechecks conflicts, rejects stale approvals, records the
    /// previous slot in the bounded trace, resets confirmations, and emits
    /// exactly one `RESCHED` event. Approving an already-applied proposal
    /// is idempotent and emits nothing.
    pub fn approve_reschedule(
        env: Env,
        caller: Address,
        proposal_id: BytesN<32>,
    ) -> Result<Session, ContractError> {
        let mut proposal = Self::load_reschedule(&env, &proposal_id)?;
        let mut session = Self::load_session(&env, &proposal.session_id)?;
        if caller != session.mentor && caller != session.mentee {
            return Err(ContractError::NotAuthorized);
        }
        // Participant approval: the proposing party cannot approve its own
        // change; consent must come from the counterparty.
        if caller == proposal.proposed_by {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();

        // Idempotent replay: applying twice returns the current session
        // without a duplicate event or trace entry.
        if proposal.status == RescheduleStatus::Applied {
            return Ok(session);
        }
        if proposal.status != RescheduleStatus::Proposed {
            return Err(ContractError::RescheduleNotOpen);
        }

        let now = env.ledger().timestamp();
        if now > proposal.expires_at {
            return Err(ContractError::RescheduleExpired);
        }
        // Stale approval: the session changed (another reschedule applied)
        // since this proposal was made.
        if session.revision != proposal.session_revision {
            return Err(ContractError::StaleRescheduleApproval);
        }
        if session.reschedule_history.len() >= MAX_RESCHEDULES_PER_SESSION {
            return Err(ContractError::RescheduleLimitReached);
        }

        // Conflict recheck at apply time: a booking made between proposal
        // and approval must not create a double-booking.
        let end_utc = Self::session_end(proposal.new_start_utc, proposal.new_duration_minutes)?;
        if Self::conflicts_with(
            &env,
            &session.mentor,
            proposal.new_start_utc,
            end_utc,
            Some(&session.session_id),
        ) || Self::conflicts_with(
            &env,
            &session.mentee,
            proposal.new_start_utc,
            end_utc,
            Some(&session.session_id),
        ) {
            return Err(ContractError::ScheduleConflict);
        }

        let previous_start = session.start_utc;
        let previous_duration = session.duration_minutes;
        let previous_offset = session.local_offset_minutes;
        session.reschedule_history.push_back(ScheduleChange {
            proposal_id: proposal_id.clone(),
            previous_start_utc: previous_start,
            previous_duration_minutes: previous_duration,
            previous_local_offset_minutes: previous_offset,
            new_start_utc: proposal.new_start_utc,
            new_duration_minutes: proposal.new_duration_minutes,
            new_local_offset_minutes: proposal.new_local_offset_minutes,
            changed_at: now,
        });
        session.start_utc = proposal.new_start_utc;
        session.duration_minutes = proposal.new_duration_minutes;
        session.local_offset_minutes = proposal.new_local_offset_minutes;
        session.revision = session.revision.saturating_add(1);
        // Both participants must reconfirm the new slot; calendar views
        // derive from the session, so they converge on the change.
        session.status = SessionStatus::Booked;
        session.mentor_confirmed_at = 0;
        session.mentee_confirmed_at = 0;
        Self::put_session(&env, &session);

        proposal.status = RescheduleStatus::Applied;
        proposal.responded_by = Some(caller.clone());
        proposal.responded_at = now;
        proposal.applied_at = now;
        Self::put_reschedule(&env, &proposal);
        env.storage()
            .persistent()
            .remove(&DataKey::OpenReschedule(session.session_id.clone()));

        env.events().publish(
            (symbol_short!("RESCHED"), proposal_id),
            (
                session.session_id.clone(),
                previous_start,
                proposal.new_start_utc,
                proposal.new_duration_minutes,
                caller,
            ),
        );
        Ok(session)
    }

    /// Decline a pending reschedule as the counterparty. The session is
    /// untouched. The proposer cannot decline its own proposal.
    pub fn decline_reschedule(
        env: Env,
        caller: Address,
        proposal_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut proposal = Self::load_reschedule(&env, &proposal_id)?;
        let session = Self::load_session(&env, &proposal.session_id)?;
        if caller != session.mentor && caller != session.mentee {
            return Err(ContractError::NotAuthorized);
        }
        if caller == proposal.proposed_by {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        if proposal.status == RescheduleStatus::Declined {
            return Ok(());
        }
        if proposal.status != RescheduleStatus::Proposed {
            return Err(ContractError::RescheduleNotOpen);
        }
        let now = env.ledger().timestamp();
        proposal.status = RescheduleStatus::Declined;
        proposal.responded_by = Some(caller.clone());
        proposal.responded_at = now;
        Self::put_reschedule(&env, &proposal);
        env.storage()
            .persistent()
            .remove(&DataKey::OpenReschedule(session.session_id.clone()));
        env.events().publish(
            (symbol_short!("RSDECL"), proposal_id),
            (session.session_id, caller, now),
        );
        Ok(())
    }

    // ── views ───────────────────────────────────────────────────────────

    pub fn get_agreement(
        env: Env,
        agreement_id: BytesN<32>,
    ) -> Result<AgreementAnchor, ContractError> {
        Self::load_agreement(&env, &agreement_id)
    }

    pub fn get_session(env: Env, session_id: BytesN<32>) -> Result<Session, ContractError> {
        Self::load_session(&env, &session_id)
    }

    /// Bounded index of the session ids a participant belongs to.
    pub fn participant_sessions(
        env: Env,
        participant: Address,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Ok(Self::load_participant_sessions(&env, &participant))
    }

    /// True when `participant` already has a session overlapping the
    /// proposed slot. Public read: it reveals only the fact of a conflict,
    /// never who the other participant is.
    pub fn has_conflict(
        env: Env,
        participant: Address,
        start_utc: u64,
        duration_minutes: u32,
    ) -> Result<bool, ContractError> {
        let end_utc = start_utc
            .checked_add(
                u64::from(duration_minutes)
                    .checked_mul(60)
                    .ok_or(ContractError::InvalidDuration)?,
            )
            .ok_or(ContractError::InvalidDuration)?;
        let sessions = Self::load_participant_sessions(&env, &participant);
        for i in 0..sessions.len() {
            let id = sessions.get(i).ok_or(ContractError::SessionNotFound)?;
            let session = Self::load_session(&env, &id)?;
            if start_utc
                < session
                    .start_utc
                    .checked_add(
                        u64::from(session.duration_minutes)
                            .checked_mul(60)
                            .ok_or(ContractError::InvalidDuration)?,
                    )
                    .ok_or(ContractError::InvalidDuration)?
                && session.start_utc < end_utc
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A session's schedule in both UTC and the booked local offset, so
    /// confirmations can display "local time plus UTC".
    pub fn session_times(env: Env, session_id: BytesN<32>) -> Result<SessionTimes, ContractError> {
        let session = Self::load_session(&env, &session_id)?;
        let duration = u64::from(session.duration_minutes)
            .checked_mul(60)
            .ok_or(ContractError::InvalidDuration)?;
        let end_utc = session
            .start_utc
            .checked_add(duration)
            .ok_or(ContractError::InvalidDuration)?;
        let offset = i64::from(session.local_offset_minutes)
            .checked_mul(60)
            .ok_or(ContractError::InvalidTimezoneOffset)?;
        let start_local = shift_by_offset(session.start_utc, offset)?;
        let end_local = shift_by_offset(end_utc, offset)?;
        Ok(SessionTimes {
            start_utc: session.start_utc,
            end_utc,
            local_offset_minutes: session.local_offset_minutes,
            start_local,
            end_local,
        })
    }

    pub fn get_reschedule(
        env: Env,
        proposal_id: BytesN<32>,
    ) -> Result<RescheduleProposal, ContractError> {
        Self::load_reschedule(&env, &proposal_id)
    }

    /// Bounded, append-only trace of applied schedule changes for a
    /// session. The original slot is the first entry's `previous_*`
    /// values, so a rescheduled meeting never loses its history.
    pub fn session_reschedules(
        env: Env,
        session_id: BytesN<32>,
    ) -> Result<Vec<ScheduleChange>, ContractError> {
        Ok(Self::load_session(&env, &session_id)?.reschedule_history)
    }

    /// Preview the settlement consequences of a duration change before a
    /// proposal exists. Public and side-effect free: it reveals only the
    /// caller-supplied duration and the session's current duration.
    pub fn reschedule_payment_impact(
        env: Env,
        session_id: BytesN<32>,
        new_duration_minutes: u32,
    ) -> Result<PaymentImpact, ContractError> {
        let session = Self::load_session(&env, &session_id)?;
        if !(MIN_DURATION_MINUTES..=MAX_DURATION_MINUTES).contains(&new_duration_minutes) {
            return Err(ContractError::InvalidDuration);
        }
        Ok(Self::payment_impact(
            session.duration_minutes,
            new_duration_minutes,
        ))
    }

    /// Admin-only: upgrade the contract to `new_wasm_hash`. The contract
    /// address is stable; `version()` reports the ABI generation so
    /// clients can detect upgrade compatibility.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("UPGRADED"),), (new_wasm_hash,));
        Ok(())
    }

    // ── internals ───────────────────────────────────────────────────────

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

    fn require_registrar(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let registrar: Address = env
            .storage()
            .instance()
            .get(&DataKey::Registrar)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != registrar {
            return Err(ContractError::NotRegistrar);
        }
        caller.require_auth();
        Ok(())
    }

    fn validate_reschedule_policy(
        notice_seconds: u32,
        approval_window_seconds: u32,
    ) -> Result<(), ContractError> {
        if notice_seconds > MAX_LEAD_TIME_SECONDS
            || !(MIN_APPROVAL_WINDOW_SECONDS..=MAX_APPROVAL_WINDOW_SECONDS)
                .contains(&approval_window_seconds)
        {
            return Err(ContractError::InvalidReschedulePolicy);
        }
        Ok(())
    }

    /// End timestamp of a slot, with checked arithmetic.
    fn session_end(start_utc: u64, duration_minutes: u32) -> Result<u64, ContractError> {
        start_utc
            .checked_add(
                u64::from(duration_minutes)
                    .checked_mul(60)
                    .ok_or(ContractError::InvalidDuration)?,
            )
            .ok_or(ContractError::InvalidDuration)
    }

    /// Whether `participant` already holds a session overlapping
    /// `[start_utc, end_utc)`, ignoring `exclude` (the session being
    /// moved). Back-to-back sessions do not overlap.
    fn conflicts_with(
        env: &Env,
        participant: &Address,
        start_utc: u64,
        end_utc: u64,
        exclude: Option<&BytesN<32>>,
    ) -> bool {
        let sessions = Self::load_participant_sessions(env, participant);
        for i in 0..sessions.len() {
            let id = match sessions.get(i) {
                Some(id) => id,
                None => continue,
            };
            if let Some(skip) = exclude {
                if &id == skip {
                    continue;
                }
            }
            let other = match env
                .storage()
                .persistent()
                .get::<_, Session>(&DataKey::Session(id))
            {
                Some(other) => other,
                None => continue,
            };
            let other_end = match Self::session_end(other.start_utc, other.duration_minutes) {
                Ok(end) => end,
                Err(_) => continue,
            };
            if start_utc < other_end && other.start_utc < end_utc {
                return true;
            }
        }
        false
    }

    fn payment_impact(current_minutes: u32, proposed_minutes: u32) -> PaymentImpact {
        let delta = i64::from(proposed_minutes) - i64::from(current_minutes);
        PaymentImpact {
            current_minutes,
            proposed_minutes,
            delta_minutes: delta as i32,
            settlement_adjustment_required: proposed_minutes != current_minutes,
        }
    }

    fn validate_lead_time(min_lead_time_seconds: u32) -> Result<(), ContractError> {
        if min_lead_time_seconds > MAX_LEAD_TIME_SECONDS {
            return Err(ContractError::InvalidLeadTime);
        }
        Ok(())
    }

    fn validate_slot(
        env: &Env,
        start_utc: u64,
        local_offset_minutes: i32,
        duration_minutes: u32,
    ) -> Result<(), ContractError> {
        if !(MIN_DURATION_MINUTES..=MAX_DURATION_MINUTES).contains(&duration_minutes) {
            return Err(ContractError::InvalidDuration);
        }
        if !(MIN_OFFSET_MINUTES..=MAX_OFFSET_MINUTES).contains(&local_offset_minutes) {
            return Err(ContractError::InvalidTimezoneOffset);
        }
        let now = env.ledger().timestamp();
        if start_utc <= now {
            return Err(ContractError::StartTimeInPast);
        }
        let lead: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinLeadTime)
            .ok_or(ContractError::NotInitialized)?;
        let earliest = now
            .checked_add(u64::from(lead))
            .ok_or(ContractError::StartTimeInPast)?;
        if start_utc < earliest {
            return Err(ContractError::LeadTimeNotMet);
        }
        Ok(())
    }

    fn load_agreement(
        env: &Env,
        agreement_id: &BytesN<32>,
    ) -> Result<AgreementAnchor, ContractError> {
        let key = DataKey::Agreement(agreement_id.clone());
        let anchor: AgreementAnchor = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AgreementNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(anchor)
    }

    fn load_session(env: &Env, session_id: &BytesN<32>) -> Result<Session, ContractError> {
        let key = DataKey::Session(session_id.clone());
        let session: Session = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::SessionNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(session)
    }

    fn load_participant_sessions(env: &Env, participant: &Address) -> Vec<BytesN<32>> {
        let key = DataKey::ParticipantSessions(participant.clone());
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        // Only a live entry can be TTL-bumped; extending a missing key
        // would trap.
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        ids
    }

    fn load_reschedule(
        env: &Env,
        proposal_id: &BytesN<32>,
    ) -> Result<RescheduleProposal, ContractError> {
        let key = DataKey::Reschedule(proposal_id.clone());
        let proposal: RescheduleProposal = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::RescheduleNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(proposal)
    }

    fn put_reschedule(env: &Env, proposal: &RescheduleProposal) {
        let key = DataKey::Reschedule(proposal.proposal_id.clone());
        env.storage().persistent().set(&key, proposal);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn put_session(env: &Env, session: &Session) {
        let key = DataKey::Session(session.session_id.clone());
        env.storage().persistent().set(&key, session);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn index_session(
        env: &Env,
        participant: &Address,
        ids: &Vec<BytesN<32>>,
        session_id: &BytesN<32>,
    ) {
        let mut updated = ids.clone();
        updated.push_back(session_id.clone());
        let key = DataKey::ParticipantSessions(participant.clone());
        env.storage().persistent().set(&key, &updated);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

/// Apply a minutes-east-of-UTC offset to a timestamp without underflowing
/// for west-of-UTC offsets before the Unix epoch.
fn shift_by_offset(timestamp: u64, offset_seconds: i64) -> Result<u64, ContractError> {
    let shifted = i64::try_from(timestamp)
        .ok()
        .and_then(|t| t.checked_add(offset_seconds))
        .ok_or(ContractError::InvalidTimezoneOffset)?;
    u64::try_from(shifted).map_err(|_| ContractError::InvalidTimezoneOffset)
}

#[cfg(test)]
mod tests;
