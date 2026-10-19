#![no_std]

//! Mentorship rematch workflow — issue #1238 (**MENTOR-042**, Matching
//! track).
//!
//! Mentorship matches sometimes stop working: schedules change, working
//! styles clash, or a conflict of interest appears. This contract is the
//! on-chain primitive for handling that reality without erasing history
//! or assigning blame.
//!
//! Scope of this pass (MENTOR-042):
//!
//! - **Request → review → closure plan → successor → continuity.** A
//!   participant requests a rematch on an active match (with a public
//!   reason code and a commitment to a private explanation plus a
//!   closure/handover plan hash). A coordinator reviews it, may open
//!   financial or safety cases, may propose a successor mentor, and the
//!   mentee — and only the mentee — approves the successor before the old
//!   engagement is superseded by a new match.
//! - **Private reasons are scoped.** The reason *code* is a blame-free
//!   enum (no free text); the private explanation exists only as a
//!   `reason_hash` commitment, readable solely by the match's
//!   participants and coordinators (`get_request` is deny-by-default).
//! - **Prior records remain intact.** Closing or superseding a match
//!   never deletes or rewrites it: the old record keeps every field and
//!   gains only a status and a successor pointer, and the full request
//!   history stays queryable per match.
//! - **Open financial or safety cases block unsafe closure.** While a
//!   case flag is open, `close_engagement` and `apply_successor` fail
//!   with `UnsafeClosureBlocked`.
//! - **Rematch metrics avoid blame scoring.** `stats()` exposes only
//!   aggregate counters — never a per-mentor or per-mentee score.
//! - **Bounded and replay-safe.** One open request per match, a capped
//!   request history per match, capped coordinator set, successor
//!   proposals that expire (stale approvals fail with `ProposalExpired`),
//!   and idempotent retries for `request_rematch`.
//!
//! The canonical matching contracts (MENTOR-037 proposals, MENTOR-038
//! mutual acceptance) will own match creation; until they land, a
//! registrar address (admin-set, deny-by-default) anchors match records
//! here. See `contracts/docs/mentorship-rematch.md` for the storage
//! layout, event schema, off-chain boundary, and rollout notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling
// contract convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1238 — bounded storage: one match keeps at most this many rematch
/// requests in its history.
const MAX_REQUESTS_PER_MATCH: u32 = 8;

/// #1238 — bounded coordinator set.
const MAX_COORDINATORS: u32 = 16;

/// #1238 — bounds on the successor-proposal window so it cannot be
/// configured into never expiring or expiring instantly.
const MIN_PROPOSAL_WINDOW_SECONDS: u32 = 60;
const MAX_PROPOSAL_WINDOW_SECONDS: u32 = 2_592_000; // 30 days

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// Only coordinators may review, flag cases, propose successors, or
    /// close engagements.
    NotCoordinator = 4,
    /// Only the registrar (admin-set, deny-by-default) may anchor match
    /// records.
    NotRegistrar = 5,
    /// The caller is not the mentor or mentee of the match, or is trying
    /// to read a scoped request they are party to.
    NotAuthorized = 6,
    MatchNotFound = 7,
    MatchAlreadyRegistered = 8,
    /// Only an active match can be rematched.
    MatchNotActive = 9,
    RematchRequestNotFound = 10,
    /// A match may have at most one open rematch request at a time.
    RematchAlreadyOpen = 11,
    /// The match already holds MAX_REQUESTS_PER_MATCH requests.
    RequestLimitReached = 12,
    /// The request is not in a state that allows this transition.
    InvalidRequestState = 13,
    /// The successor must be a different person than the current mentor
    /// and the mentee.
    InvalidSuccessor = 14,
    /// The successor proposal expired; a fresh one must be proposed.
    ProposalExpired = 15,
    /// An open financial or safety case blocks closing or superseding the
    /// match.
    UnsafeClosureBlocked = 16,
    /// A non-empty closure plan commitment is required to open a rematch.
    ClosurePlanMissing = 17,
    /// The supplied configuration value is outside its allowed bounds.
    InvalidConfig = 18,
    /// The request id is taken by a different request.
    RequestAlreadyExists = 19,
    /// The same request id was retried with a different payload.
    IdempotencyConflict = 20,
    /// The coordinator set is full.
    CoordinatorLimitReached = 21,
    /// The address is not (or is no longer) a coordinator.
    CoordinatorNotFound = 22,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Registrar,
    /// Bounded set of coordinators.
    Coordinators,
    /// Successor-proposal window, seconds.
    ProposalWindow,
    /// Aggregate counters (no identifiers, no scores).
    Stats,
    /// One anchored match record.
    Match(BytesN<32>),
    /// One rematch request.
    Request(BytesN<32>),
    /// Bounded index of request ids per match.
    MatchRequests(BytesN<32>),
    /// Financial/safety case flags per match.
    CaseFlags(BytesN<32>),
}

/// #1238 — the match lifecycle. `Closed` means the engagement ended
/// without a successor; `Superseded` means a successor match took over.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchStatus {
    Active,
    Closed,
    Superseded,
}

/// #1238 — an anchored mentorship match. The canonical match contracts
/// (MENTOR-037/038) will own this record; here it is projected by the
/// registrar. Nothing is ever deleted — closure and supersession only
/// add status and pointers.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchRecord {
    pub match_id: BytesN<32>,
    /// Reference to the engagement agreement (MENTOR-045), if issued.
    pub agreement_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub status: MatchStatus,
    /// Set when a successor match takes over this match.
    pub successor_match_id: Option<BytesN<32>>,
    /// Set on the successor match, pointing back at the match it replaced.
    pub superseded_match_id: Option<BytesN<32>>,
    pub created_at: u64,
    pub closed_at: u64,
}

/// #1238 — public, blame-free reason codes. The private explanation lives
/// off-chain and is referenced only by a commitment.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RematchReason {
    Incompatibility,
    AvailabilityChange,
    ConflictOfInterest,
    SafetyConcern,
    Other,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestStatus {
    Open,
    InReview,
    SuccessorProposed,
    ApprovedForClosure,
    Rejected,
    Closed,
    Superseded,
}

/// #1238 — a rematch request: reason code plus commitments to the private
/// explanation and the closure/handover plan, with the successor
/// proposal lifecycle attached.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RematchRequest {
    pub request_id: BytesN<32>,
    pub match_id: BytesN<32>,
    pub requested_by: Address,
    pub reason: RematchReason,
    /// Commitment over the private explanation; scoped to participants
    /// and coordinators.
    pub reason_hash: BytesN<32>,
    /// Commitment over the continuity/closure plan.
    pub closure_plan_hash: BytesN<32>,
    pub created_at: u64,
    pub review_started_at: u64,
    pub status: RequestStatus,
    pub successor: Option<Address>,
    pub proposal_hash: BytesN<32>,
    pub proposed_at: u64,
    pub proposal_expires_at: u64,
    /// Set when the mentee approves the proposed successor.
    pub successor_approved_at: u64,
    pub closed_at: u64,
}

/// #1238 — open/closed safety and financial case flags. Only the
/// existence of a case is recorded; case contents live off-chain with
/// the safety team (MENTOR-086/087).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaseFlags {
    pub financial_open: bool,
    pub safety_open: bool,
}

/// #1238 — rematch metrics. Deliberately aggregate-only: no addresses,
/// no match ids, and no per-mentor or per-mentee scores, so the metric
/// cannot become a blame instrument.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RematchStats {
    pub total_matches: u32,
    pub open_requests: u32,
    pub closed_requests: u32,
    pub superseded_matches: u32,
}

#[contract]
pub struct MentorshipRematchContract;

#[contractimpl]
impl MentorshipRematchContract {
    /// Initialize the contract admin and successor-proposal window. Run
    /// once; the admin is the first coordinator and the initial
    /// registrar.
    pub fn initialize(
        env: Env,
        admin: Address,
        proposal_window_seconds: u32,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        Self::validate_window(proposal_window_seconds)?;
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Registrar, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ProposalWindow, &proposal_window_seconds);
        let mut coordinators: Vec<Address> = Vec::new(&env);
        coordinators.push_back(admin.clone());
        env.storage()
            .instance()
            .set(&DataKey::Coordinators, &coordinators);
        env.storage().instance().set(
            &DataKey::Stats,
            &RematchStats {
                total_matches: 0,
                open_requests: 0,
                closed_requests: 0,
                superseded_matches: 0,
            },
        );
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

    pub fn proposal_window_seconds(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::ProposalWindow)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    pub fn is_coordinator(env: Env, address: Address) -> bool {
        let coordinators: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Coordinators)
            .unwrap_or_else(|| Vec::new(&env));
        for i in 0..coordinators.len() {
            if let Some(c) = coordinators.get(i) {
                if c == address {
                    return true;
                }
            }
        }
        false
    }

    pub fn coordinators(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Coordinators)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Admin-only: add a coordinator. Bounded so the set cannot grow
    /// without limit.
    pub fn register_coordinator(
        env: Env,
        admin: Address,
        coordinator: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let mut coordinators: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Coordinators)
            .unwrap_or_else(|| Vec::new(&env));
        for i in 0..coordinators.len() {
            if let Some(c) = coordinators.get(i) {
                if c == coordinator {
                    return Ok(());
                }
            }
        }
        if coordinators.len() >= MAX_COORDINATORS {
            return Err(ContractError::CoordinatorLimitReached);
        }
        coordinators.push_back(coordinator.clone());
        env.storage()
            .instance()
            .set(&DataKey::Coordinators, &coordinators);
        env.events()
            .publish((symbol_short!("SCOORD"),), (coordinator,));
        Ok(())
    }

    /// Admin-only: remove a coordinator.
    pub fn revoke_coordinator(
        env: Env,
        admin: Address,
        coordinator: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let coordinators: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Coordinators)
            .unwrap_or_else(|| Vec::new(&env));
        let mut kept: Vec<Address> = Vec::new(&env);
        let mut found = false;
        for i in 0..coordinators.len() {
            if let Some(c) = coordinators.get(i) {
                if c == coordinator {
                    found = true;
                } else {
                    kept.push_back(c);
                }
            }
        }
        if !found {
            return Err(ContractError::CoordinatorNotFound);
        }
        env.storage().instance().set(&DataKey::Coordinators, &kept);
        env.events()
            .publish((symbol_short!("RCOORD"),), (coordinator,));
        Ok(())
    }

    /// Admin-only: move the registrar role to the canonical matching
    /// contract when it ships (deny-by-default until then).
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

    /// Admin-only: update the successor-proposal window.
    pub fn set_proposal_window(
        env: Env,
        admin: Address,
        proposal_window_seconds: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_window(proposal_window_seconds)?;
        env.storage()
            .instance()
            .set(&DataKey::ProposalWindow, &proposal_window_seconds);
        env.events()
            .publish((symbol_short!("PWINDOW"),), (proposal_window_seconds,));
        Ok(())
    }

    // ── #1238 — match anchoring ─────────────────────────────────────────

    /// Registrar-only: anchor a match so its participants can open a
    /// rematch against it.
    pub fn register_match(
        env: Env,
        caller: Address,
        match_id: BytesN<32>,
        agreement_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
    ) -> Result<(), ContractError> {
        Self::require_registrar(&env, &caller)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Match(match_id.clone()))
        {
            return Err(ContractError::MatchAlreadyRegistered);
        }
        if mentor == mentee {
            return Err(ContractError::NotAuthorized);
        }
        let record = MatchRecord {
            match_id: match_id.clone(),
            agreement_id,
            mentor,
            mentee,
            status: MatchStatus::Active,
            successor_match_id: None,
            superseded_match_id: None,
            created_at: env.ledger().timestamp(),
            closed_at: 0,
        };
        Self::put_match(&env, &record);
        Self::bump_stats(&env, |stats| {
            stats.total_matches = stats.total_matches.saturating_add(1);
        });
        env.events().publish((symbol_short!("MREG"),), (match_id,));
        Ok(())
    }

    // ── #1238 — rematch workflow ────────────────────────────────────────

    /// Participant-only: request a rematch on an active match. The public
    /// `reason` is a blame-free code; the private explanation and the
    /// closure plan are commitments only. One open request per match;
    /// retries with the same request id and payload are idempotent.
    pub fn request_rematch(
        env: Env,
        caller: Address,
        match_id: BytesN<32>,
        request_id: BytesN<32>,
        reason: RematchReason,
        reason_hash: BytesN<32>,
        closure_plan_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        let record = Self::load_match(&env, &match_id)?;
        if record.status != MatchStatus::Active {
            return Err(ContractError::MatchNotActive);
        }
        if caller != record.mentor && caller != record.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        if closure_plan_hash == zero_hash(&env) {
            return Err(ContractError::ClosurePlanMissing);
        }

        // Idempotent replay: the same request id with the same payload is
        // the same request; a different payload is a stable failure.
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RematchRequest>(&DataKey::Request(request_id.clone()))
        {
            let same_payload = existing.match_id == match_id
                && existing.requested_by == caller
                && existing.reason == reason
                && existing.reason_hash == reason_hash
                && existing.closure_plan_hash == closure_plan_hash;
            if !same_payload {
                return Err(ContractError::IdempotencyConflict);
            }
            return Ok(());
        }

        // Continuity control: one open rematch per match.
        let index_key = DataKey::MatchRequests(match_id.clone());
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if ids.len() >= MAX_REQUESTS_PER_MATCH {
            return Err(ContractError::RequestLimitReached);
        }
        for i in 0..ids.len() {
            let id = ids.get(i).ok_or(ContractError::RematchRequestNotFound)?;
            let other: RematchRequest = env
                .storage()
                .persistent()
                .get(&DataKey::Request(id))
                .ok_or(ContractError::RematchRequestNotFound)?;
            if other.status == RequestStatus::Open
                || other.status == RequestStatus::InReview
                || other.status == RequestStatus::SuccessorProposed
                || other.status == RequestStatus::ApprovedForClosure
            {
                return Err(ContractError::RematchAlreadyOpen);
            }
        }

        let now = env.ledger().timestamp();
        let request = RematchRequest {
            request_id: request_id.clone(),
            match_id: match_id.clone(),
            requested_by: caller,
            reason,
            reason_hash,
            closure_plan_hash,
            created_at: now,
            review_started_at: 0,
            status: RequestStatus::Open,
            successor: None,
            proposal_hash: zero_hash(&env),
            proposed_at: 0,
            proposal_expires_at: 0,
            successor_approved_at: 0,
            closed_at: 0,
        };
        let request_key = DataKey::Request(request_id.clone());
        env.storage().persistent().set(&request_key, &request);
        env.storage()
            .persistent()
            .extend_ttl(&request_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let mut indexed = ids;
        indexed.push_back(request_id.clone());
        env.storage().persistent().set(&index_key, &indexed);
        env.storage()
            .persistent()
            .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        Self::bump_stats(&env, |stats| {
            stats.open_requests = stats.open_requests.saturating_add(1);
        });
        // The event carries the public reason code only; reason_hash and
        // closure_plan_hash stay in scoped storage.
        env.events()
            .publish((symbol_short!("RMREQ"), request_id), (match_id, reason));
        Ok(())
    }

    /// Scoped read: only the match's participants and coordinators may
    /// read a request, because it carries the private reason commitment.
    pub fn get_request(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
    ) -> Result<RematchRequest, ContractError> {
        let request: RematchRequest = env
            .storage()
            .persistent()
            .get(&DataKey::Request(request_id))
            .ok_or(ContractError::RematchRequestNotFound)?;
        let record = Self::load_match(&env, &request.match_id)?;
        let participant = caller == record.mentor || caller == record.mentee;
        if !participant && !Self::is_coordinator(env.clone(), caller.clone()) {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        Ok(request)
    }

    /// Coordinator-only: start reviewing a request.
    pub fn start_review(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        let mut request = Self::load_request(&env, &request_id)?;
        if request.status != RequestStatus::Open {
            return Err(ContractError::InvalidRequestState);
        }
        request.status = RequestStatus::InReview;
        request.review_started_at = env.ledger().timestamp();
        Self::put_request(&env, &request);
        env.events()
            .publish((symbol_short!("RMRVW"), request_id), (caller,));
        Ok(())
    }

    /// Coordinator-only: reject the rematch; the match stays active.
    pub fn reject_request(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        let mut request = Self::load_request(&env, &request_id)?;
        if request.status != RequestStatus::Open && request.status != RequestStatus::InReview {
            return Err(ContractError::InvalidRequestState);
        }
        request.status = RequestStatus::Rejected;
        Self::put_request(&env, &request);
        Self::bump_stats(&env, |stats| {
            stats.open_requests = stats.open_requests.saturating_sub(1);
            stats.closed_requests = stats.closed_requests.saturating_add(1);
        });
        env.events()
            .publish((symbol_short!("RMREJ"), request_id), (caller,));
        Ok(())
    }

    /// Coordinator-only: open or close a financial/safety case flag on a
    /// match. Only the existence of a case is recorded — case contents
    /// live off-chain with the safety team.
    pub fn set_case_flags(
        env: Env,
        caller: Address,
        match_id: BytesN<32>,
        financial_open: bool,
        safety_open: bool,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        Self::load_match(&env, &match_id)?;
        let flags = CaseFlags {
            financial_open,
            safety_open,
        };
        let key = DataKey::CaseFlags(match_id.clone());
        env.storage().persistent().set(&key, &flags);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("RMFLAG"),),
            (match_id, financial_open, safety_open),
        );
        Ok(())
    }

    /// Coordinator-only: propose a successor mentor for a reviewed
    /// request. The proposal carries a commitment hash and expires after
    /// the configured window.
    pub fn propose_successor(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
        successor: Address,
        proposal_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        let mut request = Self::load_request(&env, &request_id)?;
        if request.status != RequestStatus::InReview
            && request.status != RequestStatus::SuccessorProposed
        {
            return Err(ContractError::InvalidRequestState);
        }
        let record = Self::load_match(&env, &request.match_id)?;
        // Continuity control: the successor must be a new mentor, not one
        // of the current match's participants.
        if successor == record.mentor || successor == record.mentee {
            return Err(ContractError::InvalidSuccessor);
        }
        let window: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalWindow)
            .ok_or(ContractError::NotInitialized)?;
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(u64::from(window))
            .ok_or(ContractError::InvalidConfig)?;
        request.successor = Some(successor.clone());
        request.proposal_hash = proposal_hash;
        request.proposed_at = now;
        request.proposal_expires_at = expires_at;
        request.status = RequestStatus::SuccessorProposed;
        Self::put_request(&env, &request);
        env.events().publish(
            (symbol_short!("RMSUCC"), request_id),
            (successor, expires_at),
        );
        Ok(())
    }

    /// Participant-only: the *mentee* approves the proposed successor.
    /// Approvals after the proposal window fail with `ProposalExpired` —
    /// stale approvals never take effect.
    pub fn approve_successor(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut request = Self::load_request(&env, &request_id)?;
        let record = Self::load_match(&env, &request.match_id)?;
        // Continuity control: the mentee — not the mentor who is leaving
        // — chooses whether the successor is acceptable.
        if caller != record.mentee {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        if request.status == RequestStatus::ApprovedForClosure {
            return Ok(());
        }
        if request.status != RequestStatus::SuccessorProposed {
            return Err(ContractError::InvalidRequestState);
        }
        let now = env.ledger().timestamp();
        if now > request.proposal_expires_at {
            return Err(ContractError::ProposalExpired);
        }
        request.successor_approved_at = now;
        request.status = RequestStatus::ApprovedForClosure;
        Self::put_request(&env, &request);
        env.events()
            .publish((symbol_short!("RMAPPR"), request_id), (caller, now));
        Ok(())
    }

    /// Coordinator-only: close the engagement without a successor. Open
    /// financial or safety cases block the closure. The match record is
    /// preserved in full — only its status and timestamp change.
    pub fn close_engagement(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        let mut request = Self::load_request(&env, &request_id)?;
        if request.status != RequestStatus::InReview
            && request.status != RequestStatus::SuccessorProposed
            && request.status != RequestStatus::ApprovedForClosure
        {
            return Err(ContractError::InvalidRequestState);
        }
        let mut record = Self::load_match(&env, &request.match_id)?;
        Self::require_no_open_cases(&env, &record.match_id)?;
        let now = env.ledger().timestamp();
        record.status = MatchStatus::Closed;
        record.closed_at = now;
        Self::put_match(&env, &record);
        request.status = RequestStatus::Closed;
        request.closed_at = now;
        Self::put_request(&env, &request);
        Self::bump_stats(&env, |stats| {
            stats.open_requests = stats.open_requests.saturating_sub(1);
            stats.closed_requests = stats.closed_requests.saturating_add(1);
        });
        env.events().publish(
            (symbol_short!("RMCLSE"), request_id),
            (record.match_id, now),
        );
        Ok(())
    }

    /// Coordinator-only: supersede the match with an approved successor
    /// match. The old record stays intact with a successor pointer; the
    /// new match records the match it superseded, so continuity is
    /// traceable in both directions. Blocked by open cases.
    pub fn apply_successor(
        env: Env,
        caller: Address,
        request_id: BytesN<32>,
        successor_match_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_coordinator(&env, &caller)?;
        let mut request = Self::load_request(&env, &request_id)?;
        if request.status != RequestStatus::ApprovedForClosure {
            return Err(ContractError::InvalidRequestState);
        }
        let mut record = Self::load_match(&env, &request.match_id)?;
        Self::require_no_open_cases(&env, &record.match_id)?;
        let successor = request
            .successor
            .clone()
            .ok_or(ContractError::InvalidRequestState)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Match(successor_match_id.clone()))
        {
            return Err(ContractError::MatchAlreadyRegistered);
        }

        let now = env.ledger().timestamp();
        let new_record = MatchRecord {
            match_id: successor_match_id.clone(),
            agreement_id: record.agreement_id.clone(),
            mentor: successor,
            mentee: record.mentee.clone(),
            status: MatchStatus::Active,
            successor_match_id: None,
            superseded_match_id: Some(record.match_id.clone()),
            created_at: now,
            closed_at: 0,
        };
        Self::put_match(&env, &new_record);

        record.status = MatchStatus::Superseded;
        record.successor_match_id = Some(successor_match_id.clone());
        record.closed_at = now;
        Self::put_match(&env, &record);

        request.status = RequestStatus::Superseded;
        request.closed_at = now;
        Self::put_request(&env, &request);

        Self::bump_stats(&env, |stats| {
            stats.total_matches = stats.total_matches.saturating_add(1);
            stats.open_requests = stats.open_requests.saturating_sub(1);
            stats.closed_requests = stats.closed_requests.saturating_add(1);
            stats.superseded_matches = stats.superseded_matches.saturating_add(1);
        });
        env.events().publish(
            (symbol_short!("RMSUPER"), request_id),
            (record.match_id, successor_match_id),
        );
        Ok(())
    }

    // ── views ───────────────────────────────────────────────────────────

    pub fn get_match(env: Env, match_id: BytesN<32>) -> Result<MatchRecord, ContractError> {
        Self::load_match(&env, &match_id)
    }

    /// Bounded index of the rematch requests filed against a match.
    pub fn match_requests(
        env: Env,
        match_id: BytesN<32>,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        let key = DataKey::MatchRequests(match_id);
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        Ok(ids)
    }

    /// Whether financial or safety cases are open on a match. Only the
    /// existence of a case is public; details stay off-chain.
    pub fn get_case_flags(env: Env, match_id: BytesN<32>) -> Result<CaseFlags, ContractError> {
        Self::load_match(&env, &match_id)?;
        let key = DataKey::CaseFlags(match_id);
        let flags: CaseFlags = env.storage().persistent().get(&key).unwrap_or(CaseFlags {
            financial_open: false,
            safety_open: false,
        });
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        Ok(flags)
    }

    /// Aggregate rematch metrics — counters only, never a blame score.
    pub fn stats(env: Env) -> Result<RematchStats, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Stats)
            .ok_or(ContractError::NotInitialized)
    }

    /// Admin-only: upgrade the contract to `new_wasm_hash`. Additive
    /// increments (further continuity controls) keep `version()` 1 and
    /// never change existing entry points or event schemas.
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

    fn require_coordinator(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if !Self::is_coordinator(env.clone(), caller.clone()) {
            return Err(ContractError::NotCoordinator);
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

    fn validate_window(seconds: u32) -> Result<(), ContractError> {
        if !(MIN_PROPOSAL_WINDOW_SECONDS..=MAX_PROPOSAL_WINDOW_SECONDS).contains(&seconds) {
            return Err(ContractError::InvalidConfig);
        }
        Ok(())
    }

    fn require_no_open_cases(env: &Env, match_id: &BytesN<32>) -> Result<(), ContractError> {
        let flags: CaseFlags = env
            .storage()
            .persistent()
            .get(&DataKey::CaseFlags(match_id.clone()))
            .unwrap_or(CaseFlags {
                financial_open: false,
                safety_open: false,
            });
        if flags.financial_open || flags.safety_open {
            return Err(ContractError::UnsafeClosureBlocked);
        }
        Ok(())
    }

    fn load_match(env: &Env, match_id: &BytesN<32>) -> Result<MatchRecord, ContractError> {
        let key = DataKey::Match(match_id.clone());
        let record: MatchRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::MatchNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    fn put_match(env: &Env, record: &MatchRecord) {
        let key = DataKey::Match(record.match_id.clone());
        env.storage().persistent().set(&key, record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_request(env: &Env, request_id: &BytesN<32>) -> Result<RematchRequest, ContractError> {
        let key = DataKey::Request(request_id.clone());
        let request: RematchRequest = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::RematchRequestNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(request)
    }

    fn put_request(env: &Env, request: &RematchRequest) {
        let key = DataKey::Request(request.request_id.clone());
        env.storage().persistent().set(&key, request);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn bump_stats(env: &Env, f: impl FnOnce(&mut RematchStats)) {
        let mut stats: RematchStats =
            env.storage()
                .instance()
                .get(&DataKey::Stats)
                .unwrap_or(RematchStats {
                    total_matches: 0,
                    open_requests: 0,
                    closed_requests: 0,
                    superseded_matches: 0,
                });
        f(&mut stats);
        env.storage().instance().set(&DataKey::Stats, &stats);
    }
}

/// An all-zero commitment, used as the "missing" sentinel.
fn zero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[cfg(test)]
mod tests;
