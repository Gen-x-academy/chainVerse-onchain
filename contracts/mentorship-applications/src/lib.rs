#![no_std]

//! Mentorship application decisions (MENTOR-035, issue #1231) and
//! withdrawal / reapplication (MENTOR-036, issue #1232).
//!
//! The contract is the on-chain authority for *how* a mentorship
//! application moves through review. It never stores free-form notes,
//! reviewer commentary, or applicant answers: only status transitions,
//! stable applicant-visible reason codes, `BytesN<32>` commitments to
//! internal reasoning, timestamps, and the communication policy an
//! off-chain notifier must honour.
//!
//! Design notes:
//!
//! * Decisions are append-only. Every accepted transition appends an
//!   immutable `DecisionRecord` under `(application, seq)`; nothing is
//!   ever overwritten or deleted, so the review history stays auditable
//!   even after a withdrawal or reapplication.
//! * The applicant-facing view (`applicant_visible_decision`) deliberately
//!   omits the internal-reason commitment and the deciding coordinator.
//!   The full record is coordinator-gated (`decision`).
//! * Replays are safe: repeating an identical decision or withdrawal
//!   returns the original sequence number without appending history or
//!   emitting a second event, and repeating a reapplication returns the
//!   already-created application id.
//! * Withdrawal requires an explicit `acknowledge_consequences` flag,
//!   releases the program's active-application reservation, and never
//!   removes the application or its history.
//! * A reapplication always creates a *new* application with its own
//!   consent version and affirmation timestamp; stale consent is never
//!   copied forward (`StaleConsent` otherwise).
//!
//! See `contracts/docs/mentorship-applications.md` for the privacy
//! boundary, dependencies, and rollout notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env,
};

/// Persistent entries are bumped to at least ~180 days and kept for at
/// most ~360 days, matching the rest of the contract suite.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Hard cap on the immutable decision history of a single application.
/// Beyond this the review must continue off-chain; the cap keeps the
/// bounded storage profile deterministic.
const MAX_DECISIONS_PER_APPLICATION: u32 = 32;

/// Hard per-program reservation bound, applied even when a program
/// policy leaves `max_active_applications` at zero (unlimited).
const MAX_ACTIVE_APPLICATIONS: u32 = 10_000;

const ZERO_HASH: [u8; 32] = [0u8; 32];

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ApplicationError {
    /// The contract has no admin yet (`initialize` has not run).
    NotInitialized = 1,
    /// `initialize` may only run once.
    AlreadyInitialized = 2,
    /// Deny-by-default: the caller is neither the admin nor a coordinator,
    /// or is not the applicant the action belongs to.
    Unauthorized = 3,
    /// No program policy has been configured for this program.
    PolicyNotFound = 4,
    /// No application exists under the supplied id.
    ApplicationNotFound = 5,
    /// `consent_version` must be a positive, currently published version.
    InvalidConsent = 6,
    /// Applicant-visible reason codes are stable, non-zero identifiers.
    InvalidReasonCode = 7,
    /// `effective_at` may not be in the past.
    InvalidEffectiveTime = 8,
    /// Decisions are closed for this program (`decision_cutoff` reached).
    DecisionWindowClosed = 9,
    /// The requested transition is not valid from the current status.
    InvalidTransition = 10,
    /// The immutable decision history reached `MAX_DECISIONS_PER_APPLICATION`.
    DecisionHistoryFull = 11,
    /// Withdrawals are closed for this program (`withdrawal_cutoff` reached).
    WithdrawalWindowClosed = 12,
    /// Withdrawal must confirm its consequences via `acknowledge_consequences`.
    WithdrawalNotAcknowledged = 13,
    /// A final decision already exists; the application can no longer be withdrawn.
    WithdrawalNotAllowed = 14,
    /// The program policy (or the previous application's status) forbids reapplying.
    ReapplicationNotAllowed = 15,
    /// The program's reapplication cooldown has not elapsed yet.
    ReapplicationTooSoon = 16,
    /// The program's attempt limit has been reached.
    ReapplicationLimitReached = 17,
    /// A reapplication must carry consent at least as new as the previous one.
    StaleConsent = 18,
    /// The program's active-application reservation bound is reached.
    CapacityExceeded = 19,
    /// A monotonic counter reached `u32::MAX`.
    CounterOverflow = 20,
    /// Rejections and waitlistings must commit to an internal reason hash.
    InternalReasonRequired = 21,
    /// The configured policy itself is inconsistent.
    InvalidPolicy = 22,
    /// Withdrawal is the applicant's action; coordinators cannot record it.
    WithdrawalIsApplicantAction = 23,
}

/// Lifecycle status of a mentorship application.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationStatus {
    Submitted,
    UnderReview,
    InfoRequested,
    Waitlisted,
    Accepted,
    Rejected,
    Withdrawn,
}

impl ApplicationStatus {
    /// Stable numeric code for off-chain consumers and event payloads.
    pub fn code(self) -> u32 {
        match self {
            ApplicationStatus::Submitted => 1,
            ApplicationStatus::UnderReview => 2,
            ApplicationStatus::InfoRequested => 3,
            ApplicationStatus::Waitlisted => 4,
            ApplicationStatus::Accepted => 5,
            ApplicationStatus::Rejected => 6,
            ApplicationStatus::Withdrawn => 7,
        }
    }

    /// A status that no longer accepts decisions or withdrawals.
    fn is_final(self) -> bool {
        matches!(
            self,
            ApplicationStatus::Accepted
                | ApplicationStatus::Rejected
                | ApplicationStatus::Withdrawn
        )
    }
}

/// The structured action recorded in an application's history (MENTOR-035).
///
/// `Withdraw` is part of the history vocabulary so the applicant's own
/// action is preserved in the same immutable timeline, but coordinators
/// cannot record it through `record_decision`: withdrawal only happens
/// through `withdraw_application`, which enforces the withdrawal window and
/// consequence acknowledgement.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionAction {
    Review,
    RequestInfo,
    Waitlist,
    Accept,
    Reject,
    Withdraw,
}

impl DecisionAction {
    pub fn code(self) -> u32 {
        match self {
            DecisionAction::Review => 1,
            DecisionAction::RequestInfo => 2,
            DecisionAction::Waitlist => 3,
            DecisionAction::Accept => 4,
            DecisionAction::Reject => 5,
            DecisionAction::Withdraw => 6,
        }
    }

    fn target_status(self) -> ApplicationStatus {
        match self {
            DecisionAction::Review => ApplicationStatus::UnderReview,
            DecisionAction::RequestInfo => ApplicationStatus::InfoRequested,
            DecisionAction::Waitlist => ApplicationStatus::Waitlisted,
            DecisionAction::Accept => ApplicationStatus::Accepted,
            DecisionAction::Reject => ApplicationStatus::Rejected,
            DecisionAction::Withdraw => ApplicationStatus::Withdrawn,
        }
    }

    /// Reject/waitlist decisions must commit to an internal reasoning hash
    /// so the applicant-visible reason code never has to carry it.
    fn requires_internal_reason(self) -> bool {
        matches!(self, DecisionAction::Waitlist | DecisionAction::Reject)
    }

    fn is_withdrawal(self) -> bool {
        self == DecisionAction::Withdraw
    }
}

/// How (and whether) the applicant may be told about a decision. The
/// contract cannot send messages; it records the policy so the off-chain
/// notifier can apply it consistently and auditable.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommunicationPolicy {
    /// Notify the applicant as soon as the decision is recorded.
    NotifyImmediately,
    /// Notify at `effective_at` rather than at recording time.
    NotifyAtEffectiveTime,
    /// Do not notify the applicant; coordinator-internal only.
    InternalOnly,
}

/// Program-level review policy. All windows use the same convention as the
/// rest of the suite: a cutoff is exclusive, i.e. the action is closed at
/// and after `cutoff`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramPolicy {
    /// Last timestamp at which a decision may be recorded; 0 = no cutoff.
    pub decision_cutoff: u64,
    /// Last timestamp at which an applicant may withdraw; 0 = no cutoff.
    pub withdrawal_cutoff: u64,
    /// Whether a withdrawn/rejected applicant may reapply.
    pub allow_reapplication: bool,
    /// Minimum seconds between an application closing and a reapplication.
    pub reapply_cooldown: u64,
    /// Maximum attempts per applicant per program; 0 = unlimited.
    pub max_attempts: u32,
    /// Maximum simultaneously active applications for the program; 0 = the
    /// contract-wide hard bound (`MAX_ACTIVE_APPLICATIONS`).
    pub max_active_applications: u32,
}

/// A mentorship application record. `previous_application` links a
/// reapplication to the attempt it replaces without copying any of its
/// state (notably, without copying its consent).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub applicant: Address,
    /// 1 for the first application, incremented on every reapplication.
    pub attempt: u32,
    pub status: ApplicationStatus,
    /// Off-chain consent/terms version the applicant accepted for *this*
    /// attempt.
    pub consent_version: u32,
    /// Ledger time at which this attempt's consent was affirmed.
    pub consent_affirmed_at: u64,
    /// The attempt this application replaces, if it is a reapplication.
    pub previous_application: Option<BytesN<32>>,
    pub submitted_at: u64,
    /// Ledger time at which the application reached a final status; 0 while open.
    pub closed_at: u64,
    /// Number of immutable decision records, also the latest `seq`.
    pub decision_count: u32,
    /// The latest decision's `seq`; 0 when none has been recorded.
    pub last_decision_seq: u32,
}

/// Input for `record_decision` (MENTOR-035).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionInput {
    /// Stable applicant-visible reason code; must be non-zero.
    pub public_reason: u32,
    /// Commitment to the internal reasoning/notes, kept off-chain. Must be
    /// non-zero for `Waitlist` and `Reject`.
    pub internal_reason_hash: BytesN<32>,
    /// When the decision becomes effective for communication purposes; 0
    /// means "now". May not be backdated.
    pub effective_at: u64,
    pub communication: CommunicationPolicy,
}

/// Input for `withdraw_application` (MENTOR-036).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalInput {
    /// Applicant-visible reason code; must be non-zero.
    pub public_reason: u32,
    /// See `DecisionInput::effective_at`.
    pub effective_at: u64,
    pub communication: CommunicationPolicy,
    /// Withdrawal explicitly confirms its consequences (reservation
    /// release, record retention, and blocked further review).
    pub acknowledge_consequences: bool,
}

/// An immutable decision-history entry. `Withdraw` actions are recorded
/// here too (with `action = None`) so the applicant's own action is part of
/// the same immutable history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionRecord {
    pub application_id: BytesN<32>,
    pub seq: u32,
    pub actor: Address,
    pub action: DecisionAction,
    pub resulting_status: ApplicationStatus,
    /// Applicant-visible reason code.
    pub public_reason: u32,
    /// Internal-reason commitment; never returned to applicants.
    pub internal_reason_hash: BytesN<32>,
    pub effective_at: u64,
    pub communication: CommunicationPolicy,
    pub recorded_at: u64,
}

/// Applicant-facing projection of a decision: public reason code, status,
/// effective time and communication policy only.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicantDecisionView {
    pub application_id: BytesN<32>,
    pub seq: u32,
    pub action: DecisionAction,
    pub resulting_status: ApplicationStatus,
    pub public_reason: u32,
    pub effective_at: u64,
    pub communication: CommunicationPolicy,
    pub recorded_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Coordinator(Address),
    Policy(BytesN<32>),
    Application(BytesN<32>),
    ApplicationCount,
    Decision(BytesN<32>, u32),
    ActiveCount(BytesN<32>),
    /// Previous application id -> reapplication id, for replay-safe reapply.
    ReappliedFrom(BytesN<32>),
}

fn admin(env: &Env) -> Result<Address, ApplicationError> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(ApplicationError::NotInitialized)
}

/// Admin-gated authorization, mirroring the `require_admin` pattern used by
/// the rest of the suite.
fn require_admin(env: &Env, caller: &Address) -> Result<(), ApplicationError> {
    let admin = admin(env)?;
    if *caller != admin {
        return Err(ApplicationError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// Admin or a registered coordinator may record decisions and read internal
/// reasoning. Deny-by-default: everything else is `Unauthorized`.
fn require_coordinator(env: &Env, caller: &Address) -> Result<(), ApplicationError> {
    let admin = admin(env)?;
    let registered: bool = env
        .storage()
        .persistent()
        .get(&DataKey::Coordinator(caller.clone()))
        .unwrap_or(false);
    if *caller != admin && !registered {
        return Err(ApplicationError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

fn bump_persistent(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

/// Collision-resistant id derivation: monotonic nonce mixed with ledger time
/// and caller-supplied salt, so two applications born in one ledger differ.
fn derive_id(env: &Env, salt: Bytes) -> Result<BytesN<32>, ApplicationError> {
    let nonce: u64 = env
        .storage()
        .instance()
        .get(&DataKey::ApplicationCount)
        .unwrap_or(0);
    let next = nonce
        .checked_add(1)
        .ok_or(ApplicationError::CounterOverflow)?;
    env.storage()
        .instance()
        .set(&DataKey::ApplicationCount, &next);
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    input.append(&Bytes::from_slice(
        env,
        &env.ledger().timestamp().to_be_bytes(),
    ));
    input.append(&salt);
    Ok(env.crypto().sha256(&input).into())
}

fn policy(env: &Env, program_id: &BytesN<32>) -> Result<ProgramPolicy, ApplicationError> {
    env.storage()
        .persistent()
        .get(&DataKey::Policy(program_id.clone()))
        .ok_or(ApplicationError::PolicyNotFound)
}

fn load_application(
    env: &Env,
    application_id: &BytesN<32>,
) -> Result<Application, ApplicationError> {
    env.storage()
        .persistent()
        .get(&DataKey::Application(application_id.clone()))
        .ok_or(ApplicationError::ApplicationNotFound)
}

fn store_application(env: &Env, application: &Application) {
    let key = DataKey::Application(application.id.clone());
    env.storage().persistent().set(&key, application);
    bump_persistent(env, &key);
}

fn active_count(env: &Env, program_id: &BytesN<32>) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::ActiveCount(program_id.clone()))
        .unwrap_or(0)
}

fn set_active_count(env: &Env, program_id: &BytesN<32>, count: u32) {
    let key = DataKey::ActiveCount(program_id.clone());
    env.storage().persistent().set(&key, &count);
    bump_persistent(env, &key);
}

fn reserve_slot(env: &Env, program_id: &BytesN<32>, limit: u32) -> Result<(), ApplicationError> {
    let current = active_count(env, program_id);
    if current >= limit {
        return Err(ApplicationError::CapacityExceeded);
    }
    set_active_count(env, program_id, current + 1);
    Ok(())
}

/// Release a reservation without underflowing on an already-released slot.
fn release_slot(env: &Env, program_id: &BytesN<32>) {
    let current = active_count(env, program_id);
    if current > 0 {
        set_active_count(env, program_id, current - 1);
    }
}

fn effective_limit(policy: &ProgramPolicy) -> u32 {
    if policy.max_active_applications == 0 {
        MAX_ACTIVE_APPLICATIONS
    } else if policy.max_active_applications < MAX_ACTIVE_APPLICATIONS {
        policy.max_active_applications
    } else {
        MAX_ACTIVE_APPLICATIONS
    }
}

/// Validate a reapplication's consent against the attempt it replaces:
/// positive, and never older than the consent it would replace.
fn validate_reapplication_consent(
    previous: &Application,
    consent_version: u32,
) -> Result<(), ApplicationError> {
    if consent_version == 0 {
        return Err(ApplicationError::InvalidConsent);
    }
    if consent_version < previous.consent_version {
        return Err(ApplicationError::StaleConsent);
    }
    Ok(())
}

/// Shared creation path for a first application attempt.
fn create_application(
    env: &Env,
    applicant: Address,
    program_id: BytesN<32>,
    attempt: u32,
    consent_version: u32,
    previous_application: Option<BytesN<32>>,
) -> Result<BytesN<32>, ApplicationError> {
    let now = env.ledger().timestamp();
    let mut salt = Bytes::new(env);
    salt.append(&Bytes::from_slice(env, &program_id.clone().to_array()));
    salt.append(&applicant.clone().to_xdr(env));
    salt.append(&Bytes::from_slice(env, &attempt.to_be_bytes()));
    let id = derive_id(env, salt)?;
    let application = Application {
        id: id.clone(),
        program_id,
        applicant: applicant.clone(),
        attempt,
        status: ApplicationStatus::Submitted,
        consent_version,
        consent_affirmed_at: now,
        previous_application,
        submitted_at: now,
        closed_at: 0,
        decision_count: 0,
        last_decision_seq: 0,
    };
    store_application(env, &application);
    Ok(id)
}

#[contract]
pub struct MentorshipApplications;

#[contractimpl]
impl MentorshipApplications {
    /// One-time bootstrap: sets the review admin. Coordinators are added
    /// separately with `set_coordinator`.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ApplicationError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ApplicationError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events().publish((symbol_short!("MNT_INIT"),), admin);
        Ok(())
    }

    /// Admin-only: upgrade to `new_wasm_hash` (same address preserved).
    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ApplicationError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("MNT_UPG"),), new_wasm_hash);
        Ok(())
    }

    /// Admin-only: grant or revoke decision authority for a coordinator.
    pub fn set_coordinator(
        env: Env,
        caller: Address,
        coordinator: Address,
        allowed: bool,
    ) -> Result<(), ApplicationError> {
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

    /// Admin-only: configure the review policy for a program. Replacing a
    /// policy affects *future* actions only; recorded decisions are immutable.
    pub fn set_program_policy(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        policy: ProgramPolicy,
    ) -> Result<(), ApplicationError> {
        require_admin(&env, &caller)?;
        if policy.max_attempts > 0 && !policy.allow_reapplication {
            // A positive attempt cap without reapplication enabled is
            // contradictory; reject it instead of silently ignoring one field.
            return Err(ApplicationError::InvalidPolicy);
        }
        let key = DataKey::Policy(program_id.clone());
        env.storage().persistent().set(&key, &policy);
        bump_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("MNT_POL"),),
            (
                program_id,
                policy.decision_cutoff,
                policy.withdrawal_cutoff,
                policy.allow_reapplication,
            ),
        );
        Ok(())
    }

    pub fn get_program_policy(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramPolicy, ApplicationError> {
        policy(&env, &program_id)
    }

    /// Open a first-attempt application. This is the minimal intake
    /// primitive the decision/withdrawal flows need; richer intake (drafts,
    /// configurable questions, eligibility preflight) is tracked by
    /// MENTOR-030..034.
    ///
    /// The applicant must sign for themselves — no coordinator can submit on
    /// their behalf — and must reference a positive consent/terms version.
    pub fn open_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        consent_version: u32,
    ) -> Result<BytesN<32>, ApplicationError> {
        applicant.require_auth();
        if consent_version == 0 {
            return Err(ApplicationError::InvalidConsent);
        }
        let policy = policy(&env, &program_id)?;
        reserve_slot(&env, &program_id, effective_limit(&policy))?;
        let id = create_application(
            &env,
            applicant.clone(),
            program_id.clone(),
            1,
            consent_version,
            None,
        )?;
        env.events().publish(
            (symbol_short!("MNT_OPEN"),),
            (id.clone(), program_id, applicant, 1u32),
        );
        Ok(id)
    }

    /// Record a structured decision (MENTOR-035). Admin or a registered
    /// coordinator only.
    ///
    /// Valid source states are everything except the final ones
    /// (`Accepted`, `Rejected`, `Withdrawn`). Repeating an identical
    /// decision — same action, same public reason, same internal-reason
    /// commitment — is a no-op that returns the original `seq`, so retries
    /// never duplicate history.
    pub fn record_decision(
        env: Env,
        coordinator: Address,
        application_id: BytesN<32>,
        action: DecisionAction,
        input: DecisionInput,
    ) -> Result<u32, ApplicationError> {
        require_coordinator(&env, &coordinator)?;
        if action.is_withdrawal() {
            return Err(ApplicationError::WithdrawalIsApplicantAction);
        }
        if input.public_reason == 0 {
            return Err(ApplicationError::InvalidReasonCode);
        }
        if action.requires_internal_reason() && input.internal_reason_hash.to_array() == ZERO_HASH {
            return Err(ApplicationError::InternalReasonRequired);
        }
        let now = env.ledger().timestamp();
        let effective_at = if input.effective_at == 0 {
            now
        } else {
            if input.effective_at < now {
                return Err(ApplicationError::InvalidEffectiveTime);
            }
            input.effective_at
        };
        let mut application = load_application(&env, &application_id)?;
        let target = action.target_status();

        // Replay protection: an identical retry returns the existing entry,
        // including when the first attempt already reached a final status.
        if application.last_decision_seq > 0 {
            let last: DecisionRecord = env
                .storage()
                .persistent()
                .get(&DataKey::Decision(
                    application_id.clone(),
                    application.last_decision_seq,
                ))
                .ok_or(ApplicationError::ApplicationNotFound)?;
            if last.action == action
                && last.public_reason == input.public_reason
                && last.internal_reason_hash == input.internal_reason_hash
                && application.status == target
            {
                return Ok(last.seq);
            }
        }

        if application.status.is_final() {
            return Err(ApplicationError::InvalidTransition);
        }
        let policy = policy(&env, &application.program_id)?;
        if policy.decision_cutoff != 0 && now >= policy.decision_cutoff {
            return Err(ApplicationError::DecisionWindowClosed);
        }

        if application.decision_count >= MAX_DECISIONS_PER_APPLICATION {
            return Err(ApplicationError::DecisionHistoryFull);
        }
        let seq = application
            .decision_count
            .checked_add(1)
            .ok_or(ApplicationError::CounterOverflow)?;
        let record = DecisionRecord {
            application_id: application_id.clone(),
            seq,
            actor: coordinator.clone(),
            action,
            resulting_status: target,
            public_reason: input.public_reason,
            internal_reason_hash: input.internal_reason_hash,
            effective_at,
            communication: input.communication,
            recorded_at: now,
        };
        let key = DataKey::Decision(application_id.clone(), seq);
        env.storage().persistent().set(&key, &record);
        bump_persistent(&env, &key);

        application.status = target;
        application.decision_count = seq;
        application.last_decision_seq = seq;
        if target.is_final() {
            application.closed_at = now;
            release_slot(&env, &application.program_id);
        }
        store_application(&env, &application);

        env.events().publish(
            (symbol_short!("MNT_DEC"),),
            (
                application_id,
                seq,
                action.code(),
                input.public_reason,
                effective_at,
            ),
        );
        Ok(seq)
    }

    /// Withdraw an application before the program cutoff (MENTOR-036).
    ///
    /// Withdrawal is applicant-only, requires `acknowledge_consequences`,
    /// releases the program reservation, blocks any further review action,
    /// and leaves the application and its decision history intact. The
    /// applicant's action is appended to the same immutable history.
    pub fn withdraw_application(
        env: Env,
        applicant: Address,
        application_id: BytesN<32>,
        input: WithdrawalInput,
    ) -> Result<u32, ApplicationError> {
        applicant.require_auth();
        let mut application = load_application(&env, &application_id)?;
        if application.applicant != applicant {
            return Err(ApplicationError::Unauthorized);
        }
        if input.public_reason == 0 {
            return Err(ApplicationError::InvalidReasonCode);
        }
        if !input.acknowledge_consequences {
            return Err(ApplicationError::WithdrawalNotAcknowledged);
        }
        let now = env.ledger().timestamp();
        let effective_at = if input.effective_at == 0 {
            now
        } else {
            if input.effective_at < now {
                return Err(ApplicationError::InvalidEffectiveTime);
            }
            input.effective_at
        };

        // Replay protection for the applicant's own retried withdrawal.
        if application.status == ApplicationStatus::Withdrawn {
            let last: DecisionRecord = env
                .storage()
                .persistent()
                .get(&DataKey::Decision(
                    application_id.clone(),
                    application.last_decision_seq,
                ))
                .ok_or(ApplicationError::ApplicationNotFound)?;
            if last.action.is_withdrawal() && last.public_reason == input.public_reason {
                return Ok(last.seq);
            }
            return Err(ApplicationError::InvalidTransition);
        }
        if application.status.is_final() {
            return Err(ApplicationError::WithdrawalNotAllowed);
        }
        let policy = policy(&env, &application.program_id)?;
        if policy.withdrawal_cutoff != 0 && now >= policy.withdrawal_cutoff {
            return Err(ApplicationError::WithdrawalWindowClosed);
        }
        if application.decision_count >= MAX_DECISIONS_PER_APPLICATION {
            return Err(ApplicationError::DecisionHistoryFull);
        }
        let seq = application
            .decision_count
            .checked_add(1)
            .ok_or(ApplicationError::CounterOverflow)?;
        let record = DecisionRecord {
            application_id: application_id.clone(),
            seq,
            actor: applicant.clone(),
            action: DecisionAction::Withdraw,
            resulting_status: ApplicationStatus::Withdrawn,
            public_reason: input.public_reason,
            internal_reason_hash: BytesN::from_array(&env, &ZERO_HASH),
            effective_at,
            communication: input.communication,
            recorded_at: now,
        };
        let key = DataKey::Decision(application_id.clone(), seq);
        env.storage().persistent().set(&key, &record);
        bump_persistent(&env, &key);

        application.status = ApplicationStatus::Withdrawn;
        application.decision_count = seq;
        application.last_decision_seq = seq;
        application.closed_at = now;
        store_application(&env, &application);
        release_slot(&env, &application.program_id);

        env.events().publish(
            (symbol_short!("MNT_WDRAW"),),
            (application_id, applicant, input.public_reason, effective_at),
        );
        Ok(seq)
    }

    /// Reapply to the same program (MENTOR-036).
    ///
    /// Only a `Withdrawn` or `Rejected` attempt can be reapplied to, only
    /// when the current policy allows it, after the cooldown, and below the
    /// attempt cap. The new application links back to `previous_application`
    /// and records its own consent version + affirmation timestamp — the
    /// previous attempt's consent is never reused. Repeating the call for
    /// the same previous application returns the existing id.
    pub fn reapply(
        env: Env,
        applicant: Address,
        previous_application_id: BytesN<32>,
        consent_version: u32,
    ) -> Result<BytesN<32>, ApplicationError> {
        applicant.require_auth();
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<DataKey, BytesN<32>>(&DataKey::ReappliedFrom(previous_application_id.clone()))
        {
            return Ok(existing);
        }
        let previous = load_application(&env, &previous_application_id)?;
        if previous.applicant != applicant {
            return Err(ApplicationError::Unauthorized);
        }
        if !matches!(
            previous.status,
            ApplicationStatus::Withdrawn | ApplicationStatus::Rejected
        ) {
            return Err(ApplicationError::ReapplicationNotAllowed);
        }
        let policy = policy(&env, &previous.program_id)?;
        if !policy.allow_reapplication {
            return Err(ApplicationError::ReapplicationNotAllowed);
        }
        validate_reapplication_consent(&previous, consent_version)?;
        let attempt = previous
            .attempt
            .checked_add(1)
            .ok_or(ApplicationError::CounterOverflow)?;
        if policy.max_attempts != 0 && attempt > policy.max_attempts {
            return Err(ApplicationError::ReapplicationLimitReached);
        }
        let now = env.ledger().timestamp();
        if policy.reapply_cooldown > 0 {
            let eligible_at = previous
                .closed_at
                .checked_add(policy.reapply_cooldown)
                .ok_or(ApplicationError::CounterOverflow)?;
            if now < eligible_at {
                return Err(ApplicationError::ReapplicationTooSoon);
            }
        }
        reserve_slot(&env, &previous.program_id, effective_limit(&policy))?;
        let id = create_application(
            &env,
            applicant.clone(),
            previous.program_id.clone(),
            attempt,
            consent_version,
            Some(previous_application_id.clone()),
        )?;
        let link_key = DataKey::ReappliedFrom(previous_application_id.clone());
        env.storage().persistent().set(&link_key, &id);
        bump_persistent(&env, &link_key);
        env.events().publish(
            (symbol_short!("MNT_REAP"),),
            (id.clone(), previous_application_id, applicant, attempt),
        );
        Ok(id)
    }

    /// Full application record, including its decision history pointers.
    pub fn get_application(
        env: Env,
        application_id: BytesN<32>,
    ) -> Result<Application, ApplicationError> {
        load_application(&env, &application_id)
    }

    /// Number of immutable decision records for an application.
    pub fn decision_count(env: Env, application_id: BytesN<32>) -> Result<u32, ApplicationError> {
        Ok(load_application(&env, &application_id)?.decision_count)
    }

    /// Applicant-visible decision: public reason code and lifecycle data
    /// only. The internal-reason commitment and the deciding coordinator are
    /// never exposed here.
    pub fn applicant_visible_decision(
        env: Env,
        application_id: BytesN<32>,
        seq: u32,
    ) -> Result<ApplicantDecisionView, ApplicationError> {
        let record: DecisionRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Decision(application_id.clone(), seq))
            .ok_or(ApplicationError::ApplicationNotFound)?;
        Ok(ApplicantDecisionView {
            application_id: record.application_id,
            seq: record.seq,
            action: record.action,
            resulting_status: record.resulting_status,
            public_reason: record.public_reason,
            effective_at: record.effective_at,
            communication: record.communication,
            recorded_at: record.recorded_at,
        })
    }

    /// Coordinator-gated internal record, including the internal-reason
    /// commitment and the actor. Applicants must use
    /// `applicant_visible_decision`.
    pub fn decision(
        env: Env,
        caller: Address,
        application_id: BytesN<32>,
        seq: u32,
    ) -> Result<DecisionRecord, ApplicationError> {
        require_coordinator(&env, &caller)?;
        env.storage()
            .persistent()
            .get(&DataKey::Decision(application_id, seq))
            .ok_or(ApplicationError::ApplicationNotFound)
    }

    /// Number of active (non-final) applications reserving a program slot.
    pub fn active_application_count(env: Env, program_id: BytesN<32>) -> u32 {
        active_count(&env, &program_id)
    }
}

#[cfg(test)]
mod tests;
