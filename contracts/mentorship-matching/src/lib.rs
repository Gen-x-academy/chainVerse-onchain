#![no_std]

//! Mentor match proposals (MENTOR-037, issue #1233) and
//! coordinator-assisted matching (MENTOR-039, issue #1235).
//!
//! The contract owns the *matching* half of the mentorship workflow:
//!
//! * a minimal participant registry (role + eligibility status) and a
//!   pairwise block list, so hard eligibility and blocks can never be
//!   bypassed by a coordinator;
//! * mentor-initiated match proposals carrying only a terms commitment
//!   (`BytesN<32>`), the proposed fee, and an expiry — never free-form
//!   compatibility notes;
//! * participant consent bits, so an adjusted proposal must be re-approved
//!   by both parties;
//! * an append-only, per-coordinator action log recording every
//!   coordinator-created or coordinator-adjusted proposal together with a
//!   reason commitment, making overrides reportable.
//!
//! Mutual-acceptance *product* behaviour beyond the two consent bits
//! (notifications, cooling-off, match confirmation, rematching) is tracked
//! by MENTOR-038 and the rematch/feedback issues; this contract only
//! exposes the consent primitive those flows need.
//!
//! See `contracts/docs/mentorship-matching.md` for the privacy boundary,
//! dependencies, and rollout notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env,
};

/// Persistent entries are bumped to at least ~180 days and kept for at
/// most ~360 days, matching the rest of the contract suite.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Storage bound: at most this many proposals may be awaiting approval for
/// one program at a time.
const MAX_PENDING_PROPOSALS_PER_PROGRAM: u32 = 128;

/// A proposal may not be left open longer than 90 days.
const MAX_PROPOSAL_LIFETIME: u64 = 90 * 24 * 60 * 60;

/// Storage/reporting bound: the per-coordinator action log is capped so its
/// size is deterministic. Export and reset the log off-chain once reached.
const MAX_COORDINATOR_ACTIONS: u32 = 1_024;

const ZERO_HASH: [u8; 32] = [0u8; 32];

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum MatchingError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    ParticipantNotFound = 4,
    ParticipantInactive = 5,
    RoleMismatch = 6,
    SelfMatch = 7,
    Blocked = 8,
    InvalidTerms = 9,
    InvalidExpiry = 10,
    ProposalNotFound = 11,
    DuplicateActiveProposal = 12,
    ProposalExpired = 13,
    InvalidTransition = 14,
    NotParticipant = 15,
    AlreadyApproved = 16,
    StaleProposalVersion = 17,
    ReasonRequired = 18,
    InvalidReasonCode = 19,
    ProgramCapacityReached = 20,
    ActionLogFull = 21,
    CounterOverflow = 22,
    ParticipantExists = 23,
    InvalidProgram = 24,
    ReasonNotFound = 25,
    ActionNotFound = 26,
}

/// Which side of a match a participant may occupy. A single address cannot
/// hold both roles in this contract.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantRole {
    Mentor,
    Mentee,
}

/// Hard eligibility state. Only `Active` participants can be proposed or
/// approve a proposal.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantStatus {
    Active,
    Inactive,
    Suspended,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Participant {
    pub role: ParticipantRole,
    pub status: ParticipantStatus,
    pub updated_at: u64,
}

/// Who originated a proposal. Coordinator-originated proposals always carry
/// a reason commitment and are logged for reporting.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalOrigin {
    Mentor,
    Coordinator,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    /// Waiting for one or both participants to approve.
    Proposed,
    /// Both participants approved; the match is agreed.
    Active,
    /// A participant declined.
    Declined,
    /// The expiry passed before mutual approval; finalized lazily.
    Expired,
}

impl ProposalStatus {
    fn is_open(self) -> bool {
        matches!(self, ProposalStatus::Proposed | ProposalStatus::Active)
    }
}

/// The on-chain portion of proposed terms. Compatibility reasons, session
/// plans, and any other narrative stay off-chain; only an integrity
/// commitment, the fee and the expiry are committed here.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalTerms {
    pub terms_hash: BytesN<32>,
    pub proposed_fee: i128,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub terms_hash: BytesN<32>,
    pub proposed_fee: i128,
    pub expires_at: u64,
    /// The initiator (the mentor, or the coordinator for assisted matches).
    pub created_by: Address,
    pub origin: ProposalOrigin,
    /// The last coordinator who adjusted this proposal, if any.
    pub last_adjusted_by: Option<Address>,
    pub status: ProposalStatus,
    /// Incremented on every coordinator adjustment; approvals carry the
    /// version they were given for.
    pub version: u32,
    pub mentor_approved: bool,
    pub mentee_approved: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorActionKind {
    Proposed,
    Adjusted,
}

/// Append-only record of a coordinator's matching action, used for
/// override reporting. Only the reason *commitment* is stored.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoordinatorAction {
    pub coordinator: Address,
    pub proposal_id: BytesN<32>,
    pub kind: CoordinatorActionKind,
    pub reason_hash: BytesN<32>,
    pub version: u32,
    pub at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Coordinator(Address),
    Participant(Address),
    /// Directional block marker; blocking is enforced symmetrically.
    Blocked(Address, Address),
    ProposalCount,
    Proposal(BytesN<32>),
    /// Open proposal for a (program, mentor, mentee) triple.
    PairIndex(BytesN<32>, Address, Address),
    /// Proposals awaiting approval in a program (storage bound).
    PendingCount(BytesN<32>),
    ProposalReason(BytesN<32>),
    CoordActionCount(Address),
    CoordAction(Address, u32),
}

fn admin(env: &Env) -> Result<Address, MatchingError> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(MatchingError::NotInitialized)
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), MatchingError> {
    let admin = admin(env)?;
    if *caller != admin {
        return Err(MatchingError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn require_coordinator(env: &Env, caller: &Address) -> Result<(), MatchingError> {
    let admin = admin(env)?;
    let registered: bool = env
        .storage()
        .persistent()
        .get(&DataKey::Coordinator(caller.clone()))
        .unwrap_or(false);
    if *caller != admin && !registered {
        return Err(MatchingError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn bump_persistent(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

fn derive_id(env: &Env, salt: Bytes) -> Result<BytesN<32>, MatchingError> {
    let nonce: u64 = env
        .storage()
        .instance()
        .get(&DataKey::ProposalCount)
        .unwrap_or(0);
    let next = nonce.checked_add(1).ok_or(MatchingError::CounterOverflow)?;
    env.storage().instance().set(&DataKey::ProposalCount, &next);
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    input.append(&Bytes::from_slice(
        env,
        &env.ledger().timestamp().to_be_bytes(),
    ));
    input.append(&salt);
    Ok(env.crypto().sha256(&input).into())
}

fn load_participant(env: &Env, who: &Address) -> Result<Participant, MatchingError> {
    env.storage()
        .persistent()
        .get(&DataKey::Participant(who.clone()))
        .ok_or(MatchingError::ParticipantNotFound)
}

fn load_proposal(env: &Env, proposal_id: &BytesN<32>) -> Result<Proposal, MatchingError> {
    env.storage()
        .persistent()
        .get(&DataKey::Proposal(proposal_id.clone()))
        .ok_or(MatchingError::ProposalNotFound)
}

fn store_proposal(env: &Env, proposal: &Proposal) {
    let key = DataKey::Proposal(proposal.id.clone());
    env.storage().persistent().set(&key, proposal);
    bump_persistent(env, &key);
}

/// Blocking is symmetric: a block in *either* direction disqualifies the
/// pair, so neither side can be matched by a coordinator's override.
fn blocked(env: &Env, a: &Address, b: &Address) -> bool {
    let forward: bool = env
        .storage()
        .persistent()
        .get(&DataKey::Blocked(a.clone(), b.clone()))
        .unwrap_or(false);
    let reverse: bool = env
        .storage()
        .persistent()
        .get(&DataKey::Blocked(b.clone(), a.clone()))
        .unwrap_or(false);
    forward || reverse
}

/// Hard eligibility: both parties registered, in the right role, active, and
/// not blocked. Every creation/approval/adjustment path re-runs this, so no
/// coordinator path can bypass it.
fn require_eligible(env: &Env, mentor: &Address, mentee: &Address) -> Result<(), MatchingError> {
    let mentor_profile = load_participant(env, mentor)?;
    let mentee_profile = load_participant(env, mentee)?;
    if mentor_profile.role != ParticipantRole::Mentor
        || mentee_profile.role != ParticipantRole::Mentee
    {
        return Err(MatchingError::RoleMismatch);
    }
    if mentor_profile.status != ParticipantStatus::Active
        || mentee_profile.status != ParticipantStatus::Active
    {
        return Err(MatchingError::ParticipantInactive);
    }
    if blocked(env, mentor, mentee) {
        return Err(MatchingError::Blocked);
    }
    Ok(())
}

fn valid_terms(env: &Env, terms: &ProposalTerms) -> Result<(), MatchingError> {
    if terms.terms_hash.to_array() == ZERO_HASH || terms.proposed_fee < 0 {
        return Err(MatchingError::InvalidTerms);
    }
    let now = env.ledger().timestamp();
    if terms.expires_at <= now {
        return Err(MatchingError::InvalidExpiry);
    }
    let latest = now
        .checked_add(MAX_PROPOSAL_LIFETIME)
        .ok_or(MatchingError::InvalidExpiry)?;
    if terms.expires_at > latest {
        return Err(MatchingError::InvalidExpiry);
    }
    Ok(())
}

fn pending_count(env: &Env, program_id: &BytesN<32>) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::PendingCount(program_id.clone()))
        .unwrap_or(0)
}

fn set_pending_count(env: &Env, program_id: &BytesN<32>, count: u32) {
    let key = DataKey::PendingCount(program_id.clone());
    env.storage().persistent().set(&key, &count);
    bump_persistent(env, &key);
}

fn increase_pending(env: &Env, program_id: &BytesN<32>) -> Result<(), MatchingError> {
    let count = pending_count(env, program_id);
    if count >= MAX_PENDING_PROPOSALS_PER_PROGRAM {
        return Err(MatchingError::ProgramCapacityReached);
    }
    set_pending_count(env, program_id, count + 1);
    Ok(())
}

fn decrease_pending(env: &Env, program_id: &BytesN<32>) {
    let count = pending_count(env, program_id);
    if count > 0 {
        set_pending_count(env, program_id, count - 1);
    }
}

/// Lazily finalize a proposal whose expiry has passed, releasing its
/// pending slot exactly once.
fn expire_proposal(env: &Env, proposal: &mut Proposal) {
    if proposal.status == ProposalStatus::Proposed {
        decrease_pending(env, &proposal.program_id);
    }
    proposal.status = ProposalStatus::Expired;
    proposal.updated_at = env.ledger().timestamp();
    store_proposal(env, proposal);
}

fn record_coordinator_action(
    env: &Env,
    coordinator: &Address,
    proposal_id: &BytesN<32>,
    kind: CoordinatorActionKind,
    reason_hash: &BytesN<32>,
    version: u32,
) -> Result<u32, MatchingError> {
    let count: u32 = env
        .storage()
        .persistent()
        .get(&DataKey::CoordActionCount(coordinator.clone()))
        .unwrap_or(0);
    if count >= MAX_COORDINATOR_ACTIONS {
        return Err(MatchingError::ActionLogFull);
    }
    let seq = count.checked_add(1).ok_or(MatchingError::CounterOverflow)?;
    let action = CoordinatorAction {
        coordinator: coordinator.clone(),
        proposal_id: proposal_id.clone(),
        kind,
        reason_hash: reason_hash.clone(),
        version,
        at: env.ledger().timestamp(),
    };
    let key = DataKey::CoordAction(coordinator.clone(), seq);
    env.storage().persistent().set(&key, &action);
    bump_persistent(env, &key);
    let count_key = DataKey::CoordActionCount(coordinator.clone());
    env.storage().persistent().set(&count_key, &seq);
    bump_persistent(env, &count_key);
    env.events().publish(
        (symbol_short!("MNT_CACT"),),
        (coordinator.clone(), seq, kind as u32, proposal_id.clone()),
    );
    Ok(seq)
}

struct CreationOptions {
    created_by: Address,
    origin: ProposalOrigin,
    reason_hash: Option<BytesN<32>>,
}

/// Shared proposal-creation path for mentor-initiated (MENTOR-037) and
/// coordinator-assisted (MENTOR-039) proposals.
fn create(
    env: &Env,
    program_id: &BytesN<32>,
    mentor: &Address,
    mentee: &Address,
    terms: &ProposalTerms,
    options: CreationOptions,
) -> Result<BytesN<32>, MatchingError> {
    let zero_program = BytesN::from_array(env, &ZERO_HASH);
    if *program_id == zero_program {
        return Err(MatchingError::InvalidProgram);
    }
    if mentor == mentee {
        return Err(MatchingError::SelfMatch);
    }
    valid_terms(env, terms)?;
    require_eligible(env, mentor, mentee)?;

    // Duplicate active proposals are prevented per (program, mentor, mentee).
    let pair_key = DataKey::PairIndex(program_id.clone(), mentor.clone(), mentee.clone());
    if let Some(existing_id) = env
        .storage()
        .persistent()
        .get::<DataKey, BytesN<32>>(&pair_key)
    {
        let mut existing = load_proposal(env, &existing_id)?;
        if existing.status.is_open() {
            if env.ledger().timestamp() < existing.expires_at {
                return Err(MatchingError::DuplicateActiveProposal);
            }
            expire_proposal(env, &mut existing);
        }
    }

    let now = env.ledger().timestamp();
    let mut salt = Bytes::new(env);
    salt.append(&Bytes::from_slice(env, &program_id.clone().to_array()));
    salt.append(&mentor.clone().to_xdr(env));
    salt.append(&mentee.clone().to_xdr(env));
    let id = derive_id(env, salt)?;

    increase_pending(env, program_id)?;
    let proposal = Proposal {
        id: id.clone(),
        program_id: program_id.clone(),
        mentor: mentor.clone(),
        mentee: mentee.clone(),
        terms_hash: terms.terms_hash.clone(),
        proposed_fee: terms.proposed_fee,
        expires_at: terms.expires_at,
        created_by: options.created_by.clone(),
        origin: options.origin,
        last_adjusted_by: None,
        status: ProposalStatus::Proposed,
        version: 1,
        mentor_approved: false,
        mentee_approved: false,
        created_at: now,
        updated_at: now,
    };
    store_proposal(env, &proposal);
    env.storage().persistent().set(&pair_key, &id);
    bump_persistent(env, &pair_key);
    env.events().publish(
        (symbol_short!("MNT_PROP"),),
        (
            id.clone(),
            program_id.clone(),
            mentor.clone(),
            mentee.clone(),
            options.origin as u32,
            1u32,
        ),
    );

    if let Some(reason) = options.reason_hash {
        let reason_key = DataKey::ProposalReason(id.clone());
        env.storage().persistent().set(&reason_key, &reason);
        bump_persistent(env, &reason_key);
        record_coordinator_action(
            env,
            &options.created_by,
            &id,
            CoordinatorActionKind::Proposed,
            &reason,
            1,
        )?;
    }
    Ok(id)
}

#[contract]
pub struct MentorshipMatching;

#[contractimpl]
impl MentorshipMatching {
    /// One-time bootstrap of the matching admin. Coordinators are added with
    /// `set_coordinator`; participants are registered by the admin.
    pub fn initialize(env: Env, admin: Address) -> Result<(), MatchingError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(MatchingError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events().publish((symbol_short!("MNT_INIT"),), admin);
        Ok(())
    }

    /// Admin-only: upgrade to `new_wasm_hash`, preserving the address.
    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), MatchingError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("MNT_UPG"),), new_wasm_hash);
        Ok(())
    }

    /// Admin-only: grant or revoke coordinator authority.
    pub fn set_coordinator(
        env: Env,
        caller: Address,
        coordinator: Address,
        allowed: bool,
    ) -> Result<(), MatchingError> {
        require_admin(&env, &caller)?;
        let key = DataKey::Coordinator(coordinator.clone());
        if allowed {
            env.storage().persistent().set(&key, &true);
            bump_persistent(&env, &key);
        } else {
            env.storage().persistent().remove(&key);
        }
        env.events()
            .publish((symbol_short!("MNT_COORD"),), (coordinator, allowed));
        Ok(())
    }

    pub fn is_coordinator(env: Env, who: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Coordinator(who))
            .unwrap_or(false)
    }

    /// Admin-only: register a participant with a fixed role. Eligibility
    /// details (profiles, expertise, learning goals) stay off-chain; only the
    /// role/status gate lives here.
    pub fn register_participant(
        env: Env,
        caller: Address,
        participant: Address,
        role: ParticipantRole,
    ) -> Result<(), MatchingError> {
        require_admin(&env, &caller)?;
        let key = DataKey::Participant(participant.clone());
        if env.storage().persistent().has(&key) {
            return Err(MatchingError::ParticipantExists);
        }
        let profile = Participant {
            role,
            status: ParticipantStatus::Active,
            updated_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &profile);
        bump_persistent(&env, &key);
        env.events()
            .publish((symbol_short!("MNT_REG"),), (participant, role as u32));
        Ok(())
    }

    /// Admin-only: change hard eligibility. Deactivating a participant does
    /// not cancel existing proposals, but every subsequent approval or
    /// adjustment re-checks status and fails.
    pub fn set_participant_status(
        env: Env,
        caller: Address,
        participant: Address,
        status: ParticipantStatus,
    ) -> Result<(), MatchingError> {
        require_admin(&env, &caller)?;
        let mut profile = load_participant(&env, &participant)?;
        profile.status = status;
        profile.updated_at = env.ledger().timestamp();
        let key = DataKey::Participant(participant.clone());
        env.storage().persistent().set(&key, &profile);
        bump_persistent(&env, &key);
        env.events()
            .publish((symbol_short!("MNT_STAT"),), (participant, status as u32));
        Ok(())
    }

    pub fn get_participant(env: Env, participant: Address) -> Result<Participant, MatchingError> {
        load_participant(&env, &participant)
    }

    /// Participant-only: record or clear a block against another address.
    /// Blocks are enforced symmetrically in both proposal creation and
    /// approval.
    pub fn set_block(
        env: Env,
        participant: Address,
        counterparty: Address,
        blocked: bool,
    ) -> Result<(), MatchingError> {
        if participant == counterparty {
            return Err(MatchingError::SelfMatch);
        }
        participant.require_auth();
        let key = DataKey::Blocked(participant.clone(), counterparty.clone());
        if blocked {
            env.storage().persistent().set(&key, &true);
            bump_persistent(&env, &key);
        } else {
            env.storage().persistent().remove(&key);
        }
        env.events().publish(
            (symbol_short!("MNT_BLOCK"),),
            (participant, counterparty, blocked),
        );
        Ok(())
    }

    pub fn is_blocked(env: Env, a: Address, b: Address) -> bool {
        blocked(&env, &a, &b)
    }

    /// Mentor-initiated match proposal (MENTOR-037). The mentor signs for
    /// themselves; the mentee is linked by address and must approve. The
    /// compatibility reasons remain off-chain behind `terms_hash`.
    pub fn create_proposal(
        env: Env,
        mentor: Address,
        program_id: BytesN<32>,
        mentee: Address,
        terms: ProposalTerms,
    ) -> Result<BytesN<32>, MatchingError> {
        mentor.require_auth();
        create(
            &env,
            &program_id,
            &mentor,
            &mentee,
            &terms,
            CreationOptions {
                created_by: mentor.clone(),
                origin: ProposalOrigin::Mentor,
                reason_hash: None,
            },
        )
    }

    /// Coordinator-assisted match proposal (MENTOR-039). Requires
    /// coordinator authority and a non-zero reason commitment; the action is
    /// logged against the coordinator for reporting. Hard eligibility and
    /// blocks are still enforced by the shared creation path.
    pub fn propose_as_coordinator(
        env: Env,
        coordinator: Address,
        program_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
        terms: ProposalTerms,
        reason_hash: BytesN<32>,
    ) -> Result<BytesN<32>, MatchingError> {
        require_coordinator(&env, &coordinator)?;
        if reason_hash.to_array() == ZERO_HASH {
            return Err(MatchingError::ReasonRequired);
        }
        create(
            &env,
            &program_id,
            &mentor,
            &mentee,
            &terms,
            CreationOptions {
                created_by: coordinator,
                origin: ProposalOrigin::Coordinator,
                reason_hash: Some(reason_hash),
            },
        )
    }

    /// Either participant approves the proposal at `expected_version`. When
    /// both approvals are present the match becomes `Active`.
    ///
    /// `expected_version` makes stale approvals fail deterministically after
    /// a coordinator adjustment: participants always approve the exact
    /// version they were shown.
    pub fn approve_proposal(
        env: Env,
        participant: Address,
        proposal_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), MatchingError> {
        let mut proposal = load_proposal(&env, &proposal_id)?;
        let is_mentor = participant == proposal.mentor;
        let is_mentee = participant == proposal.mentee;
        if !is_mentor && !is_mentee {
            return Err(MatchingError::NotParticipant);
        }
        participant.require_auth();
        if proposal.version != expected_version {
            return Err(MatchingError::StaleProposalVersion);
        }
        if (is_mentor && proposal.mentor_approved) || (is_mentee && proposal.mentee_approved) {
            return Err(MatchingError::AlreadyApproved);
        }
        if proposal.status != ProposalStatus::Proposed {
            return Err(MatchingError::InvalidTransition);
        }
        if env.ledger().timestamp() >= proposal.expires_at {
            // Expiry cannot be persisted on a failing call (Soroban reverts
            // the invocation); the next successful creation for this pair
            // finalizes it.
            return Err(MatchingError::ProposalExpired);
        }
        // Re-check hard eligibility and blocks: a block or suspension added
        // after the proposal must block the approval.
        require_eligible(&env, &proposal.mentor, &proposal.mentee)?;

        if is_mentor {
            proposal.mentor_approved = true;
        } else {
            proposal.mentee_approved = true;
        }
        proposal.updated_at = env.ledger().timestamp();
        let both_approved = proposal.mentor_approved && proposal.mentee_approved;
        if both_approved {
            proposal.status = ProposalStatus::Active;
            decrease_pending(&env, &proposal.program_id);
        }
        store_proposal(&env, &proposal);
        env.events().publish(
            (symbol_short!("MNT_APPR"),),
            (proposal_id.clone(), participant.clone(), expected_version),
        );
        if both_approved {
            env.events().publish(
                (symbol_short!("MNT_MATCH"),),
                (
                    proposal_id,
                    proposal.program_id,
                    proposal.mentor,
                    proposal.mentee,
                ),
            );
        }
        Ok(())
    }

    /// Either participant declines a pending proposal. Repeating the same
    /// decline is a no-op, so retries never duplicate effects.
    pub fn decline_proposal(
        env: Env,
        participant: Address,
        proposal_id: BytesN<32>,
        reason_code: u32,
    ) -> Result<(), MatchingError> {
        if reason_code == 0 {
            return Err(MatchingError::InvalidReasonCode);
        }
        let mut proposal = load_proposal(&env, &proposal_id)?;
        if participant != proposal.mentor && participant != proposal.mentee {
            return Err(MatchingError::NotParticipant);
        }
        participant.require_auth();
        if proposal.status == ProposalStatus::Declined {
            return Ok(());
        }
        if proposal.status != ProposalStatus::Proposed {
            return Err(MatchingError::InvalidTransition);
        }
        proposal.status = ProposalStatus::Declined;
        proposal.updated_at = env.ledger().timestamp();
        decrease_pending(&env, &proposal.program_id);
        store_proposal(&env, &proposal);
        env.events().publish(
            (symbol_short!("MNT_DECL"),),
            (proposal_id, participant, reason_code),
        );
        Ok(())
    }

    /// Coordinator-only: adjust a proposal's terms or expiry (MENTOR-039).
    ///
    /// Adjusting resets both approvals and bumps the version, so participants
    /// must explicitly re-approve the changed proposal. Hard eligibility and
    /// blocks cannot be bypassed, and the action is logged with its reason
    /// commitment for reporting.
    pub fn adjust_proposal(
        env: Env,
        coordinator: Address,
        proposal_id: BytesN<32>,
        terms: ProposalTerms,
        reason_hash: BytesN<32>,
    ) -> Result<BytesN<32>, MatchingError> {
        require_coordinator(&env, &coordinator)?;
        if reason_hash.to_array() == ZERO_HASH {
            return Err(MatchingError::ReasonRequired);
        }
        valid_terms(&env, &terms)?;
        let mut proposal = load_proposal(&env, &proposal_id)?;
        if !proposal.status.is_open() {
            return Err(MatchingError::InvalidTransition);
        }
        if env.ledger().timestamp() >= proposal.expires_at {
            return Err(MatchingError::ProposalExpired);
        }
        require_eligible(&env, &proposal.mentor, &proposal.mentee)?;

        if proposal.status == ProposalStatus::Active {
            increase_pending(&env, &proposal.program_id)?;
        }
        proposal.terms_hash = terms.terms_hash;
        proposal.proposed_fee = terms.proposed_fee;
        proposal.expires_at = terms.expires_at;
        proposal.status = ProposalStatus::Proposed;
        proposal.version = proposal
            .version
            .checked_add(1)
            .ok_or(MatchingError::CounterOverflow)?;
        proposal.mentor_approved = false;
        proposal.mentee_approved = false;
        proposal.last_adjusted_by = Some(coordinator.clone());
        proposal.updated_at = env.ledger().timestamp();
        store_proposal(&env, &proposal);

        let reason_key = DataKey::ProposalReason(proposal_id.clone());
        env.storage().persistent().set(&reason_key, &reason_hash);
        bump_persistent(&env, &reason_key);
        record_coordinator_action(
            &env,
            &coordinator,
            &proposal_id,
            CoordinatorActionKind::Adjusted,
            &reason_hash,
            proposal.version,
        )?;
        env.events().publish(
            (symbol_short!("MNT_ADJ"),),
            (proposal_id.clone(), coordinator, proposal.version),
        );
        Ok(proposal_id)
    }

    pub fn get_proposal(env: Env, proposal_id: BytesN<32>) -> Result<Proposal, MatchingError> {
        load_proposal(&env, &proposal_id)
    }

    /// Whether a proposal is still awaiting or holding consent (it has not
    /// been declined and its expiry has not passed).
    pub fn is_proposal_open(env: Env, proposal_id: BytesN<32>) -> Result<bool, MatchingError> {
        let proposal = load_proposal(&env, &proposal_id)?;
        Ok(proposal.status.is_open() && env.ledger().timestamp() < proposal.expires_at)
    }

    /// The open proposal for a (program, mentor, mentee) triple, if any.
    pub fn open_proposal_for(
        env: Env,
        program_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
    ) -> Option<BytesN<32>> {
        let id: BytesN<32> = env
            .storage()
            .persistent()
            .get(&DataKey::PairIndex(program_id, mentor, mentee))?;
        let proposal = load_proposal(&env, &id).ok()?;
        if proposal.status.is_open() && env.ledger().timestamp() < proposal.expires_at {
            Some(id)
        } else {
            None
        }
    }

    /// Number of proposals awaiting approval in a program (storage bound).
    pub fn pending_proposal_count(env: Env, program_id: BytesN<32>) -> u32 {
        pending_count(&env, &program_id)
    }

    /// Reason commitment attached by the last coordinator action on a
    /// proposal, if any. The narrative itself stays off-chain.
    pub fn proposal_reason(env: Env, proposal_id: BytesN<32>) -> Result<BytesN<32>, MatchingError> {
        env.storage()
            .persistent()
            .get(&DataKey::ProposalReason(proposal_id))
            .ok_or(MatchingError::ReasonNotFound)
    }

    /// Number of coordinator actions recorded for reporting.
    pub fn coordinator_action_count(env: Env, coordinator: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::CoordActionCount(coordinator))
            .unwrap_or(0)
    }

    /// Read a coordinator action (1-indexed) for override reporting.
    pub fn coordinator_action(
        env: Env,
        coordinator: Address,
        seq: u32,
    ) -> Result<CoordinatorAction, MatchingError> {
        env.storage()
            .persistent()
            .get(&DataKey::CoordAction(coordinator, seq))
            .ok_or(MatchingError::ActionNotFound)
    }
}

#[cfg(test)]
mod tests;
