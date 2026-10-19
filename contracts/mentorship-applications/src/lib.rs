#![no_std]

//! Mentorship application intake, autosaved drafts, configurable application
//! forms, and scoped reviewer queues.
//!
//! Scope of this pass (issues #1226/MENTOR-030, #1227/MENTOR-031,
//! #1228/MENTOR-032, #1230/MENTOR-034):
//!
//! - **#1226/MENTOR-030 — Create mentorship applications.** Mentees save a
//!   draft and submit it in a separate, atomic call carrying goals,
//!   expectations, schedule, preferred mode and program answers as an
//!   integrity commitment. Submission validates the accepted, published form
//!   version and its required answers, requires current affirmative consent,
//!   and stores exactly one active application per `(applicant, program)`, so
//!   a retried or duplicate submission is rejected before any state changes.
//! - **#1227/MENTOR-031 — Autosave application drafts.** `autosave_draft`
//!   persists resumable drafts merged field-by-field with per-field
//!   timestamps, monotonic revision tokens and explicit last-saved feedback.
//!   A retried save is idempotent via its `request_token`; a save based on a
//!   stale `base_revision` fails with `DraftConflict` instead of silently
//!   overwriting a concurrent edit; and a submitted application can no longer
//!   be mutated as a draft because the draft is removed and further saves are
//!   rejected.
//! - **#1228/MENTOR-032 — Configurable application questions.** Program
//!   owners publish typed, ordered, required/optional questions with help-text
//!   commitments and deterministic conditional visibility. Published versions
//!   are immutable (publishing always creates version N+1), only integrity
//!   commitments — never labels, help text, or options — enter storage (so
//!   unsafe markup cannot be committed), and every application records the
//!   form version it was accepted against so historical responses stay
//!   renderable.
//! - **#1230/MENTOR-034 — Application review queues.** Admins grant reviewers
//!   a program-scoped role and capacity; reviewers work a filterable queue
//!   with priority, deterministic stable ordering, concurrency-safe
//!   assignment, due dates, conflict-of-interest declarations that block
//!   decisions, and workload indicators. Every queue action is appended to a
//!   bounded audit trail and emitted as an event.
//!
//! Privacy is minimized by construction: this contract never stores form
//! labels, help text, options, goals, expectations, schedules, preferred
//! modes, free-text answers, or reviewer notes. Those all live off-chain and
//! are represented on-chain only by caller-supplied `BytesN<32>` integrity
//! commitments, addresses, enums, counters, and timestamps. See
//! `contracts/docs/mentorship-applications.md` and `docs/mentorship/` for the
//! off-chain boundary, rollout, and cross-repository MENTOR work.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Vec};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Storage bound: at most this many questions in a published form version.
const MAX_QUESTIONS: u32 = 32;
/// Storage bound: at most this many answers on a draft or a submission.
const MAX_ANSWERS: u32 = 32;
/// Storage bound: at most this many applications tracked in one program queue.
const MAX_QUEUE: u32 = 256;
/// Storage bound: at most this many scoped reviewers per program.
const MAX_REVIEWERS: u32 = 32;
/// Storage bound: at most this many audit entries per application.
const MAX_AUDIT_ENTRIES: u32 = 64;
/// Storage bound: the largest page `list_queue` will return.
const MAX_QUEUE_PAGE: u32 = 64;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The caller is neither the admin nor an active, program-scoped reviewer.
    NotAuthorized = 4,
    ProgramNotFound = 5,
    ProgramAlreadyExists = 6,
    ProgramInactive = 7,
    /// The program's intake deadline has passed (drafts and submissions).
    DeadlinePassed = 8,
    /// A counter would overflow `u32`/`u64`; checked rather than wrapping.
    VersionOverflow = 9,

    // ── #1228/MENTOR-032 — configurable forms ────────────────────────────
    /// No form version has ever been published for this program.
    NoFormPublished = 10,
    FormVersionNotFound = 11,
    /// A form version is empty, has duplicate ids/orders, references a later
    /// question, uses a zero commitment, or has an options/type mismatch.
    InvalidForm = 12,

    // ── #1227/MENTOR-031 — drafts ────────────────────────────────────────
    DraftNotFound = 13,
    /// `base_revision` is not the stored revision — a concurrent edit won.
    DraftConflict = 14,
    TooManyAnswers = 15,
    DuplicateAnswer = 16,
    /// An answer references a question that is not part of the form version.
    AnswerNotInForm = 17,
    /// An answer was supplied for a question its visibility condition hides.
    AnswerNotVisible = 18,
    /// A visible, required question has no answer.
    MissingRequiredAnswer = 19,

    // ── #1226/MENTOR-030 — submissions ───────────────────────────────────
    /// An application already exists for this `(applicant, program)` pair.
    DuplicateApplication = 20,
    ApplicationNotFound = 21,

    // ── consent (required before submission) ─────────────────────────────
    NoConsentTermsPublished = 22,
    ConsentTermsNotFound = 23,
    ConsentNotFound = 24,
    ConsentRevoked = 25,
    ConsentOutOfDate = 26,
    /// The caller did not affirmatively accept the consent terms.
    ConsentRequired = 27,

    // ── #1230/MENTOR-034 — review queues ─────────────────────────────────
    AlreadyReviewer = 28,
    NotReviewer = 29,
    /// The reviewer pool for this program is full (`MAX_REVIEWERS`).
    ReviewerPoolFull = 30,
    /// The reviewer is already at their configured active-assignment capacity.
    ReviewerAtCapacity = 31,
    /// The application is no longer assigned to `expected_assignee`.
    AssignmentConflict = 32,
    ApplicationAlreadyDecided = 33,
    /// The reviewer declared a conflict of interest for this application.
    ConflictOfInterest = 34,
    /// The application is not assigned to this reviewer.
    NotAssignedReviewer = 35,
    QueueFull = 36,
    AuditTrailFull = 37,
    InvalidFilter = 38,
    /// A reviewer capacity of zero would make every assignment fail.
    InvalidCapacity = 39,
    /// `data_hash` must be a real integrity commitment, not all zeroes.
    InvalidDataHash = 40,
    /// An answer commitment must be a real hash, not all zeroes.
    InvalidAnswerHash = 41,
    /// A consent-terms commitment must be a real hash, not all zeroes.
    InvalidConsentTerms = 42,
    /// `Undecided` is not a valid `record_decision` input.
    InvalidDecision = 43,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Monotonic application id allocator (starts at 1).
    NextApplicationId,
    Program(BytesN<32>),
    /// Latest published form version for a program.
    FormVersion(BytesN<32>),
    /// An immutable, published form version.
    Form(BytesN<32>, u32),
    /// Latest published consent-terms version for a program.
    ConsentVersion(BytesN<32>),
    /// An immutable, published consent-terms version.
    ConsentTerms(BytesN<32>, u32),
    Consent(Address, BytesN<32>),
    /// A resumable draft, keyed by `(applicant, program)`.
    Draft(Address, BytesN<32>),
    /// The uniqueness constraint for one active application per pair.
    ApplicationByApplicant(Address, BytesN<32>),
    Application(u64),
    /// Stable, bounded application-id queue for a program.
    Queue(BytesN<32>),
    Reviewer(Address, BytesN<32>),
    ReviewerList(BytesN<32>),
    Review(u64),
    /// A reviewer's conflict-of-interest declaration for one application.
    Conflict(Address, u64),
    Audit(u64),
}

/// A mentorship program and its intake window.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub active: bool,
    /// Ledger timestamp (seconds) after which drafts/submissions are rejected.
    pub deadline: u64,
}

/// #1228/MENTOR-032 — the supported answer types. Typed questions let
/// off-chain renderers and validators share one canonical shape; the answer
/// content itself never leaves the client.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestionType {
    ShortText,
    LongText,
    Email,
    Number,
    Choice,
    MultiChoice,
    Date,
}

/// #1228/MENTOR-032 — deterministic conditional visibility. A question is
/// visible only when the referenced (earlier) question is answered and, when
/// `required_answer_hash` is set, that answer's commitment matches exactly.
/// Value-dependent branching therefore stays deterministic and privacy
/// preserving: only the commitment is compared, never the answer text.
/// `depends_on == 0` means "always visible" (question ids start at 1).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibilityCondition {
    pub depends_on: u32,
    pub required_answer_hash: Option<BytesN<32>>,
}

/// #1228/MENTOR-032 — one typed question. `label_hash`, `help_hash` and
/// `options_hash` are integrity commitments over off-chain content: the
/// contract accepts no strings at all, so markup (unsafe or otherwise) can
/// never enter on-chain state. `order` must strictly increase within a
/// published version, making rendering order deterministic.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Question {
    pub id: u32,
    pub kind: QuestionType,
    pub required: bool,
    pub order: u32,
    pub label_hash: BytesN<32>,
    pub help_hash: BytesN<32>,
    pub options_hash: Option<BytesN<32>>,
    pub visible_if: VisibilityCondition,
}

/// #1228/MENTOR-032 — an immutable, published form version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipForm {
    pub program_id: BytesN<32>,
    pub version: u32,
    pub schema_hash: BytesN<32>,
    pub questions: Vec<Question>,
    pub published_at: u64,
}

/// #1227/MENTOR-031 — an answer patch inside an autosave. `None` clears the
/// field (e.g. the applicant unchecked a conditional question).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnswerPatch {
    pub question_id: u32,
    pub answer_hash: Option<BytesN<32>>,
}

/// #1226/MENTOR-030 — one committed answer on submission. Only the
/// commitment over the off-chain answer is stored, never the answer itself.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Answer {
    pub question_id: u32,
    pub answer_hash: BytesN<32>,
}

/// #1227/MENTOR-031 — one saved field with its own last-updated timestamp.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftAnswer {
    pub question_id: u32,
    pub answer_hash: BytesN<32>,
    pub updated_at: u64,
}

/// #1227/MENTOR-031 — a resumable draft. `revision` is the optimistic
/// concurrency token; `request_token` is the idempotency key of the most
/// recent successful save.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Draft {
    pub applicant: Address,
    pub program_id: BytesN<32>,
    pub form_version: u32,
    pub revision: u32,
    pub request_token: BytesN<32>,
    pub created_at: u64,
    pub last_saved_at: u64,
    pub answers: Vec<DraftAnswer>,
}

/// #1227/MENTOR-031 — explicit last-saved feedback returned to the client.
/// `replayed` is true when the request token matched the last successful
/// save, meaning the call was an idempotent retry and no state changed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftReceipt {
    pub revision: u32,
    pub saved_at: u64,
    pub request_token: BytesN<32>,
    pub replayed: bool,
}

/// #1226/MENTOR-030 — an immutable submitted application record. Review
/// state (priority, assignment, decision) lives separately so submission
/// content can never be rewritten by queue actions.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application {
    pub application_id: u64,
    pub applicant: Address,
    pub program_id: BytesN<32>,
    pub form_version: u32,
    pub consent_version: u32,
    pub answers_count: u32,
    /// Integrity commitment over the off-chain application content (goals,
    /// expectations, schedule, preferred mode, answers, attachments).
    pub data_hash: BytesN<32>,
    pub submitted_at: u64,
}

/// Consent state for one applicant in one program.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsentRecord {
    pub version: u32,
    pub accepted_at: u64,
    pub revoked: bool,
    pub revoked_at: u64,
}

/// #1230/MENTOR-034 — queue priority. Deterministic ordering uses this rank:
/// `Urgent` > `High` > `Normal` > `Low`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

/// #1230/MENTOR-034 — review lifecycle. Derived from the review record, not
/// from the immutable application.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewStatus {
    Pending,
    Assigned,
    Decided,
}

/// #1230/MENTOR-034 — a reviewer's final decision. `Undecided` is the stored
/// initial value and is rejected as a `record_decision` input.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutcome {
    Undecided,
    Accepted,
    Rejected,
}

/// #1230/MENTOR-034 — queue filter selectors (`Any` matches everything).
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueStatusFilter {
    Any,
    Pending,
    Assigned,
    Decided,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuePriorityFilter {
    Any,
    Low,
    Normal,
    High,
    Urgent,
}

/// #1230/MENTOR-034 — a program-scoped reviewer grant. `capacity` bounds the
/// reviewer's active (assigned, undecided) workload; `active` allows a grant
/// to be revoked without losing the configured capacity.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerProfile {
    pub capacity: u32,
    pub active: bool,
}

/// #1230/MENTOR-034 — mutable review/queue state for one application.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewState {
    pub application_id: u64,
    pub priority: Priority,
    pub status: ReviewStatus,
    pub assignee: Option<Address>,
    pub assigned_at: u64,
    pub due_at: u64,
    pub decided_by: Option<Address>,
    pub decided_at: u64,
    pub outcome: DecisionOutcome,
}

/// #1230/MENTOR-034 — one stable queue row.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEntry {
    pub application_id: u64,
    pub applicant: Address,
    pub priority: Priority,
    pub status: ReviewStatus,
    pub assignee: Option<Address>,
    pub due_at: u64,
    pub submitted_at: u64,
    pub overdue: bool,
}

/// #1230/MENTOR-034 — deterministic queue filters. `due_before` only matches
/// entries with a due date (`due_at != 0`) at or before the timestamp.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueFilter {
    pub status: QueueStatusFilter,
    pub priority: QueuePriorityFilter,
    pub assignee: Option<Address>,
    pub unassigned_only: bool,
    pub due_before: Option<u64>,
    pub limit: u32,
}

/// #1230/MENTOR-034 — workload indicator for one reviewer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewerWorkload {
    pub capacity: u32,
    pub assigned_active: u32,
    pub decided: u32,
    pub overdue: u32,
}

/// #1230/MENTOR-034 — one audited queue action.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueAction {
    PriorityChanged,
    Assigned,
    ConflictDeclared,
    ConflictCleared,
    DecisionRecorded,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub actor: Address,
    pub action: QueueAction,
    pub at: u64,
}

#[contract]
pub struct MentorshipApplicationsContract;

#[contractimpl]
impl MentorshipApplicationsContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    fn is_admin(env: &Env, caller: &Address) -> bool {
        match env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::Admin)
        {
            Some(admin) => admin == *caller,
            None => false,
        }
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = Self::admin(env)?;
        if admin != *caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn bump(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn ensure_commitment(env: &Env, hash: &BytesN<32>) -> Result<(), ContractError> {
        if *hash == Self::zero_hash(env) {
            return Err(ContractError::InvalidForm);
        }
        Ok(())
    }

    fn require_active_program(
        env: &Env,
        program_id: &BytesN<32>,
    ) -> Result<Program, ContractError> {
        let program: Program = env
            .storage()
            .persistent()
            .get(&DataKey::Program(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;
        if !program.active {
            return Err(ContractError::ProgramInactive);
        }
        if env.ledger().timestamp() > program.deadline {
            return Err(ContractError::DeadlinePassed);
        }
        Ok(program)
    }

    // ── Programs ──────────────────────────────────────────────────────────

    /// Admin-only: register a program that applications can be submitted
    /// against.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        deadline: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        let program = Program {
            active: true,
            deadline,
        };
        env.storage().persistent().set(&key, &program);
        Self::bump(&env, &key);
        Ok(())
    }

    /// Admin-only: activate/deactivate a program (e.g. to stop intake early).
    pub fn set_program_active(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        let mut program: Program = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        program.active = active;
        env.storage().persistent().set(&key, &program);
        Self::bump(&env, &key);
        Ok(())
    }

    pub fn get_program(env: Env, program_id: BytesN<32>) -> Result<Program, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Program(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    // ── #1228/MENTOR-032 — configurable, versioned forms ─────────────────

    /// Admin-only (the program owner): publish a new, immutable form version.
    /// Versions are never overwritten — publishing again always creates
    /// version N+1. Validation rejects empty forms, duplicate question ids or
    /// orders, zero commitments, a `Choice`/`MultiChoice` question without an
    /// options commitment (or an options commitment on another type), and
    /// visibility conditions that reference the same or a later question, so
    /// conditions are acyclic and deterministically evaluable in one pass.
    pub fn publish_form(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        schema_hash: BytesN<32>,
        questions: Vec<Question>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }
        Self::ensure_commitment(&env, &schema_hash)?;
        Self::validate_form(&env, &questions)?;

        let version_key = DataKey::FormVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let form = MentorshipForm {
            program_id: program_id.clone(),
            version: next_version,
            schema_hash,
            questions,
            published_at: env.ledger().timestamp(),
        };

        let form_key = DataKey::Form(program_id.clone(), next_version);
        env.storage().persistent().set(&form_key, &form);
        Self::bump(&env, &form_key);

        env.storage().persistent().set(&version_key, &next_version);
        Self::bump(&env, &version_key);

        env.events().publish(
            (soroban_sdk::symbol_short!("FORMPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    fn validate_form(env: &Env, questions: &Vec<Question>) -> Result<(), ContractError> {
        if questions.is_empty() || questions.len() > MAX_QUESTIONS {
            return Err(ContractError::InvalidForm);
        }

        let mut i: u32 = 0;
        while i < questions.len() {
            let question = questions.get(i).ok_or(ContractError::InvalidForm)?;
            if question.id == 0 {
                return Err(ContractError::InvalidForm);
            }
            Self::ensure_commitment(env, &question.label_hash)?;
            Self::ensure_commitment(env, &question.help_hash)?;

            let choice = matches!(
                question.kind,
                QuestionType::Choice | QuestionType::MultiChoice
            );
            match &question.options_hash {
                Some(options) => {
                    if !choice {
                        return Err(ContractError::InvalidForm);
                    }
                    Self::ensure_commitment(env, options)?;
                }
                None => {
                    if choice {
                        return Err(ContractError::InvalidForm);
                    }
                }
            }

            if i > 0 {
                let previous = questions.get(i - 1).ok_or(ContractError::InvalidForm)?;
                if question.order <= previous.order || question.id == previous.id {
                    return Err(ContractError::InvalidForm);
                }
            }

            let condition = &question.visible_if;
            if condition.depends_on != 0 {
                // The dependency must be an earlier question: this rejects
                // self references, forward references and cycles for free.
                let mut found = false;
                let mut j: u32 = 0;
                while j < i {
                    if let Some(earlier) = questions.get(j) {
                        if earlier.id == condition.depends_on {
                            found = true;
                            break;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return Err(ContractError::InvalidForm);
                }
                if let Some(hash) = &condition.required_answer_hash {
                    Self::ensure_commitment(env, hash)?;
                }
            } else if condition.required_answer_hash.is_some() {
                // An expected value without a dependency is meaningless.
                return Err(ContractError::InvalidForm);
            }

            i += 1;
        }

        // Ids must be unique even when they are not adjacent.
        let mut a: u32 = 0;
        while a < questions.len() {
            let left = questions.get(a).ok_or(ContractError::InvalidForm)?;
            let mut b: u32 = a + 1;
            while b < questions.len() {
                let right = questions.get(b).ok_or(ContractError::InvalidForm)?;
                if left.id == right.id {
                    return Err(ContractError::InvalidForm);
                }
                b += 1;
            }
            a += 1;
        }

        Ok(())
    }

    pub fn get_latest_form_version(env: Env, program_id: BytesN<32>) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::FormVersion(program_id))
            .ok_or(ContractError::NoFormPublished)
    }

    pub fn get_form(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<MentorshipForm, ContractError> {
        let key = DataKey::Form(program_id, version);
        let form = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::FormVersionNotFound)?;
        Self::bump(&env, &key);
        Ok(form)
    }

    fn load_form(
        env: &Env,
        program_id: &BytesN<32>,
        version: u32,
    ) -> Result<MentorshipForm, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Form(program_id.clone(), version))
            .ok_or(ContractError::FormVersionNotFound)
    }

    /// View helper for off-chain renderers: validates an answer set against a
    /// published form version without touching state, using the same rules as
    /// submission (known ids, no duplicates, visibility, required answers).
    pub fn validate_answers(
        env: Env,
        program_id: BytesN<32>,
        form_version: u32,
        answers: Vec<Answer>,
    ) -> Result<(), ContractError> {
        let form = Self::load_form(&env, &program_id, form_version)?;
        Self::check_answers(&env, &form, &answers)
    }

    fn question_index(questions: &Vec<Question>, id: u32) -> Option<u32> {
        let mut i: u32 = 0;
        while i < questions.len() {
            if let Some(question) = questions.get(i) {
                if question.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn find_answer(answers: &Vec<Answer>, question_id: u32) -> Option<Answer> {
        let mut i: u32 = 0;
        while i < answers.len() {
            if let Some(answer) = answers.get(i) {
                if answer.question_id == question_id {
                    return Some(answer);
                }
            }
            i += 1;
        }
        None
    }

    fn find_draft_answer(answers: &Vec<DraftAnswer>, question_id: u32) -> Option<u32> {
        let mut i: u32 = 0;
        while i < answers.len() {
            if let Some(answer) = answers.get(i) {
                if answer.question_id == question_id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn check_answers(
        env: &Env,
        form: &MentorshipForm,
        answers: &Vec<Answer>,
    ) -> Result<(), ContractError> {
        if answers.len() > MAX_ANSWERS {
            return Err(ContractError::TooManyAnswers);
        }

        let mut i: u32 = 0;
        while i < answers.len() {
            let answer = answers.get(i).ok_or(ContractError::AnswerNotInForm)?;
            if Self::question_index(&form.questions, answer.question_id).is_none() {
                return Err(ContractError::AnswerNotInForm);
            }
            if answer.answer_hash == Self::zero_hash(env) {
                return Err(ContractError::InvalidAnswerHash);
            }
            let mut j: u32 = i + 1;
            while j < answers.len() {
                let other = answers.get(j).ok_or(ContractError::AnswerNotInForm)?;
                if other.question_id == answer.question_id {
                    return Err(ContractError::DuplicateAnswer);
                }
                j += 1;
            }
            i += 1;
        }

        let mut q: u32 = 0;
        while q < form.questions.len() {
            let question = form.questions.get(q).ok_or(ContractError::InvalidForm)?;
            let answered = Self::find_answer(answers, question.id);

            let mut visible = true;
            if question.visible_if.depends_on != 0 {
                visible = match Self::find_answer(answers, question.visible_if.depends_on) {
                    None => false,
                    Some(parent) => match &question.visible_if.required_answer_hash {
                        Some(expected) => parent.answer_hash == *expected,
                        None => true,
                    },
                };
            }

            if !visible {
                if answered.is_some() {
                    return Err(ContractError::AnswerNotVisible);
                }
            } else if question.required && answered.is_none() {
                return Err(ContractError::MissingRequiredAnswer);
            }
            q += 1;
        }

        let _ = env;
        Ok(())
    }

    // ── Consent (required before submission) ────────────────────────────

    /// Admin-only: publish a new, immutable consent-terms version for a
    /// program. Only the commitment enters storage — never the terms text.
    pub fn publish_consent_terms(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        terms_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }
        if terms_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidConsentTerms);
        }

        let version_key = DataKey::ConsentVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let terms_hash_key = DataKey::ConsentTerms(program_id.clone(), next_version);
        env.storage().persistent().set(&terms_hash_key, &terms_hash);
        Self::bump(&env, &terms_hash_key);

        env.storage().persistent().set(&version_key, &next_version);
        Self::bump(&env, &version_key);

        env.events().publish(
            (soroban_sdk::symbol_short!("CONSPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    pub fn get_latest_consent_version(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id))
            .ok_or(ContractError::NoConsentTermsPublished)
    }

    pub fn get_consent_terms(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ConsentTerms(program_id, version))
            .ok_or(ContractError::ConsentTermsNotFound)
    }

    /// Affirmatively record the applicant's consent to the program's latest
    /// published terms version. Recording again after new terms are published
    /// is the re-consent action.
    pub fn record_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        applicant.require_auth();

        let latest_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
            .ok_or(ContractError::NoConsentTermsPublished)?;

        let record = ConsentRecord {
            version: latest_version,
            accepted_at: env.ledger().timestamp(),
            revoked: false,
            revoked_at: 0,
        };

        let key = DataKey::Consent(applicant.clone(), program_id.clone());
        env.storage().persistent().set(&key, &record);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("CONSENT"),),
            (applicant, program_id, latest_version),
        );

        Ok(latest_version)
    }

    /// Revoke the applicant's current consent. This blocks future drafts and
    /// submissions until fresh consent is recorded; it does not rewrite an
    /// application that was already submitted.
    pub fn revoke_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let key = DataKey::Consent(applicant.clone(), program_id);
        let mut record: ConsentRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ConsentNotFound)?;

        record.revoked = true;
        record.revoked_at = env.ledger().timestamp();

        env.storage().persistent().set(&key, &record);
        Self::bump(&env, &key);
        Ok(())
    }

    pub fn get_consent(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<ConsentRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Consent(applicant, program_id))
            .ok_or(ContractError::ConsentNotFound)
    }

    /// True iff the applicant has a non-revoked consent record at the
    /// program's current terms version.
    pub fn has_valid_consent(env: Env, applicant: Address, program_id: BytesN<32>) -> bool {
        let latest_version: u32 = match env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
        {
            Some(version) => version,
            None => return false,
        };
        let record: Option<ConsentRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::Consent(applicant, program_id));
        match record {
            Some(record) => !record.revoked && record.version == latest_version,
            None => false,
        }
    }

    // ── #1227/MENTOR-031 — autosaved drafts ─────────────────────────────

    /// Create or update a resumable draft.
    ///
    /// - `base_revision == 0` creates the draft at revision 1 (fails with
    ///   `DraftConflict` if a draft already exists).
    /// - Otherwise `base_revision` must equal the stored revision; a stale
    ///   base fails with `DraftConflict`, so concurrent edits are never
    ///   silently overwritten.
    /// - A retry carrying the same `request_token` as the last successful
    ///   save is a no-op returning `replayed: true`, so an offline client can
    ///   replay a save it never saw acknowledged.
    /// - Drafts bind to the latest published form version on creation, and
    ///   are rejected once the application is submitted or the deadline has
    ///   passed.
    pub fn autosave_draft(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        base_revision: u32,
        request_token: BytesN<32>,
        updates: Vec<AnswerPatch>,
    ) -> Result<DraftReceipt, ContractError> {
        applicant.require_auth();

        let key = DataKey::Draft(applicant.clone(), program_id.clone());
        let existing: Option<Draft> = env.storage().persistent().get(&key);

        // Idempotent replay is checked before any state or window check, so a
        // retry still returns the original receipt even if the deadline passed
        // or the application was submitted since the save. Nothing changes.
        if let Some(draft) = &existing {
            if draft.request_token == request_token {
                return Ok(DraftReceipt {
                    revision: draft.revision,
                    saved_at: draft.last_saved_at,
                    request_token,
                    replayed: true,
                });
            }
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::ApplicationByApplicant(
                applicant.clone(),
                program_id.clone(),
            ))
        {
            return Err(ContractError::DuplicateApplication);
        }

        Self::require_active_program(&env, &program_id)?;

        let now = env.ledger().timestamp();

        if let Some(draft) = existing {
            if draft.revision != base_revision {
                return Err(ContractError::DraftConflict);
            }

            let form = Self::load_form(&env, &program_id, draft.form_version)?;
            let mut answers = draft.answers.clone();
            Self::apply_patches(&env, &form, &mut answers, &updates, now)?;

            let revision = draft
                .revision
                .checked_add(1)
                .ok_or(ContractError::VersionOverflow)?;

            let updated = Draft {
                applicant: applicant.clone(),
                program_id: program_id.clone(),
                form_version: draft.form_version,
                revision,
                request_token: request_token.clone(),
                created_at: draft.created_at,
                last_saved_at: now,
                answers,
            };
            env.storage().persistent().set(&key, &updated);
            Self::bump(&env, &key);

            env.events().publish(
                (soroban_sdk::symbol_short!("DRAFT"),),
                (applicant, program_id, revision, false),
            );

            return Ok(DraftReceipt {
                revision,
                saved_at: now,
                request_token,
                replayed: false,
            });
        }

        if base_revision != 0 {
            return Err(ContractError::DraftConflict);
        }

        let form_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::FormVersion(program_id.clone()))
            .ok_or(ContractError::NoFormPublished)?;
        let form = Self::load_form(&env, &program_id, form_version)?;

        let mut answers = Vec::new(&env);
        Self::apply_patches(&env, &form, &mut answers, &updates, now)?;

        let draft = Draft {
            applicant: applicant.clone(),
            program_id: program_id.clone(),
            form_version,
            revision: 1,
            request_token: request_token.clone(),
            created_at: now,
            last_saved_at: now,
            answers,
        };
        env.storage().persistent().set(&key, &draft);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("DRAFT"),),
            (applicant, program_id, 1u32, false),
        );

        Ok(DraftReceipt {
            revision: 1,
            saved_at: now,
            request_token,
            replayed: false,
        })
    }

    fn apply_patches(
        env: &Env,
        form: &MentorshipForm,
        answers: &mut Vec<DraftAnswer>,
        updates: &Vec<AnswerPatch>,
        now: u64,
    ) -> Result<(), ContractError> {
        if updates.len() > MAX_ANSWERS {
            return Err(ContractError::TooManyAnswers);
        }

        let mut i: u32 = 0;
        while i < updates.len() {
            let patch = updates.get(i).ok_or(ContractError::AnswerNotInForm)?;
            if Self::question_index(&form.questions, patch.question_id).is_none() {
                return Err(ContractError::AnswerNotInForm);
            }
            let mut j: u32 = i + 1;
            while j < updates.len() {
                let other = updates.get(j).ok_or(ContractError::AnswerNotInForm)?;
                if other.question_id == patch.question_id {
                    return Err(ContractError::DuplicateAnswer);
                }
                j += 1;
            }
            i += 1;
        }

        let mut u: u32 = 0;
        while u < updates.len() {
            let patch = updates.get(u).ok_or(ContractError::AnswerNotInForm)?;
            match &patch.answer_hash {
                Some(hash) => {
                    if *hash == Self::zero_hash(env) {
                        return Err(ContractError::InvalidAnswerHash);
                    }
                    match Self::find_draft_answer(answers, patch.question_id) {
                        Some(index) => {
                            let mut existing =
                                answers.get(index).ok_or(ContractError::AnswerNotInForm)?;
                            existing.answer_hash = hash.clone();
                            existing.updated_at = now;
                            answers.set(index, existing);
                        }
                        None => {
                            if answers.len() >= MAX_ANSWERS {
                                return Err(ContractError::TooManyAnswers);
                            }
                            answers.push_back(DraftAnswer {
                                question_id: patch.question_id,
                                answer_hash: hash.clone(),
                                updated_at: now,
                            });
                        }
                    }
                }
                None => {
                    if let Some(index) = Self::find_draft_answer(answers, patch.question_id) {
                        answers.remove(index);
                    }
                }
            }
            u += 1;
        }
        Ok(())
    }

    pub fn get_draft(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<Draft, ContractError> {
        let key = DataKey::Draft(applicant, program_id);
        let draft = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::DraftNotFound)?;
        Self::bump(&env, &key);
        Ok(draft)
    }

    /// Discard a draft using the same optimistic revision check as autosave,
    /// so a stale client cannot delete a newer draft.
    pub fn discard_draft(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        expected_revision: u32,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let key = DataKey::Draft(applicant.clone(), program_id.clone());
        let draft: Draft = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::DraftNotFound)?;
        if draft.revision != expected_revision {
            return Err(ContractError::DraftConflict);
        }

        env.storage().persistent().remove(&key);
        env.events().publish(
            (soroban_sdk::symbol_short!("DRFTDEL"),),
            (applicant, program_id),
        );
        Ok(())
    }

    // ── #1226/MENTOR-030 — atomic submission ─────────────────────────────

    /// Submit an application atomically. Everything is validated before the
    /// first write: the program is active and inside its window, the accepted
    /// form version exists and every visible required question has an answer,
    /// consent is affirmative, current and unrevoked, and no application
    /// exists for this `(applicant, program)` pair. Any failure reverts the
    /// whole invocation, so a failed check is safe to retry and a retry can
    /// never duplicate a successful submission. On success the draft, if any,
    /// is removed — a submitted application can never be mutated as a draft.
    pub fn submit_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        form_version: u32,
        answers: Vec<Answer>,
        data_hash: BytesN<32>,
        consent: bool,
    ) -> Result<u64, ContractError> {
        applicant.require_auth();

        Self::require_active_program(&env, &program_id)?;

        let form = Self::load_form(&env, &program_id, form_version)?;
        Self::check_answers(&env, &form, &answers)?;

        if !consent {
            return Err(ContractError::ConsentRequired);
        }
        if data_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidDataHash);
        }

        let consent_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ConsentVersion(program_id.clone()))
            .ok_or(ContractError::NoConsentTermsPublished)?;
        let record: ConsentRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Consent(applicant.clone(), program_id.clone()))
            .ok_or(ContractError::ConsentNotFound)?;
        if record.revoked {
            return Err(ContractError::ConsentRevoked);
        }
        if record.version != consent_version {
            return Err(ContractError::ConsentOutOfDate);
        }

        let uniqueness_key = DataKey::ApplicationByApplicant(applicant.clone(), program_id.clone());
        if env.storage().persistent().has(&uniqueness_key) {
            return Err(ContractError::DuplicateApplication);
        }

        let queue_key = DataKey::Queue(program_id.clone());
        let mut queue: Vec<u64> = env
            .storage()
            .persistent()
            .get(&queue_key)
            .unwrap_or(Vec::new(&env));
        if queue.len() >= MAX_QUEUE {
            return Err(ContractError::QueueFull);
        }

        let next: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextApplicationId)
            .unwrap_or(1u64);
        let application_id = next;
        let following = next.checked_add(1).ok_or(ContractError::VersionOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::NextApplicationId, &following);

        let application = Application {
            application_id,
            applicant: applicant.clone(),
            program_id: program_id.clone(),
            form_version,
            consent_version: record.version,
            answers_count: answers.len(),
            data_hash,
            submitted_at: env.ledger().timestamp(),
        };

        let application_key = DataKey::Application(application_id);
        env.storage()
            .persistent()
            .set(&application_key, &application);
        Self::bump(&env, &application_key);

        env.storage()
            .persistent()
            .set(&uniqueness_key, &application_id);
        Self::bump(&env, &uniqueness_key);

        let review = ReviewState {
            application_id,
            priority: Priority::Normal,
            status: ReviewStatus::Pending,
            assignee: None,
            assigned_at: 0,
            due_at: 0,
            decided_by: None,
            decided_at: 0,
            outcome: DecisionOutcome::Undecided,
        };
        let review_key = DataKey::Review(application_id);
        env.storage().persistent().set(&review_key, &review);
        Self::bump(&env, &review_key);

        queue.push_back(application_id);
        env.storage().persistent().set(&queue_key, &queue);
        Self::bump(&env, &queue_key);

        let draft_key = DataKey::Draft(applicant.clone(), program_id.clone());
        if env.storage().persistent().has(&draft_key) {
            env.storage().persistent().remove(&draft_key);
        }

        env.events().publish(
            (soroban_sdk::symbol_short!("SUBMIT"),),
            (application_id, applicant, program_id),
        );

        Ok(application_id)
    }

    fn load_application(env: &Env, application_id: u64) -> Result<Application, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Application(application_id))
            .ok_or(ContractError::ApplicationNotFound)
    }

    pub fn get_application(env: Env, application_id: u64) -> Result<Application, ContractError> {
        let key = DataKey::Application(application_id);
        let application = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ApplicationNotFound)?;
        Self::bump(&env, &key);
        Ok(application)
    }

    pub fn get_application_id(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<u64, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::ApplicationByApplicant(applicant, program_id))
            .ok_or(ContractError::ApplicationNotFound)
    }

    pub fn has_applied(env: Env, applicant: Address, program_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::ApplicationByApplicant(applicant, program_id))
    }

    // ── #1230/MENTOR-034 — scoped reviewer queues ────────────────────────

    /// Admin-only: grant (or re-activate) a reviewer scoped to one program,
    /// with a non-zero active-assignment capacity.
    pub fn grant_reviewer(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        reviewer: Address,
        capacity: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }
        if capacity == 0 {
            return Err(ContractError::InvalidCapacity);
        }

        let profile_key = DataKey::Reviewer(reviewer.clone(), program_id.clone());
        let existing: Option<ReviewerProfile> = env.storage().persistent().get(&profile_key);
        if let Some(profile) = &existing {
            if profile.active {
                return Err(ContractError::AlreadyReviewer);
            }
        }

        let list_key = DataKey::ReviewerList(program_id.clone());
        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or(Vec::new(&env));
        let mut known = false;
        let mut i: u32 = 0;
        while i < list.len() {
            if let Some(address) = list.get(i) {
                if address == reviewer {
                    known = true;
                    break;
                }
            }
            i += 1;
        }
        if !known {
            if list.len() >= MAX_REVIEWERS {
                return Err(ContractError::ReviewerPoolFull);
            }
            list.push_back(reviewer.clone());
            env.storage().persistent().set(&list_key, &list);
            Self::bump(&env, &list_key);
        }

        let profile = ReviewerProfile {
            capacity,
            active: true,
        };
        env.storage().persistent().set(&profile_key, &profile);
        Self::bump(&env, &profile_key);

        env.events().publish(
            (soroban_sdk::symbol_short!("REVGRANT"),),
            (program_id, reviewer, capacity),
        );
        Ok(())
    }

    /// Admin-only: revoke a reviewer's program scope. Their profile (and
    /// capacity) is retained so a later re-grant is explicit.
    pub fn revoke_reviewer(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        reviewer: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let profile_key = DataKey::Reviewer(reviewer.clone(), program_id.clone());
        let mut profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&profile_key)
            .ok_or(ContractError::NotReviewer)?;
        if !profile.active {
            return Err(ContractError::NotReviewer);
        }
        profile.active = false;
        env.storage().persistent().set(&profile_key, &profile);
        Self::bump(&env, &profile_key);

        env.events().publish(
            (soroban_sdk::symbol_short!("REVOKE"),),
            (program_id, reviewer),
        );
        Ok(())
    }

    pub fn get_reviewer(
        env: Env,
        program_id: BytesN<32>,
        reviewer: Address,
    ) -> Result<ReviewerProfile, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Reviewer(reviewer, program_id))
            .ok_or(ContractError::NotReviewer)
    }

    fn is_active_reviewer(env: &Env, program_id: &BytesN<32>, reviewer: &Address) -> bool {
        let profile: Option<ReviewerProfile> = env
            .storage()
            .persistent()
            .get(&DataKey::Reviewer(reviewer.clone(), program_id.clone()));
        match profile {
            Some(profile) => profile.active,
            None => false,
        }
    }

    fn require_reviewer(
        env: &Env,
        program_id: &BytesN<32>,
        reviewer: &Address,
    ) -> Result<(), ContractError> {
        if !Self::is_active_reviewer(env, program_id, reviewer) {
            return Err(ContractError::NotReviewer);
        }
        reviewer.require_auth();
        Ok(())
    }

    fn require_reviewer_or_admin(
        env: &Env,
        program_id: &BytesN<32>,
        caller: &Address,
    ) -> Result<(), ContractError> {
        if Self::is_active_reviewer(env, program_id, caller) {
            caller.require_auth();
            return Ok(());
        }
        if Self::is_admin(env, caller) {
            caller.require_auth();
            return Ok(());
        }
        Err(ContractError::NotAuthorized)
    }

    fn load_review(env: &Env, application_id: u64) -> Result<ReviewState, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Review(application_id))
            .ok_or(ContractError::ApplicationNotFound)
    }

    pub fn get_review_state(env: Env, application_id: u64) -> Result<ReviewState, ContractError> {
        let key = DataKey::Review(application_id);
        let state = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ApplicationNotFound)?;
        Self::bump(&env, &key);
        Ok(state)
    }

    fn append_audit(
        env: &Env,
        application_id: u64,
        actor: &Address,
        action: QueueAction,
    ) -> Result<(), ContractError> {
        let key = DataKey::Audit(application_id);
        let mut trail: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(env));
        if trail.len() >= MAX_AUDIT_ENTRIES {
            return Err(ContractError::AuditTrailFull);
        }
        trail.push_back(AuditEntry {
            actor: actor.clone(),
            action,
            at: env.ledger().timestamp(),
        });
        env.storage().persistent().set(&key, &trail);
        Self::bump(env, &key);
        Ok(())
    }

    pub fn get_audit_trail(
        env: Env,
        application_id: u64,
    ) -> Result<Vec<AuditEntry>, ContractError> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Application(application_id))
        {
            return Err(ContractError::ApplicationNotFound);
        }
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Audit(application_id))
            .unwrap_or(Vec::new(&env)))
    }

    /// Set queue priority. Available to the admin and to reviewers scoped to
    /// the application's program; a decided application is immutable.
    pub fn set_priority(
        env: Env,
        caller: Address,
        application_id: u64,
        priority: Priority,
    ) -> Result<(), ContractError> {
        let application = Self::load_application(&env, application_id)?;
        Self::require_reviewer_or_admin(&env, &application.program_id, &caller)?;

        let key = DataKey::Review(application_id);
        let mut review = Self::load_review(&env, application_id)?;
        if review.status == ReviewStatus::Decided {
            return Err(ContractError::ApplicationAlreadyDecided);
        }
        if review.priority == priority {
            return Ok(());
        }

        // Audit first: `append_audit` is the only fallible write left, so a
        // full trail cannot leave the priority changed without a record.
        Self::append_audit(&env, application_id, &caller, QueueAction::PriorityChanged)?;
        review.priority = priority;
        env.storage().persistent().set(&key, &review);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("PRIO"),),
            (application_id, priority),
        );
        Ok(())
    }

    /// Assign (or reassign) an application to a scoped reviewer with an
    /// optional due date (`0` = no due date). Assignment is concurrency-safe:
    /// `expected_assignee` must equal the current assignee (`None` while
    /// unassigned), otherwise the call fails with `AssignmentConflict` and the
    /// newer assignment is left untouched. Repeating the same assignment is an
    /// idempotent no-op. A reviewer at capacity is rejected.
    pub fn assign_reviewer(
        env: Env,
        caller: Address,
        application_id: u64,
        reviewer: Address,
        expected_assignee: Option<Address>,
        due_at: u64,
    ) -> Result<(), ContractError> {
        let application = Self::load_application(&env, application_id)?;
        Self::require_reviewer_or_admin(&env, &application.program_id, &caller)?;
        if !Self::is_active_reviewer(&env, &application.program_id, &reviewer) {
            return Err(ContractError::NotReviewer);
        }

        let mut review = Self::load_review(&env, application_id)?;
        if review.status == ReviewStatus::Decided {
            return Err(ContractError::ApplicationAlreadyDecided);
        }
        if review.assignee != expected_assignee {
            return Err(ContractError::AssignmentConflict);
        }

        // Idempotent retry of an assignment that already landed.
        if review.assignee == Some(reviewer.clone()) && review.due_at == due_at {
            return Ok(());
        }

        let changing_assignee = review.assignee != Some(reviewer.clone());
        if changing_assignee {
            let profile = Self::get_reviewer(
                env.clone(),
                application.program_id.clone(),
                reviewer.clone(),
            )?;
            let workload = Self::reviewer_workload(
                env.clone(),
                application.program_id.clone(),
                reviewer.clone(),
            );
            if workload.assigned_active >= profile.capacity {
                return Err(ContractError::ReviewerAtCapacity);
            }
        }

        Self::append_audit(&env, application_id, &caller, QueueAction::Assigned)?;

        let now = env.ledger().timestamp();
        review.assignee = Some(reviewer.clone());
        review.assigned_at = now;
        review.due_at = due_at;
        review.status = ReviewStatus::Assigned;

        let key = DataKey::Review(application_id);
        env.storage().persistent().set(&key, &review);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("ASSIGN"),),
            (application_id, reviewer, due_at),
        );
        Ok(())
    }

    /// A scoped reviewer declares or clears their own conflict of interest for
    /// an application. A declared conflict blocks that reviewer from recording
    /// a decision; it does not silently unassign them, so the conflict is
    /// visible to coordinators in the audit trail.
    pub fn declare_conflict(
        env: Env,
        reviewer: Address,
        application_id: u64,
        declared: bool,
    ) -> Result<(), ContractError> {
        let application = Self::load_application(&env, application_id)?;
        Self::require_reviewer(&env, &application.program_id, &reviewer)?;

        let review = Self::load_review(&env, application_id)?;
        if review.status == ReviewStatus::Decided {
            return Err(ContractError::ApplicationAlreadyDecided);
        }

        let key = DataKey::Conflict(reviewer.clone(), application_id);
        let current: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if current == declared {
            return Ok(());
        }

        let action = if declared {
            QueueAction::ConflictDeclared
        } else {
            QueueAction::ConflictCleared
        };
        Self::append_audit(&env, application_id, &reviewer, action)?;
        env.storage().persistent().set(&key, &declared);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("COI"),),
            (application_id, reviewer, declared),
        );
        Ok(())
    }

    pub fn has_conflict(env: Env, application_id: u64, reviewer: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Conflict(reviewer, application_id))
            .unwrap_or(false)
    }

    /// Record the assigned reviewer's final decision. Deny-by-default: only
    /// the current assignee (who must still be an active scoped reviewer and
    /// free of a declared conflict) may decide, and an application can be
    /// decided only once.
    pub fn record_decision(
        env: Env,
        reviewer: Address,
        application_id: u64,
        outcome: DecisionOutcome,
    ) -> Result<(), ContractError> {
        let application = Self::load_application(&env, application_id)?;
        Self::require_reviewer(&env, &application.program_id, &reviewer)?;

        let mut review = Self::load_review(&env, application_id)?;
        if review.status == ReviewStatus::Decided {
            return Err(ContractError::ApplicationAlreadyDecided);
        }
        if outcome == DecisionOutcome::Undecided {
            return Err(ContractError::InvalidDecision);
        }
        if review.assignee != Some(reviewer.clone()) {
            return Err(ContractError::NotAssignedReviewer);
        }
        let conflicted: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Conflict(reviewer.clone(), application_id))
            .unwrap_or(false);
        if conflicted {
            return Err(ContractError::ConflictOfInterest);
        }

        Self::append_audit(
            &env,
            application_id,
            &reviewer,
            QueueAction::DecisionRecorded,
        )?;

        review.status = ReviewStatus::Decided;
        review.decided_by = Some(reviewer.clone());
        review.decided_at = env.ledger().timestamp();
        review.outcome = outcome;

        let key = DataKey::Review(application_id);
        env.storage().persistent().set(&key, &review);
        Self::bump(&env, &key);

        env.events().publish(
            (soroban_sdk::symbol_short!("DECIDE"),),
            (application_id, reviewer, outcome),
        );
        Ok(())
    }

    fn status_matches(filter: QueueStatusFilter, status: ReviewStatus) -> bool {
        match filter {
            QueueStatusFilter::Any => true,
            QueueStatusFilter::Pending => status == ReviewStatus::Pending,
            QueueStatusFilter::Assigned => status == ReviewStatus::Assigned,
            QueueStatusFilter::Decided => status == ReviewStatus::Decided,
        }
    }

    fn priority_matches(filter: QueuePriorityFilter, priority: Priority) -> bool {
        match filter {
            QueuePriorityFilter::Any => true,
            QueuePriorityFilter::Low => priority == Priority::Low,
            QueuePriorityFilter::Normal => priority == Priority::Normal,
            QueuePriorityFilter::High => priority == Priority::High,
            QueuePriorityFilter::Urgent => priority == Priority::Urgent,
        }
    }

    fn priority_rank(priority: Priority) -> u32 {
        match priority {
            Priority::Low => 0,
            Priority::Normal => 1,
            Priority::High => 2,
            Priority::Urgent => 3,
        }
    }

    /// Deterministic ordering: highest priority first, then earliest due date
    /// (undated last), then ascending application id. The order depends only
    /// on stored state, so repeated reads are stable.
    fn queue_entry_before(left: &QueueEntry, right: &QueueEntry) -> bool {
        let left_rank = Self::priority_rank(left.priority);
        let right_rank = Self::priority_rank(right.priority);
        if left_rank != right_rank {
            return left_rank > right_rank;
        }
        let left_due = if left.due_at == 0 {
            u64::MAX
        } else {
            left.due_at
        };
        let right_due = if right.due_at == 0 {
            u64::MAX
        } else {
            right.due_at
        };
        if left_due != right_due {
            return left_due < right_due;
        }
        left.application_id < right.application_id
    }

    fn sort_entries(entries: &mut Vec<QueueEntry>) {
        // Insertion sort over a bounded (`MAX_QUEUE`) list; stable and
        // integer-only, so the ordering is fully deterministic.
        let mut i: u32 = 1;
        while i < entries.len() {
            let mut j = i;
            while j > 0 {
                let left = entries.get(j - 1);
                let right = entries.get(j);
                if let (Some(left), Some(right)) = (left, right) {
                    if Self::queue_entry_before(&left, &right) {
                        break;
                    }
                    entries.set(j - 1, right);
                    entries.set(j, left);
                }
                j -= 1;
            }
            i += 1;
        }
    }

    /// Read the reviewer's own, program-scoped queue. Callers must be the
    /// admin or an active reviewer scoped to `program_id`, so applications
    /// from other programs can never appear. Filters combine with AND and the
    /// result is stable (see `queue_entry_before`), capped at
    /// `MAX_QUEUE_PAGE`.
    pub fn list_queue(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        filter: QueueFilter,
    ) -> Result<Vec<QueueEntry>, ContractError> {
        Self::require_reviewer_or_admin(&env, &program_id, &caller)?;

        if filter.limit == 0 || filter.limit > MAX_QUEUE_PAGE {
            return Err(ContractError::InvalidFilter);
        }
        if filter.unassigned_only && filter.assignee.is_some() {
            return Err(ContractError::InvalidFilter);
        }
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Program(program_id.clone()))
        {
            return Err(ContractError::ProgramNotFound);
        }

        let queue: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::Queue(program_id.clone()))
            .unwrap_or(Vec::new(&env));
        let now = env.ledger().timestamp();
        let mut entries: Vec<QueueEntry> = Vec::new(&env);

        let mut i: u32 = 0;
        while i < queue.len() {
            if let Some(application_id) = queue.get(i) {
                if let Some(application) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Application>(&DataKey::Application(application_id))
                {
                    if let Some(review) = env
                        .storage()
                        .persistent()
                        .get::<DataKey, ReviewState>(&DataKey::Review(application_id))
                    {
                        let mut keep = Self::status_matches(filter.status, review.status)
                            && Self::priority_matches(filter.priority, review.priority);
                        if let Some(assignee) = &filter.assignee {
                            if review.assignee.as_ref() != Some(assignee) {
                                keep = false;
                            }
                        }
                        if filter.unassigned_only && review.assignee.is_some() {
                            keep = false;
                        }
                        if let Some(due_before) = filter.due_before {
                            if review.due_at == 0 || review.due_at > due_before {
                                keep = false;
                            }
                        }

                        if keep {
                            let overdue = review.due_at != 0
                                && review.status != ReviewStatus::Decided
                                && now > review.due_at;
                            entries.push_back(QueueEntry {
                                application_id,
                                applicant: application.applicant,
                                priority: review.priority,
                                status: review.status,
                                assignee: review.assignee,
                                due_at: review.due_at,
                                submitted_at: application.submitted_at,
                                overdue,
                            });
                        }
                    }
                }
            }
            i += 1;
        }

        Self::sort_entries(&mut entries);
        while entries.len() > filter.limit {
            entries.pop_back();
        }
        Ok(entries)
    }

    /// Workload indicator for one reviewer in one program: active
    /// (assigned, undecided) load against capacity, lifetime decisions, and
    /// the number of active assignments past their due date.
    pub fn reviewer_workload(
        env: Env,
        program_id: BytesN<32>,
        reviewer: Address,
    ) -> ReviewerWorkload {
        let capacity: u32 =
            match env
                .storage()
                .persistent()
                .get::<DataKey, ReviewerProfile>(&DataKey::Reviewer(
                    reviewer.clone(),
                    program_id.clone(),
                )) {
                Some(profile) => profile.capacity,
                None => 0,
            };

        let queue: Vec<u64> = env
            .storage()
            .persistent()
            .get(&DataKey::Queue(program_id))
            .unwrap_or(Vec::new(&env));
        let now = env.ledger().timestamp();

        let mut assigned_active: u32 = 0;
        let mut decided: u32 = 0;
        let mut overdue: u32 = 0;

        let mut i: u32 = 0;
        while i < queue.len() {
            if let Some(application_id) = queue.get(i) {
                if let Some(review) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, ReviewState>(&DataKey::Review(application_id))
                {
                    if review.assignee == Some(reviewer.clone()) {
                        match review.status {
                            ReviewStatus::Decided => decided += 1,
                            _ => {
                                assigned_active += 1;
                                if review.due_at != 0 && now > review.due_at {
                                    overdue += 1;
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        ReviewerWorkload {
            capacity,
            assigned_active,
            decided,
            overdue,
        }
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
