#![no_std]

//! Scholarship/bursary award-decision contract.
//!
//! Scope of this pass (issues #1087, #1088, #1089):
//!
//! - **#1087 — Committee decision workflow.** Authorized committee members
//!   (`Chair` opens a decision; any active member votes) stage an application
//!   through shortlist/waitlist/award/reject, with recusal supported, a
//!   configurable quorum of distinct non-recused votes required before an
//!   outcome is finalized, and every decision referencing the version of the
//!   evidence it was taken on. Every open/vote/finalize action is stored
//!   (bounded) and emitted as an event, so the full trail is auditable.
//! - **#1088 — Reserve budget during award decisions.** When an award is
//!   approved, the program's budget is held as a per-applicant `Reservation`
//!   *before* the applicant accepts. Reservations are created atomically,
//!   never let committed budget exceed the program total, expire by policy,
//!   and release capacity exactly once.
//! - **#1089 — Applicant appeals.** A bounded appeal may be filed by the
//!   affected applicant within a policy window, against an eligible
//!   (rejected/waitlisted) finalized decision, with grounds and evidence
//!   commitments. Members who took part in the original decision are excluded
//!   from the appeal vote, the appeal decision is stored separately, and the
//!   appeal outcome is final and supersedes the original decision.
//!
//! Privacy-minimized by design: applications, reviews, evidence bundles and
//! appeal grounds all live off-chain. On-chain this contract stores only
//! addresses, decision/vote metadata, numeric versions and caller-supplied
//! `BytesN<32>` integrity commitments over the off-chain content — never the
//! content itself. See `contracts/docs/scholarship-decisions.md` for
//! ownership, privacy, migration, and operational notes.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Bounded storage: at most this many distinct members may ever be added to
/// the committee, so election/quorum cost stays predictable.
const MAX_MEMBERS: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// #1087 — the caller is not an active committee `Chair`.
    NotChair = 4,
    /// #1087 — the caller is not an active committee member.
    NotCommitteeMember = 5,
    CommitteeFull = 6,
    /// #1087 — quorum must be between 1 and MAX_MEMBERS.
    InvalidQuorum = 7,
    /// #1088 — budget/amount/ttl/window values must be positive and the
    /// total budget must be able to cover at least one award.
    InvalidProgramConfig = 8,
    ProgramNotFound = 9,
    ProgramAlreadyExists = 10,
    ProgramInactive = 11,
    /// #1088 — reserving this award would push committed budget past the total.
    BudgetExceeded = 12,
    ArithmeticOverflow = 13,
    DecisionNotFound = 14,
    DecisionAlreadyExists = 15,
    /// #1087 — the decision has already been finalized (votes are closed).
    DecisionAlreadyFinalized = 16,
    /// #1087 — the decision has not been finalized yet.
    DecisionNotFinalized = 17,
    /// #1089 — only rejected/waitlisted decisions may be appealed.
    DecisionNotAppealable = 18,
    /// A member may cast at most one vote per decision/appeal.
    AlreadyVoted = 19,
    /// #1087 — fewer than `quorum` distinct non-recused votes were cast.
    QuorumNotMet = 20,
    ReservationNotFound = 21,
    /// #1088 — the reservation has already been claimed, released or expired.
    ReservationNotActive = 22,
    /// #1088 — the reservation has passed its expiry and must be expired.
    ReservationExpired = 23,
    /// #1088 — the reservation has not yet expired.
    ReservationNotExpired = 24,
    AppealNotFound = 25,
    /// #1089 — a decision may be appealed at most once.
    AppealAlreadyExists = 26,
    /// #1089 — the appeal was filed after the program's appeal window closed.
    AppealWindowClosed = 27,
    AppealNotOpen = 28,
    /// #1089 — members who voted on the original decision cannot vote on its appeal.
    AppealReviewerIneligible = 29,
    /// Evidence references must be well formed (a version >= 1).
    InvalidEvidence = 30,
    /// The caller is neither the admin nor an active committee chair.
    NotAuthorized = 31,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1087 — the minimum number of distinct cast (non-recused) votes
    /// required before a decision or appeal may be finalized.
    Quorum,
    /// #1087 — whether an address is currently an active committee member.
    Member(Address),
    /// #1087 — an address's committee role (retained across deactivation).
    MemberRole(Address),
    /// #1087 — bounded list of every address ever added to the committee.
    MemberList,
    /// #1088 — a program's decision/budget configuration.
    ProgramConfig(BytesN<32>),
    /// #1087 — an application's decision record.
    Decision(BytesN<32>, Address),
    /// #1088 — an award's budget reservation.
    Reservation(BytesN<32>, Address),
    /// #1089 — an application's appeal record.
    Appeal(BytesN<32>, Address),
}

/// #1087 — the two committee roles. `Chair` may open decisions (and finalize
/// them); both roles may vote.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitteeRole {
    Reviewer,
    Chair,
}

/// #1087 — whether a decision is still accepting votes or has been finalized.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionStatus {
    Open,
    Finalized,
}

/// #1087 — a decision's outcome. `Pending` until finalized.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutcome {
    Pending,
    Awarded,
    Waitlisted,
    Rejected,
}

/// #1087 — a committee member's action on a decision or appeal. `Recuse`
/// withdraws the member from the vote and excludes them from quorum.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoteChoice {
    Award,
    Waitlist,
    Reject,
    Recuse,
}

/// #1088 — lifecycle of a budget reservation.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationStatus {
    Reserved,
    Claimed,
    Released,
    Expired,
}

/// #1089 — whether an appeal is still accepting votes or has been resolved.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppealStatus {
    Open,
    Resolved,
}

/// #1087/#1088 — a program's decision and budget configuration. The
/// `reserved_amount`/`awarded_amount` counters are the authoritative budget
/// state — never derived from anything off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionProgramConfig {
    pub active: bool,
    pub total_budget: i128,
    pub per_award_amount: i128,
    /// Sum of currently-held reservations (approved but not yet accepted).
    pub reserved_amount: i128,
    /// Sum of claimed awards (accepted by an applicant).
    pub awarded_amount: i128,
    /// Seconds a reservation is held before it may be expired.
    pub reservation_ttl: u64,
    /// Seconds after finalization during which an appeal may be filed.
    pub appeal_window: u64,
}

/// #1087 — one committee member's recorded action.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vote {
    pub member: Address,
    pub choice: VoteChoice,
    pub cast_at: u64,
}

/// #1087 — an application's decision record. `evidence_version`/
/// `evidence_hash` reference the review evidence (from the reviews contract,
/// #1086) the decision was taken on; `outcome_version` increments when an
/// appeal supersedes the outcome, so the current outcome is always traceable.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub program_id: BytesN<32>,
    pub applicant: Address,
    pub status: DecisionStatus,
    pub outcome: DecisionOutcome,
    pub outcome_version: u32,
    pub evidence_version: u32,
    pub evidence_hash: BytesN<32>,
    pub opened_at: u64,
    pub finalized_at: u64,
    pub votes: Vec<Vote>,
}

/// #1088 — a held award reservation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reservation {
    pub program_id: BytesN<32>,
    pub applicant: Address,
    pub amount: i128,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: ReservationStatus,
}

/// #1089 — an appeal against a finalized decision, stored separately from the
/// decision it challenges. `original_outcome` preserves what was appealed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Appeal {
    pub program_id: BytesN<32>,
    pub applicant: Address,
    pub original_outcome: DecisionOutcome,
    pub status: AppealStatus,
    pub outcome: DecisionOutcome,
    pub grounds_hash: BytesN<32>,
    pub evidence_hash: BytesN<32>,
    pub filed_at: u64,
    pub resolved_at: u64,
    pub votes: Vec<Vote>,
}

#[contract]
pub struct ScholarshipDecisionsContract;

#[contractimpl]
impl ScholarshipDecisionsContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
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

    fn is_active_member(env: &Env, member: &Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Member(member.clone()))
            .unwrap_or(false)
    }

    fn require_member(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if !Self::is_active_member(env, caller) {
            return Err(ContractError::NotCommitteeMember);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_chair(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let role: Option<CommitteeRole> = env
            .storage()
            .persistent()
            .get(&DataKey::MemberRole(caller.clone()));
        if !Self::is_active_member(env, caller) || role != Some(CommitteeRole::Chair) {
            return Err(ContractError::NotChair);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_chair_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let role: Option<CommitteeRole> = env
            .storage()
            .persistent()
            .get(&DataKey::MemberRole(caller.clone()));
        if Self::is_active_member(env, caller) && role == Some(CommitteeRole::Chair) {
            caller.require_auth();
            return Ok(());
        }
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::NotInitialized);
        }
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller == admin {
            caller.require_auth();
            return Ok(());
        }
        Err(ContractError::NotAuthorized)
    }

    fn list_contains(list: &Vec<Address>, needle: &Address) -> bool {
        for item in list.iter() {
            if item == *needle {
                return true;
            }
        }
        false
    }

    fn has_voted(votes: &Vec<Vote>, member: &Address) -> bool {
        for vote in votes.iter() {
            if vote.member == *member {
                return true;
            }
        }
        false
    }

    /// Counts non-recused votes by choice; recusals are excluded so they do
    /// not count toward quorum.
    fn tally(votes: &Vec<Vote>) -> (u32, u32, u32) {
        let mut award: u32 = 0;
        let mut waitlist: u32 = 0;
        let mut reject: u32 = 0;
        for vote in votes.iter() {
            match vote.choice {
                VoteChoice::Award => award += 1,
                VoteChoice::Waitlist => waitlist += 1,
                VoteChoice::Reject => reject += 1,
                VoteChoice::Recuse => {}
            }
        }
        (award, waitlist, reject)
    }

    /// #1087 — documented tie policy: the most-supported outcome wins; ties
    /// resolve in favor of the most conservative outcome, in the order
    /// `Rejected` > `Waitlisted` > `Awarded`. Deterministic — depends only on
    /// the recorded votes.
    fn decide_outcome(award: u32, waitlist: u32, reject: u32) -> DecisionOutcome {
        let mut outcome = DecisionOutcome::Awarded;
        let mut best = award;
        if waitlist >= best {
            best = waitlist;
            outcome = DecisionOutcome::Waitlisted;
        }
        if reject >= best {
            outcome = DecisionOutcome::Rejected;
        }
        outcome
    }

    // ── #1087 — committee membership and roles ────────────────────────────

    /// Admin-only: add (or re-activate) a committee member with a role.
    pub fn add_member(
        env: Env,
        admin: Address,
        member: Address,
        role: CommitteeRole,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let list_key = DataKey::MemberList;
        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or(Vec::new(&env));
        if !Self::list_contains(&list, &member) {
            if list.len() >= MAX_MEMBERS {
                return Err(ContractError::CommitteeFull);
            }
            list.push_back(member.clone());
            env.storage().persistent().set(&list_key, &list);
            env.storage()
                .persistent()
                .extend_ttl(&list_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }

        let active_key = DataKey::Member(member.clone());
        env.storage().persistent().set(&active_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&active_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let role_key = DataKey::MemberRole(member.clone());
        env.storage().persistent().set(&role_key, &role);
        env.storage()
            .persistent()
            .extend_ttl(&role_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("CMEMBR"),), (member,));
        Ok(())
    }

    /// Admin-only: deactivate a member. Their role record is retained; they
    /// stop counting toward membership and quorum.
    pub fn remove_member(
        env: Env,
        admin: Address,
        member: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let active_key = DataKey::Member(member.clone());
        env.storage().persistent().set(&active_key, &false);
        env.storage()
            .persistent()
            .extend_ttl(&active_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("CMEMBR"),), (member,));
        Ok(())
    }

    /// Admin-only: set the number of distinct cast votes required to
    /// finalize a decision or appeal.
    pub fn set_quorum(env: Env, admin: Address, quorum: u32) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if quorum == 0 || quorum > MAX_MEMBERS {
            return Err(ContractError::InvalidQuorum);
        }
        env.storage().instance().set(&DataKey::Quorum, &quorum);
        env.events()
            .publish((soroban_sdk::symbol_short!("CQUORUM"),), (quorum,));
        Ok(())
    }

    pub fn get_quorum(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Quorum)
            .unwrap_or(0)
    }

    pub fn is_member(env: Env, member: Address) -> bool {
        Self::is_active_member(&env, &member)
    }

    pub fn get_member_role(
        env: Env,
        member: Address,
    ) -> Result<CommitteeRole, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::MemberRole(member))
            .ok_or(ContractError::NotCommitteeMember)
    }

    pub fn get_active_member_count(env: Env) -> u32 {
        let list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::MemberList)
            .unwrap_or(Vec::new(&env));
        let mut count: u32 = 0;
        let mut i: u32 = 0;
        while i < list.len() {
            if let Some(member) = list.get(i) {
                if Self::is_active_member(&env, &member) {
                    count += 1;
                }
            }
            i += 1;
        }
        count
    }

    // ── #1088 — program budget configuration ─────────────────────────────

    /// Admin-only: register a program's decision budget and policies.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        total_budget: i128,
        per_award_amount: i128,
        reservation_ttl: u64,
        appeal_window: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        if total_budget <= 0
            || per_award_amount <= 0
            || per_award_amount > total_budget
            || reservation_ttl == 0
            || appeal_window == 0
        {
            return Err(ContractError::InvalidProgramConfig);
        }

        let key = DataKey::ProgramConfig(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        let config = DecisionProgramConfig {
            active: true,
            total_budget,
            per_award_amount,
            reserved_amount: 0,
            awarded_amount: 0,
            reservation_ttl,
            appeal_window,
        };
        env.storage().persistent().set(&key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("PROGREG"),), (program_id,));
        Ok(())
    }

    /// Admin-only: activate/deactivate a program's decision workflow.
    pub fn set_program_active(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::ProgramConfig(program_id.clone());
        let mut config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        config.active = active;
        env.storage().persistent().set(&key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_program_config(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<DecisionProgramConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    // ── #1087 — decision workflow ────────────────────────────────────────

    /// Chair-only: open a decision for an application, referencing the
    /// evidence version/hash it will be judged on. One decision per
    /// (program, applicant).
    pub fn open_decision(
        env: Env,
        chair: Address,
        program_id: BytesN<32>,
        applicant: Address,
        evidence_version: u32,
        evidence_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_chair(&env, &chair)?;

        let config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;
        if !config.active {
            return Err(ContractError::ProgramInactive);
        }
        if evidence_version == 0 {
            return Err(ContractError::InvalidEvidence);
        }

        let key = DataKey::Decision(program_id.clone(), applicant.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::DecisionAlreadyExists);
        }

        let decision = Decision {
            program_id: program_id.clone(),
            applicant: applicant.clone(),
            status: DecisionStatus::Open,
            outcome: DecisionOutcome::Pending,
            outcome_version: 0,
            evidence_version,
            evidence_hash,
            opened_at: env.ledger().timestamp(),
            finalized_at: 0,
            votes: Vec::new(&env),
        };
        env.storage().persistent().set(&key, &decision);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("DECOPEN"),),
            (program_id, applicant),
        );
        Ok(())
    }

    /// Member-only: record one member's action on an open decision. A member
    /// votes at most once; use `recuse` (or `VoteChoice::Recuse`) to withdraw
    /// from the vote instead of choosing an outcome.
    pub fn vote(
        env: Env,
        member: Address,
        program_id: BytesN<32>,
        applicant: Address,
        choice: VoteChoice,
    ) -> Result<(), ContractError> {
        Self::require_member(&env, &member)?;

        let key = DataKey::Decision(program_id.clone(), applicant.clone());
        let mut decision: Decision = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::DecisionNotFound)?;
        if decision.status != DecisionStatus::Open {
            return Err(ContractError::DecisionAlreadyFinalized);
        }
        if Self::has_voted(&decision.votes, &member) {
            return Err(ContractError::AlreadyVoted);
        }

        decision.votes.push_back(Vote {
            member: member.clone(),
            choice,
            cast_at: env.ledger().timestamp(),
        });
        env.storage().persistent().set(&key, &decision);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("DECVOTE"),),
            (program_id, applicant, member, choice),
        );
        Ok(())
    }

    /// Member-only: recuse from an open decision (equivalent to voting
    /// `VoteChoice::Recuse`).
    pub fn recuse(
        env: Env,
        member: Address,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<(), ContractError> {
        Self::vote(env, member, program_id, applicant, VoteChoice::Recuse)
    }

    /// Chair/admin: finalize an open decision once quorum is met. The
    /// outcome is chosen by the documented tie policy; an `Awarded` outcome
    /// atomically reserves the program's per-award budget (#1088).
    pub fn finalize_decision(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<(), ContractError> {
        Self::require_chair_or_admin(&env, &caller)?;

        let key = DataKey::Decision(program_id.clone(), applicant.clone());
        let mut decision: Decision = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::DecisionNotFound)?;
        if decision.status != DecisionStatus::Open {
            return Err(ContractError::DecisionAlreadyFinalized);
        }

        let quorum: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Quorum)
            .unwrap_or(0);
        let (award, waitlist, reject) = Self::tally(&decision.votes);
        let cast = award
            .checked_add(waitlist)
            .and_then(|v| v.checked_add(reject))
            .ok_or(ContractError::ArithmeticOverflow)?;
        if quorum == 0 || cast < quorum {
            return Err(ContractError::QuorumNotMet);
        }

        let outcome = Self::decide_outcome(award, waitlist, reject);
        if outcome == DecisionOutcome::Awarded {
            let config: DecisionProgramConfig = env
                .storage()
                .persistent()
                .get(&DataKey::ProgramConfig(program_id.clone()))
                .ok_or(ContractError::ProgramNotFound)?;
            Self::reserve_budget(&env, &program_id, &applicant, config.per_award_amount)?;
        }

        decision.outcome = outcome;
        decision.status = DecisionStatus::Finalized;
        decision.finalized_at = env.ledger().timestamp();
        decision.outcome_version = 1;
        env.storage().persistent().set(&key, &decision);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("DECFIN"),),
            (program_id, applicant, outcome),
        );
        Ok(())
    }

    pub fn get_decision(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<Decision, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Decision(program_id, applicant))
            .ok_or(ContractError::DecisionNotFound)
    }

    // ── #1088 — budget reservations ──────────────────────────────────────

    /// Atomically hold one award's budget for `applicant`. Fails rather than
    /// over-committing when `reserved + awarded + amount` would exceed
    /// `total_budget`; creates a reservation that expires after the
    /// program's `reservation_ttl`.
    fn reserve_budget(
        env: &Env,
        program_id: &BytesN<32>,
        applicant: &Address,
        amount: i128,
    ) -> Result<(), ContractError> {
        let config_key = DataKey::ProgramConfig(program_id.clone());
        let mut config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&config_key)
            .ok_or(ContractError::ProgramNotFound)?;

        let committed = config
            .reserved_amount
            .checked_add(config.awarded_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let after = committed
            .checked_add(amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if after > config.total_budget {
            return Err(ContractError::BudgetExceeded);
        }
        config.reserved_amount = config
            .reserved_amount
            .checked_add(amount)
            .ok_or(ContractError::ArithmeticOverflow)?;

        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(config.reservation_ttl)
            .ok_or(ContractError::ArithmeticOverflow)?;

        env.storage().persistent().set(&config_key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&config_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        let reservation = Reservation {
            program_id: program_id.clone(),
            applicant: applicant.clone(),
            amount,
            created_at: now,
            expires_at,
            status: ReservationStatus::Reserved,
        };
        let res_key = DataKey::Reservation(program_id.clone(), applicant.clone());
        env.storage().persistent().set(&res_key, &reservation);
        env.storage()
            .persistent()
            .extend_ttl(&res_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("DECRSV"),),
            (program_id.clone(), applicant.clone(), amount),
        );
        Ok(())
    }

    fn release_capacity(
        env: &Env,
        program_id: &BytesN<32>,
        amount: i128,
    ) -> Result<(), ContractError> {
        let config_key = DataKey::ProgramConfig(program_id.clone());
        let mut config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&config_key)
            .ok_or(ContractError::ProgramNotFound)?;
        config.reserved_amount = config
            .reserved_amount
            .checked_sub(amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        env.storage().persistent().set(&config_key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&config_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Applicant-only: accept an award, converting the held reservation into
    /// an awarded amount. Fails once the reservation has expired.
    pub fn accept_award(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let res_key = DataKey::Reservation(program_id.clone(), applicant.clone());
        let mut reservation: Reservation = env
            .storage()
            .persistent()
            .get(&res_key)
            .ok_or(ContractError::ReservationNotFound)?;
        if reservation.status != ReservationStatus::Reserved {
            return Err(ContractError::ReservationNotActive);
        }
        if env.ledger().timestamp() > reservation.expires_at {
            return Err(ContractError::ReservationExpired);
        }

        let config_key = DataKey::ProgramConfig(program_id.clone());
        let mut config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&config_key)
            .ok_or(ContractError::ProgramNotFound)?;
        config.reserved_amount = config
            .reserved_amount
            .checked_sub(reservation.amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        config.awarded_amount = config
            .awarded_amount
            .checked_add(reservation.amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        env.storage().persistent().set(&config_key, &config);
        env.storage()
            .persistent()
            .extend_ttl(&config_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        reservation.status = ReservationStatus::Claimed;
        env.storage().persistent().set(&res_key, &reservation);
        env.storage()
            .persistent()
            .extend_ttl(&res_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("AWACC"),),
            (program_id, applicant, reservation.amount),
        );
        Ok(())
    }

    /// Chair/admin: release a held reservation early (e.g. the applicant
    /// declined), returning its budget to the available pool. Releasing an
    /// already-inactive reservation fails, so capacity is freed exactly once.
    pub fn release_reservation(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<(), ContractError> {
        Self::require_chair_or_admin(&env, &caller)?;

        let res_key = DataKey::Reservation(program_id.clone(), applicant.clone());
        let mut reservation: Reservation = env
            .storage()
            .persistent()
            .get(&res_key)
            .ok_or(ContractError::ReservationNotFound)?;
        if reservation.status != ReservationStatus::Reserved {
            return Err(ContractError::ReservationNotActive);
        }

        Self::release_capacity(&env, &program_id, reservation.amount)?;
        reservation.status = ReservationStatus::Released;
        env.storage().persistent().set(&res_key, &reservation);
        env.storage()
            .persistent()
            .extend_ttl(&res_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("RSVREL"),),
            (program_id, applicant),
        );
        Ok(())
    }

    /// Anyone: expire a reservation whose policy TTL has passed, returning
    /// its budget to the available pool. Callable by any address (a
    /// permissionless cleanup) but only after `expires_at`, and only once.
    pub fn expire_reservation(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let res_key = DataKey::Reservation(program_id.clone(), applicant.clone());
        let mut reservation: Reservation = env
            .storage()
            .persistent()
            .get(&res_key)
            .ok_or(ContractError::ReservationNotFound)?;
        if reservation.status != ReservationStatus::Reserved {
            return Err(ContractError::ReservationNotActive);
        }
        if env.ledger().timestamp() <= reservation.expires_at {
            return Err(ContractError::ReservationNotExpired);
        }

        Self::release_capacity(&env, &program_id, reservation.amount)?;
        reservation.status = ReservationStatus::Expired;
        env.storage().persistent().set(&res_key, &reservation);
        env.storage()
            .persistent()
            .extend_ttl(&res_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("RSVEXP"),),
            (program_id, applicant),
        );
        Ok(())
    }

    pub fn get_reservation(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<Reservation, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Reservation(program_id, applicant))
            .ok_or(ContractError::ReservationNotFound)
    }

    // ── #1089 — applicant appeals ────────────────────────────────────────

    /// Applicant-only: file one appeal against an eligible finalized
    /// decision, within the program's appeal window. `grounds_hash` and
    /// `evidence_hash` are commitments over off-chain appeal content.
    pub fn file_appeal(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        grounds_hash: BytesN<32>,
        evidence_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let config: DecisionProgramConfig = env
            .storage()
            .persistent()
            .get(&DataKey::ProgramConfig(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;
        if !config.active {
            return Err(ContractError::ProgramInactive);
        }

        let decision: Decision = env
            .storage()
            .persistent()
            .get(&DataKey::Decision(program_id.clone(), applicant.clone()))
            .ok_or(ContractError::DecisionNotFound)?;
        if decision.status != DecisionStatus::Finalized {
            return Err(ContractError::DecisionNotFinalized);
        }
        if decision.outcome != DecisionOutcome::Rejected
            && decision.outcome != DecisionOutcome::Waitlisted
        {
            return Err(ContractError::DecisionNotAppealable);
        }

        let appeal_key = DataKey::Appeal(program_id.clone(), applicant.clone());
        if env.storage().persistent().has(&appeal_key) {
            return Err(ContractError::AppealAlreadyExists);
        }

        let deadline = decision
            .finalized_at
            .checked_add(config.appeal_window)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let now = env.ledger().timestamp();
        if now > deadline {
            return Err(ContractError::AppealWindowClosed);
        }

        let appeal = Appeal {
            program_id: program_id.clone(),
            applicant: applicant.clone(),
            original_outcome: decision.outcome,
            status: AppealStatus::Open,
            outcome: DecisionOutcome::Pending,
            grounds_hash,
            evidence_hash,
            filed_at: now,
            resolved_at: 0,
            votes: Vec::new(&env),
        };
        env.storage().persistent().set(&appeal_key, &appeal);
        env.storage()
            .persistent()
            .extend_ttl(&appeal_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("APPEAL"),),
            (program_id, applicant),
        );
        Ok(())
    }

    /// Member-only: vote on an open appeal. Members who voted on the
    /// original decision are excluded, so an appeal is judged by a fresh
    /// panel.
    pub fn vote_appeal(
        env: Env,
        member: Address,
        program_id: BytesN<32>,
        applicant: Address,
        choice: VoteChoice,
    ) -> Result<(), ContractError> {
        Self::require_member(&env, &member)?;

        let appeal_key = DataKey::Appeal(program_id.clone(), applicant.clone());
        let mut appeal: Appeal = env
            .storage()
            .persistent()
            .get(&appeal_key)
            .ok_or(ContractError::AppealNotFound)?;
        if appeal.status != AppealStatus::Open {
            return Err(ContractError::AppealNotOpen);
        }

        let decision: Decision = env
            .storage()
            .persistent()
            .get(&DataKey::Decision(program_id.clone(), applicant.clone()))
            .ok_or(ContractError::DecisionNotFound)?;
        if Self::has_voted(&decision.votes, &member) {
            return Err(ContractError::AppealReviewerIneligible);
        }
        if Self::has_voted(&appeal.votes, &member) {
            return Err(ContractError::AlreadyVoted);
        }

        appeal.votes.push_back(Vote {
            member: member.clone(),
            choice,
            cast_at: env.ledger().timestamp(),
        });
        env.storage().persistent().set(&appeal_key, &appeal);
        env.storage()
            .persistent()
            .extend_ttl(&appeal_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("APVOTE"),),
            (program_id, applicant, member, choice),
        );
        Ok(())
    }

    /// Chair/admin: finalize an open appeal once quorum (over the eligible,
    /// non-original panel) is met. The appeal outcome supersedes the
    /// decision's outcome and is final — no further appeal may be filed.
    pub fn finalize_appeal(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<(), ContractError> {
        Self::require_chair_or_admin(&env, &caller)?;

        let appeal_key = DataKey::Appeal(program_id.clone(), applicant.clone());
        let mut appeal: Appeal = env
            .storage()
            .persistent()
            .get(&appeal_key)
            .ok_or(ContractError::AppealNotFound)?;
        if appeal.status != AppealStatus::Open {
            return Err(ContractError::AppealNotOpen);
        }

        let quorum: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Quorum)
            .unwrap_or(0);
        let (award, waitlist, reject) = Self::tally(&appeal.votes);
        let cast = award
            .checked_add(waitlist)
            .and_then(|v| v.checked_add(reject))
            .ok_or(ContractError::ArithmeticOverflow)?;
        if quorum == 0 || cast < quorum {
            return Err(ContractError::QuorumNotMet);
        }

        let outcome = Self::decide_outcome(award, waitlist, reject);

        if outcome == DecisionOutcome::Awarded {
            let res_key = DataKey::Reservation(program_id.clone(), applicant.clone());
            if !env.storage().persistent().has(&res_key) {
                let config: DecisionProgramConfig = env
                    .storage()
                    .persistent()
                    .get(&DataKey::ProgramConfig(program_id.clone()))
                    .ok_or(ContractError::ProgramNotFound)?;
                Self::reserve_budget(&env, &program_id, &applicant, config.per_award_amount)?;
            }
        }

        let decision_key = DataKey::Decision(program_id.clone(), applicant.clone());
        let mut decision: Decision = env
            .storage()
            .persistent()
            .get(&decision_key)
            .ok_or(ContractError::DecisionNotFound)?;
        decision.outcome = outcome;
        decision.outcome_version = decision
            .outcome_version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        env.storage().persistent().set(&decision_key, &decision);
        env.storage()
            .persistent()
            .extend_ttl(&decision_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        appeal.status = AppealStatus::Resolved;
        appeal.outcome = outcome;
        appeal.resolved_at = env.ledger().timestamp();
        env.storage().persistent().set(&appeal_key, &appeal);
        env.storage()
            .persistent()
            .extend_ttl(&appeal_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("APFIN"),),
            (program_id, applicant, outcome),
        );
        Ok(())
    }

    pub fn get_appeal(
        env: Env,
        program_id: BytesN<32>,
        applicant: Address,
    ) -> Result<Appeal, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Appeal(program_id, applicant))
            .ok_or(ContractError::AppealNotFound)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
