use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    ApplicationError, ApplicationStatus, CommunicationPolicy, DecisionAction, DecisionInput,
    MentorshipApplications, MentorshipApplicationsClient, ProgramPolicy, WithdrawalInput,
};

fn hash(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn base_policy() -> ProgramPolicy {
    ProgramPolicy {
        decision_cutoff: 0,
        withdrawal_cutoff: 0,
        allow_reapplication: false,
        reapply_cooldown: 0,
        max_attempts: 0,
        max_active_applications: 0,
    }
}

fn reapply_policy() -> ProgramPolicy {
    ProgramPolicy {
        allow_reapplication: true,
        ..base_policy()
    }
}

struct Ctx {
    env: Env,
    contract_id: Address,
    admin: Address,
    coordinator: Address,
    program: BytesN<32>,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipApplications, ());
    let client = MentorshipApplicationsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let program = hash(&env, 9);
    client.initialize(&admin);
    client.set_coordinator(&admin, &coordinator, &true);
    client.set_program_policy(&admin, &program, &base_policy());
    Ctx {
        env,
        contract_id,
        admin,
        coordinator,
        program,
    }
}

impl Ctx {
    fn client(&self) -> MentorshipApplicationsClient<'_> {
        MentorshipApplicationsClient::new(&self.env, &self.contract_id)
    }

    fn set_policy(&self, policy: &ProgramPolicy) {
        self.client()
            .set_program_policy(&self.admin, &self.program, policy);
    }

    fn open(&self, applicant: &Address) -> BytesN<32> {
        self.client()
            .open_application(applicant, &self.program, &1u32)
    }

    fn accept_input(&self) -> DecisionInput {
        DecisionInput {
            public_reason: 10,
            internal_reason_hash: zero(&self.env),
            effective_at: 0,
            communication: CommunicationPolicy::NotifyImmediately,
        }
    }

    fn reject_input(&self) -> DecisionInput {
        DecisionInput {
            public_reason: 20,
            internal_reason_hash: hash(&self.env, 7),
            effective_at: 0,
            communication: CommunicationPolicy::NotifyAtEffectiveTime,
        }
    }

    fn withdraw_input(&self) -> WithdrawalInput {
        WithdrawalInput {
            public_reason: 30,
            effective_at: 0,
            communication: CommunicationPolicy::NotifyImmediately,
            acknowledge_consequences: true,
        }
    }
}

// ===== Bootstrap and authorization =====

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    let other = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_initialize(&other),
        Err(Ok(ApplicationError::AlreadyInitialized))
use crate::{
    Answer, AnswerPatch, AuditEntry, ContractError, DecisionOutcome,
    MentorshipApplicationsContract, MentorshipApplicationsContractClient, Priority, Question,
    QuestionType, QueueAction, QueueFilter, QueuePriorityFilter, QueueStatusFilter, ReviewStatus,
    VisibilityCondition,
};
use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke};
use soroban_sdk::{
    symbol_short, vec, Address, BytesN, Env, IntoVal, Symbol, TryFromVal, TryIntoVal, Vec,
};

// ── Helpers ────────────────────────────────────────────────────────────────

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipApplicationsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipApplicationsContractClient<'a> {
    MentorshipApplicationsContractClient::new(env, contract_id)
}

fn id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn program(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn question(env: &Env, qid: u32, kind: QuestionType, required: bool, order: u32) -> Question {
    let choices = matches!(kind, QuestionType::Choice | QuestionType::MultiChoice);
    Question {
        id: qid,
        kind,
        required,
        order,
        label_hash: id(env, (qid + 1) as u8),
        help_hash: id(env, (qid + 20) as u8),
        options_hash: if choices { Some(id(env, 99)) } else { None },
        visible_if: VisibilityCondition {
            depends_on: 0,
            required_answer_hash: None,
        },
    }
}

fn default_questions(env: &Env) -> Vec<Question> {
    vec![
        env,
        question(env, 1, QuestionType::ShortText, true, 1),
        question(env, 2, QuestionType::LongText, false, 2),
        question(env, 3, QuestionType::Choice, false, 3),
    ]
}

fn conditional_questions(env: &Env) -> Vec<Question> {
    let mut conditional = question(env, 3, QuestionType::ShortText, true, 3);
    conditional.visible_if = VisibilityCondition {
        depends_on: 2,
        required_answer_hash: Some(id(env, 7)),
    };
    vec![
        env,
        question(env, 1, QuestionType::ShortText, true, 1),
        question(env, 2, QuestionType::Choice, false, 2),
        conditional,
    ]
}

fn answer(env: &Env, qid: u32, byte: u8) -> Answer {
    Answer {
        question_id: qid,
        answer_hash: id(env, byte),
    }
}

fn default_answers(env: &Env) -> Vec<Answer> {
    vec![env, answer(env, 1, 11), answer(env, 2, 12)]
}

fn patch(env: &Env, qid: u32, byte: u8) -> AnswerPatch {
    AnswerPatch {
        question_id: qid,
        answer_hash: Some(id(env, byte)),
    }
}

fn empty_filter() -> QueueFilter {
    QueueFilter {
        status: QueueStatusFilter::Any,
        priority: QueuePriorityFilter::Any,
        assignee: None,
        unassigned_only: false,
        due_before: None,
        limit: 10,
    }
}

fn boot(client: &MentorshipApplicationsContractClient, admin: &Address) {
    client.initialize(admin);
}

fn register(
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
    program_id: &BytesN<32>,
    deadline: u64,
) {
    client.register_program(admin, program_id, &deadline);
}

fn publish_default_form(
    env: &Env,
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
    program_id: &BytesN<32>,
) -> u32 {
    client.publish_form(admin, program_id, &id(env, 88), &default_questions(env))
}

fn publish_terms(
    env: &Env,
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
    program_id: &BytesN<32>,
) -> u32 {
    client.publish_consent_terms(admin, program_id, &id(env, 77))
}

fn add_program(
    env: &Env,
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
    program_id: &BytesN<32>,
    deadline: u64,
) {
    register(client, admin, program_id, deadline);
    publish_default_form(env, client, admin, program_id);
    publish_terms(env, client, admin, program_id);
}

fn ready_program(
    env: &Env,
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
) -> BytesN<32> {
    boot(client, admin);
    let program_id = program(env, 1);
    add_program(env, client, admin, &program_id, 1_000_000);
    program_id
}

fn apply_for(
    env: &Env,
    client: &MentorshipApplicationsContractClient,
    program_id: &BytesN<32>,
) -> (Address, u64) {
    let applicant = Address::generate(env);
    client.record_consent(&applicant, program_id);
    let app_id = client.submit_application(
        &applicant,
        program_id,
        &1u32,
        &default_answers(env),
        &id(env, 60),
        &true,
    );
    (applicant, app_id)
}

fn none_address() -> Option<Address> {
    None
}

// ── Lifecycle and programs ─────────────────────────────────────────────────

#[test]
fn test_initialize_only_once() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    client.initialize(&admin);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_set_coordinator_requires_admin() {
    let ctx = setup();
    let outsider = Address::generate(&ctx.env);
    let target = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_set_coordinator(&outsider, &target, &true),
        Err(Ok(ApplicationError::Unauthorized))
fn test_version_is_stable() {
    let (env, contract_id, _admin) = setup();
    assert_eq!(client(&env, &contract_id).version(), 1);
}

#[test]
fn test_admin_entrypoints_require_initialization() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    assert_eq!(
        client.try_register_program(&admin, &program(&env, 1), &1_000u64),
        Err(Ok(ContractError::NotInitialized))
    );
}

#[test]
fn test_coordinator_can_be_revoked() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    assert!(client.is_coordinator(&ctx.coordinator));

    client.set_coordinator(&ctx.admin, &ctx.coordinator, &false);
    assert!(!client.is_coordinator(&ctx.coordinator));
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Review,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::Unauthorized))
fn test_register_program_is_admin_only() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_register_program(&attacker, &program(&env, 1), &1_000u64),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_register_program_twice_fails() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);
    assert_eq!(
        client.try_register_program(&admin, &program_id, &2_000u64),
        Err(Ok(ContractError::ProgramAlreadyExists))
    );
}

#[test]
fn test_invalid_policy_rejected() {
    let ctx = setup();
    let inconsistent = ProgramPolicy {
        max_attempts: 2,
        allow_reapplication: false,
        ..base_policy()
    };
    assert_eq!(
        ctx.client()
            .try_set_program_policy(&ctx.admin, &ctx.program, &inconsistent),
        Err(Ok(ApplicationError::InvalidPolicy))
    );
}

// ===== Opening applications =====

#[test]
fn test_open_application_requires_policy() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let unknown_program = hash(&ctx.env, 42);
    assert_eq!(
        client.try_open_application(&applicant, &unknown_program, &1u32),
        Err(Ok(ApplicationError::PolicyNotFound))
fn test_program_active_toggle_and_lookup() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    assert!(client.get_program(&program_id).active);
    client.set_program_active(&admin, &program_id, &false);
    assert!(!client.get_program(&program_id).active);
    assert_eq!(
        client.try_get_program(&program(&env, 9)),
        Err(Ok(ContractError::ProgramNotFound))
    );
}

// ── #1228/MENTOR-032 — configurable, versioned forms ──────────────────────

#[test]
fn test_publish_form_starts_at_version_one() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let version = publish_default_form(&env, &client, &admin, &program_id);
    assert_eq!(version, 1);
    assert_eq!(client.get_latest_form_version(&program_id), 1);

    let form = client.get_form(&program_id, &1);
    assert_eq!(form.version, 1);
    assert_eq!(form.questions.len(), 3);
    assert_eq!(form.questions.get(0).unwrap().order, 1);
    assert!(form.questions.get(0).unwrap().required);
    assert!(!form.questions.get(1).unwrap().required);
}

#[test]
fn test_publish_form_versions_are_immutable() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    publish_default_form(&env, &client, &admin, &program_id);
    let v2 = client.publish_form(
        &admin,
        &program_id,
        &id(&env, 90),
        &vec![&env, question(&env, 9, QuestionType::Email, true, 1)],
    );
    assert_eq!(v2, 2);

    let v1 = client.get_form(&program_id, &1);
    assert_eq!(v1.questions.len(), 3);
    let v2_form = client.get_form(&program_id, &2);
    assert_eq!(v2_form.questions.len(), 1);
    assert_eq!(v2_form.schema_hash, id(&env, 90));
}

#[test]
fn test_publish_form_requires_admin() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);
    let attacker = Address::generate(&env);

    assert_eq!(
        client.try_publish_form(
            &attacker,
            &program_id,
            &id(&env, 88),
            &default_questions(&env)
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_open_application_rejects_zero_consent() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    assert_eq!(
        client.try_open_application(&applicant, &ctx.program, &0u32),
        Err(Ok(ApplicationError::InvalidConsent))
fn test_publish_form_requires_registered_program() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    assert_eq!(
        client.try_publish_form(
            &admin,
            &program(&env, 9),
            &id(&env, 88),
            &default_questions(&env)
        ),
        Err(Ok(ContractError::ProgramNotFound))
    );
}

#[test]
fn test_open_application_stores_first_attempt() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    ctx.env.ledger().set_timestamp(1_000);
    let app_id = ctx.open(&applicant);

    let application = client.get_application(&app_id);
    assert_eq!(application.applicant, applicant);
    assert_eq!(application.program_id, ctx.program);
    assert_eq!(application.attempt, 1);
    assert_eq!(application.status, ApplicationStatus::Submitted);
    assert_eq!(application.consent_version, 1);
    assert_eq!(application.consent_affirmed_at, 1_000);
    assert_eq!(application.previous_application, None);
    assert_eq!(application.decision_count, 0);
    assert_eq!(application.last_decision_seq, 0);
    assert_eq!(client.active_application_count(&ctx.program), 1);
}

#[test]
fn test_program_policy_is_readable() {
    let ctx = setup();
    let policy = ctx.client().get_program_policy(&ctx.program);
    assert!(!policy.allow_reapplication);
    assert_eq!(policy.max_active_applications, 0);
}

// ===== Structured decisions (MENTOR-035) =====

#[test]
fn test_decision_requires_coordinator() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    assert_eq!(
        client.try_record_decision(
            &stranger,
            &app_id,
            &DecisionAction::Accept,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::Unauthorized))
fn test_publish_form_rejects_empty_form() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &Vec::new(&env)),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_publish_form_rejects_duplicate_question_ids() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let questions = vec![
        &env,
        question(&env, 1, QuestionType::ShortText, true, 1),
        question(&env, 1, QuestionType::ShortText, false, 2),
    ];
    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &questions),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_publish_form_rejects_non_increasing_order() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let questions = vec![
        &env,
        question(&env, 1, QuestionType::ShortText, true, 2),
        question(&env, 2, QuestionType::ShortText, false, 1),
    ];
    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &questions),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_accept_decision_is_recorded_and_releases_reservation() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    ctx.env.ledger().set_timestamp(200);

    let seq = client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Accept,
        &ctx.accept_input(),
    );
    assert_eq!(seq, 1);

    let application = client.get_application(&app_id);
    assert_eq!(application.status, ApplicationStatus::Accepted);
    assert_eq!(application.decision_count, 1);
    assert_eq!(application.last_decision_seq, 1);
    assert_eq!(application.closed_at, 200);
    assert_eq!(client.active_application_count(&ctx.program), 0);

    let view = client.applicant_visible_decision(&app_id, &1u32);
    assert_eq!(view.action, DecisionAction::Accept);
    assert_eq!(view.resulting_status, ApplicationStatus::Accepted);
    assert_eq!(view.public_reason, 10);
    assert_eq!(view.effective_at, 200);
}

#[test]
fn test_waitlist_requires_internal_reason_commitment() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    let missing_reason = DecisionInput {
        public_reason: 21,
        internal_reason_hash: zero(&ctx.env),
        effective_at: 0,
        communication: CommunicationPolicy::InternalOnly,
    };
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Waitlist,
            &missing_reason
        ),
        Err(Ok(ApplicationError::InternalReasonRequired))
    );

    let seq = client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Waitlist,
        &ctx.reject_input(),
    );
    assert_eq!(seq, 1);
    assert_eq!(
        client.get_application(&app_id).status,
        ApplicationStatus::Waitlisted
fn test_publish_form_rejects_zero_commitments() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let mut q = question(&env, 1, QuestionType::ShortText, true, 1);
    q.label_hash = id(&env, 0);
    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &vec![&env, q]),
        Err(Ok(ContractError::InvalidForm))
    );

    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 0), &default_questions(&env)),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_publish_form_rejects_choice_options_mismatch() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let mut choice_without_options = question(&env, 1, QuestionType::Choice, true, 1);
    choice_without_options.options_hash = None;
    assert_eq!(
        client.try_publish_form(
            &admin,
            &program_id,
            &id(&env, 88),
            &vec![&env, choice_without_options]
        ),
        Err(Ok(ContractError::InvalidForm))
    );

    let mut text_with_options = question(&env, 1, QuestionType::ShortText, true, 1);
    text_with_options.options_hash = Some(id(&env, 5));
    assert_eq!(
        client.try_publish_form(
            &admin,
            &program_id,
            &id(&env, 88),
            &vec![&env, text_with_options]
        ),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_decision_requires_public_reason_code() {
    let ctx = setup();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    let input = DecisionInput {
        public_reason: 0,
        ..ctx.accept_input()
    };
    assert_eq!(
        ctx.client().try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Accept,
            &input
        ),
        Err(Ok(ApplicationError::InvalidReasonCode))
fn test_publish_form_rejects_forward_visibility_reference() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let mut first = question(&env, 1, QuestionType::ShortText, false, 1);
    first.visible_if = VisibilityCondition {
        depends_on: 2,
        required_answer_hash: None,
    };
    let second = question(&env, 2, QuestionType::ShortText, false, 2);
    assert_eq!(
        client.try_publish_form(
            &admin,
            &program_id,
            &id(&env, 88),
            &vec![&env, first, second]
        ),
        Err(Ok(ContractError::InvalidForm))
    );

    // A self reference is rejected for the same reason.
    let mut self_ref = question(&env, 3, QuestionType::ShortText, false, 1);
    self_ref.visible_if = VisibilityCondition {
        depends_on: 3,
        required_answer_hash: None,
    };
    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &vec![&env, self_ref]),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_decision_cannot_be_backdated_and_zero_means_now() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    ctx.env.ledger().set_timestamp(1_000);

    let backdated = DecisionInput {
        effective_at: 999,
        ..ctx.accept_input()
    };
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Review,
            &backdated
        ),
        Err(Ok(ApplicationError::InvalidEffectiveTime))
    );

    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Review,
        &ctx.accept_input(),
    );
    assert_eq!(
        client
            .applicant_visible_decision(&app_id, &1u32)
            .effective_at,
        1_000
fn test_publish_form_accepts_conditional_questions() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let version = client.publish_form(
        &admin,
        &program_id,
        &id(&env, 88),
        &conditional_questions(&env),
    );
    assert_eq!(version, 1);
    let form = client.get_form(&program_id, &1);
    assert_eq!(form.questions.get(2).unwrap().visible_if.depends_on, 2);
}

#[test]
fn test_publish_form_rejects_more_than_max_questions() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    let mut questions = Vec::new(&env);
    let mut qid: u32 = 1;
    while qid <= 33 {
        questions.push_back(question(&env, qid, QuestionType::ShortText, false, qid));
        qid += 1;
    }
    assert_eq!(
        client.try_publish_form(&admin, &program_id, &id(&env, 88), &questions),
        Err(Ok(ContractError::InvalidForm))
    );
}

#[test]
fn test_form_lookup_errors() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);

    assert_eq!(
        client.try_get_latest_form_version(&program_id),
        Err(Ok(ContractError::NoFormPublished))
    );
    assert_eq!(
        client.try_get_form(&program_id, &1),
        Err(Ok(ContractError::FormVersionNotFound))
    );
}

#[test]
fn test_duplicate_decision_is_idempotent() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    let input = ctx.accept_input();

    let first = client.record_decision(&ctx.coordinator, &app_id, &DecisionAction::Accept, &input);
    let second = client.record_decision(&ctx.coordinator, &app_id, &DecisionAction::Accept, &input);
    assert_eq!(first, second);
    assert_eq!(client.decision_count(&app_id), 1);
    // The replay emitted no event at all, proving it did not duplicate effects.
    assert_eq!(ctx.env.events().all().len(), 0);

    // A *different* decision after a final status is a genuine error, not a replay.
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Reject,
            &ctx.reject_input()
        ),
        Err(Ok(ApplicationError::InvalidTransition))
    );
}

#[test]
fn test_decision_after_withdrawal_is_rejected() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &app_id, &ctx.withdraw_input());
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Accept,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::InvalidTransition))
    );
}

#[test]
fn test_coordinator_cannot_record_withdrawal() {
    let ctx = setup();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    assert_eq!(
        ctx.client().try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Withdraw,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::WithdrawalIsApplicantAction))
    );
}

#[test]
fn test_decision_window_closes_decisions() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    ctx.set_policy(&ProgramPolicy {
        decision_cutoff: 500,
        ..base_policy()
    });
    ctx.env.ledger().set_timestamp(499);
    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Review,
        &ctx.accept_input(),
    );
    ctx.env.ledger().set_timestamp(500);
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::Accept,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::DecisionWindowClosed))
fn test_validate_answers_matches_submission_rules() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000);
    client.publish_form(
        &admin,
        &program_id,
        &id(&env, 88),
        &conditional_questions(&env),
    );

    // q1 required, q2 optional, q3 visible+required only when q2 commits 0x07.
    assert_eq!(
        client.try_validate_answers(&program_id, &1, &Vec::new(&env)),
        Err(Ok(ContractError::MissingRequiredAnswer))
    );
    let only_two = vec![&env, answer(&env, 2, 8)];
    assert_eq!(
        client.try_validate_answers(&program_id, &1, &only_two),
        Err(Ok(ContractError::MissingRequiredAnswer))
    );

    let hidden_answered = vec![
        &env,
        answer(&env, 1, 11),
        answer(&env, 2, 8),
        answer(&env, 3, 13),
    ];
    assert_eq!(
        client.try_validate_answers(&program_id, &1, &hidden_answered),
        Err(Ok(ContractError::AnswerNotVisible))
    );

    let visible_required_missing = vec![&env, answer(&env, 1, 11), answer(&env, 2, 7)];
    assert_eq!(
        client.try_validate_answers(&program_id, &1, &visible_required_missing),
        Err(Ok(ContractError::MissingRequiredAnswer))
    );

    let valid = vec![
        &env,
        answer(&env, 1, 11),
        answer(&env, 2, 7),
        answer(&env, 3, 13),
    ];
    assert_eq!(
        client.try_validate_answers(&program_id, &1, &valid),
        Ok(Ok(()))
    );
}

// ── #1227/MENTOR-031 — autosaved drafts ───────────────────────────────────

#[test]
fn test_autosave_creates_revision_one_with_field_timestamps() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    env.ledger().set_timestamp(10);

    let receipt = client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11), patch(&env, 2, 12)],
    );
    assert_eq!(receipt.revision, 1);
    assert_eq!(receipt.saved_at, 10);
    assert!(!receipt.replayed);

    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.form_version, 1);
    assert_eq!(draft.answers.len(), 2);
    assert_eq!(draft.answers.get(0).unwrap().updated_at, 10);
    assert_eq!(draft.answers.get(1).unwrap().updated_at, 10);
}

#[test]
fn test_autosave_merges_fields_and_bumps_revision() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    env.ledger().set_timestamp(10);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11), patch(&env, 2, 12)],
    );

    env.ledger().set_timestamp(20);
    let receipt = client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 2, 22)],
    );
    assert_eq!(receipt.revision, 2);
    assert_eq!(receipt.saved_at, 20);

    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.revision, 2);
    assert_eq!(draft.answers.len(), 2);
    let q1 = draft.answers.get(0).unwrap();
    let q2 = draft.answers.get(1).unwrap();
    assert_eq!(q1.answer_hash, id(&env, 11));
    assert_eq!(q1.updated_at, 10, "unpatched field keeps its timestamp");
    assert_eq!(q2.answer_hash, id(&env, 22));
    assert_eq!(q2.updated_at, 20);
}

#[test]
fn test_autosave_retry_is_idempotent() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    env.ledger().set_timestamp(10);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );
    env.ledger().set_timestamp(20);
    client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 2, 12)],
    );

    // The client never saw the acknowledgement of its second save and
    // replays it with the now-stale base revision. The token match makes it
    // an idempotent success, not a conflict.
    env.ledger().set_timestamp(30);
    let receipt = client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 2, 12)],
    );
    assert!(receipt.replayed);
    assert_eq!(receipt.revision, 2);
    assert_eq!(receipt.saved_at, 20);

    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.revision, 2);
    assert_eq!(draft.last_saved_at, 20);
    assert_eq!(draft.answers.len(), 2);
}

#[test]
fn test_autosave_replay_survives_the_deadline() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 100);
    publish_default_form(&env, &client, &admin, &program_id);

    let applicant = Address::generate(&env);
    env.ledger().set_timestamp(10);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );

    // The client only retries after the intake window closed. The save already
    // happened, so replaying it is an idempotent success, not a deadline error.
    env.ledger().set_timestamp(101);
    let receipt = client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );
    assert!(receipt.replayed);
    assert_eq!(receipt.revision, 1);
    assert_eq!(receipt.saved_at, 10);
}

#[test]
fn test_autosave_rejects_stale_revision() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );
    client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 1, 22)],
    );

    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &1u32,
            &id(&env, 202),
            &vec![&env, patch(&env, 1, 33)]
        ),
        Err(Ok(ContractError::DraftConflict))
    );

    // The newer edit is untouched.
    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.revision, 2);
    assert_eq!(draft.answers.get(0).unwrap().answer_hash, id(&env, 22));
}

#[test]
fn test_autosave_create_and_update_base_revision_rules() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);

    // Updating a draft that does not exist is a conflict, not a create.
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &5u32,
            &id(&env, 200),
            &vec![&env, patch(&env, 1, 11)]
        ),
        Err(Ok(ContractError::DraftConflict))
    );

    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 1, 11)],
    );

    // A second create racing the first must not overwrite it.
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 202),
            &vec![&env, patch(&env, 2, 12)]
        ),
        Err(Ok(ContractError::DraftConflict))
    );
    assert_eq!(client.get_draft(&applicant, &program_id).answers.len(), 1);
}

#[test]
fn test_autosave_rejects_unknown_question_and_duplicate_patches() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);

    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 200),
            &vec![&env, patch(&env, 99, 11)]
        ),
        Err(Ok(ContractError::AnswerNotInForm))
    );

    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 201),
            &vec![&env, patch(&env, 1, 11), patch(&env, 1, 12)]
        ),
        Err(Ok(ContractError::DuplicateAnswer))
    );

    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 202),
            &vec![
                &env,
                AnswerPatch {
                    question_id: 1,
                    answer_hash: Some(id(&env, 0)),
                }
            ]
        ),
        Err(Ok(ContractError::InvalidAnswerHash))
    );
    assert_eq!(
        client.try_get_draft(&applicant, &program_id),
        Err(Ok(ContractError::DraftNotFound))
    );
}

#[test]
fn test_decision_history_is_bounded() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    for i in 0..32 {
        let action = if i % 2 == 0 {
            DecisionAction::RequestInfo
        } else {
            DecisionAction::Review
        };
        client.record_decision(&ctx.coordinator, &app_id, &action, &ctx.accept_input());
    }
    assert_eq!(client.decision_count(&app_id), 32);
    assert_eq!(
        client.try_record_decision(
            &ctx.coordinator,
            &app_id,
            &DecisionAction::RequestInfo,
            &ctx.accept_input()
        ),
        Err(Ok(ApplicationError::DecisionHistoryFull))
fn test_autosave_clears_a_field() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11), patch(&env, 2, 12)],
    );

    client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![
            &env,
            AnswerPatch {
                question_id: 2,
                answer_hash: None,
            },
        ],
    );
    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.answers.len(), 1);
    assert_eq!(draft.answers.get(0).unwrap().question_id, 1);

    // Clearing an already-absent field is a valid, idempotent no-op.
    let receipt = client.autosave_draft(
        &applicant,
        &program_id,
        &2u32,
        &id(&env, 202),
        &vec![
            &env,
            AnswerPatch {
                question_id: 2,
                answer_hash: None,
            },
        ],
    );
    assert_eq!(receipt.revision, 3);
    assert_eq!(client.get_draft(&applicant, &program_id).answers.len(), 1);
}

#[test]
fn test_autosave_rejects_too_many_updates() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);

    let mut updates = Vec::new(&env);
    let mut qid: u32 = 1;
    while qid <= 33 {
        updates.push_back(patch(&env, qid, (qid + 1) as u8));
        qid += 1;
    }
    assert_eq!(
        client.try_autosave_draft(&applicant, &program_id, &0u32, &id(&env, 200), &updates),
        Err(Ok(ContractError::TooManyAnswers))
    );
}

#[test]
fn test_autosave_is_blocked_after_submission_and_removes_the_draft() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );
    client.record_consent(&applicant, &program_id);
    client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 60),
        &true,
    );

    assert_eq!(
        client.try_get_draft(&applicant, &program_id),
        Err(Ok(ContractError::DraftNotFound))
    );
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 201),
            &vec![&env, patch(&env, 2, 12)]
        ),
        Err(Ok(ContractError::DuplicateApplication))
    );
}

#[test]
fn test_internal_reason_is_hidden_from_applicants() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Reject,
        &ctx.reject_input(),
    );

    let view = client.applicant_visible_decision(&app_id, &1u32);
    assert_eq!(view.public_reason, 20);
    assert_eq!(view.resulting_status, ApplicationStatus::Rejected);

    // The internal reasoning commitment and the deciding actor are gated.
    assert_eq!(
        client.try_decision(&stranger, &app_id, &1u32),
        Err(Ok(ApplicationError::Unauthorized))
    );
    let internal = client.decision(&ctx.coordinator, &app_id, &1u32);
    assert_eq!(internal.internal_reason_hash, hash(&ctx.env, 7));
    assert_eq!(internal.actor, ctx.coordinator);
}

#[test]
fn test_unknown_application_and_decision_reads() {
    let ctx = setup();
    let client = ctx.client();
    let missing = hash(&ctx.env, 77);
    assert_eq!(
        client.try_get_application(&missing),
        Err(Ok(ApplicationError::ApplicationNotFound))
    );
    assert_eq!(
        client.try_decision_count(&missing),
        Err(Ok(ApplicationError::ApplicationNotFound))
    );
    assert_eq!(
        client.try_applicant_visible_decision(&missing, &1u32),
        Err(Ok(ApplicationError::ApplicationNotFound))
fn test_autosave_respects_program_window_and_form() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);

    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 100);
    let applicant = Address::generate(&env);

    // No form published yet.
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 200),
            &vec![&env, patch(&env, 1, 11)]
        ),
        Err(Ok(ContractError::NoFormPublished))
    );

    publish_default_form(&env, &client, &admin, &program_id);
    env.ledger().set_timestamp(101);
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 201),
            &vec![&env, patch(&env, 1, 11)]
        ),
        Err(Ok(ContractError::DeadlinePassed))
    );

    client.set_program_active(&admin, &program_id, &false);
    env.ledger().set_timestamp(1);
    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 202),
            &vec![&env, patch(&env, 1, 11)]
        ),
        Err(Ok(ContractError::ProgramInactive))
    );

    assert_eq!(
        client.try_autosave_draft(
            &applicant,
            &program(&env, 9),
            &0u32,
            &id(&env, 203),
            &vec![&env, patch(&env, 1, 11)]
        ),
        Err(Ok(ContractError::ProgramNotFound))
    );
}

#[test]
fn test_draft_stays_bound_to_its_form_version() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );

    client.publish_form(
        &admin,
        &program_id,
        &id(&env, 90),
        &vec![&env, question(&env, 1, QuestionType::Email, true, 1)],
    );
    client.autosave_draft(
        &applicant,
        &program_id,
        &1u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 1, 22)],
    );

    let draft = client.get_draft(&applicant, &program_id);
    assert_eq!(draft.form_version, 1, "drafts keep their accepted version");
    assert_eq!(draft.answers.get(0).unwrap().answer_hash, id(&env, 22));
}

#[test]
fn test_discard_draft_is_revision_checked() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_id,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );

    assert_eq!(
        client.try_discard_draft(&applicant, &program_id, &0u32),
        Err(Ok(ContractError::DraftConflict))
    );
    client.discard_draft(&applicant, &program_id, &1u32);
    assert_eq!(
        client.try_get_draft(&applicant, &program_id),
        Err(Ok(ContractError::DraftNotFound))
    );
    assert_eq!(
        client.try_discard_draft(&applicant, &program_id, &1u32),
        Err(Ok(ContractError::DraftNotFound))
    );
}

#[test]
fn test_upgrade_requires_admin() {
    let ctx = setup();
    let attacker = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_upgrade(&attacker, &hash(&ctx.env, 5)),
        Err(Ok(ApplicationError::Unauthorized))
    );
}

// ===== Withdrawal (MENTOR-036) =====

#[test]
fn test_withdrawal_requires_consequence_acknowledgement() {
    let ctx = setup();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    let input = WithdrawalInput {
        acknowledge_consequences: false,
        ..ctx.withdraw_input()
    };
    assert_eq!(
        ctx.client()
            .try_withdraw_application(&applicant, &app_id, &input),
        Err(Ok(ApplicationError::WithdrawalNotAcknowledged))
    );
}

#[test]
fn test_withdrawal_only_by_applicant() {
    let ctx = setup();
    let applicant = Address::generate(&ctx.env);
    let other = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    assert_eq!(
        ctx.client()
            .try_withdraw_application(&other, &app_id, &ctx.withdraw_input()),
        Err(Ok(ApplicationError::Unauthorized))
    );
}

#[test]
fn test_withdrawal_releases_reservation_and_preserves_history() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::RequestInfo,
        &ctx.accept_input(),
    );
    ctx.env.ledger().set_timestamp(300);
    let seq = client.withdraw_application(&applicant, &app_id, &ctx.withdraw_input());
    assert_eq!(seq, 2);

    let application = client.get_application(&app_id);
    assert_eq!(application.status, ApplicationStatus::Withdrawn);
    assert_eq!(application.decision_count, 2);
    assert_eq!(application.closed_at, 300);
    assert_eq!(client.active_application_count(&ctx.program), 0);

    // Earlier decisions remain readable and immutable.
    let first = client.applicant_visible_decision(&app_id, &1u32);
    assert_eq!(first.action, DecisionAction::RequestInfo);
    let withdrawal = client.applicant_visible_decision(&app_id, &2u32);
    assert_eq!(withdrawal.action, DecisionAction::Withdraw);
    assert_eq!(withdrawal.resulting_status, ApplicationStatus::Withdrawn);
    assert_eq!(withdrawal.public_reason, 30);
}

#[test]
fn test_withdrawal_retry_is_idempotent() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    let first = client.withdraw_application(&applicant, &app_id, &ctx.withdraw_input());
    let second = client.withdraw_application(&applicant, &app_id, &ctx.withdraw_input());
    assert_eq!(first, second);
    assert_eq!(client.decision_count(&app_id), 1);
}

#[test]
fn test_withdrawal_after_final_decision_rejected() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Accept,
        &ctx.accept_input(),
    );
    assert_eq!(
        client.try_withdraw_application(&applicant, &app_id, &ctx.withdraw_input()),
        Err(Ok(ApplicationError::WithdrawalNotAllowed))
fn test_draft_identity_is_per_applicant_and_program() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_a = program(&env, 1);
    let program_b = program(&env, 2);
    add_program(&env, &client, &admin, &program_a, 1_000_000);
    add_program(&env, &client, &admin, &program_b, 1_000_000);

    let applicant = Address::generate(&env);
    client.autosave_draft(
        &applicant,
        &program_a,
        &0u32,
        &id(&env, 200),
        &vec![&env, patch(&env, 1, 11)],
    );
    client.autosave_draft(
        &applicant,
        &program_b,
        &0u32,
        &id(&env, 201),
        &vec![&env, patch(&env, 1, 22)],
    );
    assert_eq!(
        client
            .get_draft(&applicant, &program_a)
            .answers
            .get(0)
            .unwrap()
            .answer_hash,
        id(&env, 11)
    );
    assert_eq!(
        client
            .get_draft(&applicant, &program_b)
            .answers
            .get(0)
            .unwrap()
            .answer_hash,
        id(&env, 22)
    );
}

// ── #1226/MENTOR-030 — atomic submission ──────────────────────────────────

#[test]
fn test_submit_creates_application_and_queue_entry() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    env.ledger().set_timestamp(500);

    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);
    let app_id = client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 60),
        &true,
    );
    assert_eq!(app_id, 1);
    assert!(client.has_applied(&applicant, &program_id));
    assert_eq!(client.get_application_id(&applicant, &program_id), app_id);

    let application = client.get_application(&app_id);
    assert_eq!(application.applicant, applicant);
    assert_eq!(application.program_id, program_id);
    assert_eq!(application.form_version, 1);
    assert_eq!(application.consent_version, 1);
    assert_eq!(application.answers_count, 2);
    assert_eq!(application.data_hash, id(&env, 60));
    assert_eq!(application.submitted_at, 500);

    let review = client.get_review_state(&app_id);
    assert_eq!(review.status, ReviewStatus::Pending);
    assert_eq!(review.priority, Priority::Normal);
    assert_eq!(review.assignee, None);
}

#[test]
fn test_submit_rejects_invalid_answer_sets() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &vec![&env, answer(&env, 2, 12)],
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::MissingRequiredAnswer))
    );
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &vec![&env, answer(&env, 1, 11), answer(&env, 99, 12)],
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::AnswerNotInForm))
    );
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &vec![&env, answer(&env, 1, 11), answer(&env, 1, 12)],
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::DuplicateAnswer))
    );
    assert!(!client.has_applied(&applicant, &program_id));
}

#[test]
fn test_submit_rejects_hidden_answers() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = program(&env, 1);
    boot(&client, &admin);
    register(&client, &admin, &program_id, 1_000_000);
    client.publish_form(
        &admin,
        &program_id,
        &id(&env, 88),
        &conditional_questions(&env),
    );
    publish_terms(&env, &client, &admin, &program_id);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    let answers = vec![
        &env,
        answer(&env, 1, 11),
        answer(&env, 2, 8),
        answer(&env, 3, 13),
    ];
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &answers,
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::AnswerNotVisible))
    );

    // With q2 committing the required value, q3 becomes visible and required.
    let with_q2_matching = vec![&env, answer(&env, 1, 11), answer(&env, 2, 7)];
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &with_q2_matching,
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::MissingRequiredAnswer))
    );

    let fully_answered = vec![
        &env,
        answer(&env, 1, 11),
        answer(&env, 2, 7),
        answer(&env, 3, 13),
    ];
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &fully_answered,
            &id(&env, 60),
            &true
        ),
        Ok(Ok(1u64))
    );
}

#[test]
fn test_submit_requires_affirmative_consent() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &false
        ),
        Err(Ok(ContractError::ConsentRequired))
    );
    assert!(!client.has_applied(&applicant, &program_id));
}

#[test]
fn test_submit_requires_published_terms_and_a_consent_record() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 1_000_000);
    publish_default_form(&env, &client, &admin, &program_id);

    let applicant = Address::generate(&env);
    assert_eq!(
        client.try_record_consent(&applicant, &program_id),
        Err(Ok(ContractError::NoConsentTermsPublished))
    );
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::NoConsentTermsPublished))
    );

    publish_terms(&env, &client, &admin, &program_id);
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::ConsentNotFound))
    );
}

#[test]
fn test_withdrawal_window_closes() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    ctx.set_policy(&ProgramPolicy {
        withdrawal_cutoff: 1_000,
        ..base_policy()
    });
    ctx.env.ledger().set_timestamp(1_000);
    assert_eq!(
        client.try_withdraw_application(&applicant, &app_id, &ctx.withdraw_input()),
        Err(Ok(ApplicationError::WithdrawalWindowClosed))
    );
}

// ===== Reapplication (MENTOR-036) =====

#[test]
fn test_reapply_links_previous_and_records_fresh_consent() {
    let ctx = setup();
    let client = ctx.client();
    let policy = reapply_policy();
    ctx.set_policy(&policy);
    let applicant = Address::generate(&ctx.env);
    ctx.env.ledger().set_timestamp(1_000);
    let first_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());

    ctx.env.ledger().set_timestamp(2_000);
    let second_id = client.reapply(&applicant, &first_id, &2u32);
    assert_ne!(first_id, second_id);

    let second = client.get_application(&second_id);
    assert_eq!(second.attempt, 2);
    assert_eq!(second.status, ApplicationStatus::Submitted);
    assert_eq!(second.previous_application, Some(first_id.clone()));
    assert_eq!(second.consent_version, 2);
    // Fresh consent, not copied from the withdrawn attempt.
    assert_eq!(second.consent_affirmed_at, 2_000);
    assert_eq!(client.active_application_count(&ctx.program), 1);

    // Replaying the reapplication returns the existing attempt.
    let replay = client.reapply(&applicant, &first_id, &2u32);
    assert_eq!(replay, second_id);
    assert_eq!(client.active_application_count(&ctx.program), 1);
}

#[test]
fn test_reapply_requires_policy_permission() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let first_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());
    assert_eq!(
        client.try_reapply(&applicant, &first_id, &1u32),
        Err(Ok(ApplicationError::ReapplicationNotAllowed))
    );
}

#[test]
fn test_reapply_from_open_or_accepted_application_rejected() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&reapply_policy());
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    assert_eq!(
        client.try_reapply(&applicant, &app_id, &1u32),
        Err(Ok(ApplicationError::ReapplicationNotAllowed))
    );

    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Accept,
        &ctx.accept_input(),
    );
    assert_eq!(
        client.try_reapply(&applicant, &app_id, &1u32),
        Err(Ok(ApplicationError::ReapplicationNotAllowed))
    );
}

#[test]
fn test_reapply_by_other_account_rejected() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&reapply_policy());
    let applicant = Address::generate(&ctx.env);
    let other = Address::generate(&ctx.env);
    let first_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());
    assert_eq!(
        client.try_reapply(&other, &first_id, &1u32),
        Err(Ok(ApplicationError::Unauthorized))
fn test_submit_rejects_revoked_consent() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);
    client.revoke_consent(&applicant, &program_id);

    assert!(!client.has_valid_consent(&applicant, &program_id));
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::ConsentRevoked))
    );
}

#[test]
fn test_submit_rejects_out_of_date_consent_until_reconsent() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    client.publish_consent_terms(&admin, &program_id, &id(&env, 78));
    assert!(!client.has_valid_consent(&applicant, &program_id));
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::ConsentOutOfDate))
    );

    assert_eq!(client.record_consent(&applicant, &program_id), 2);
    assert!(client.has_valid_consent(&applicant, &program_id));
    client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 60),
        &true,
    );
    assert_eq!(client.get_application(&1u64).consent_version, 2);
}

#[test]
fn test_submit_rejects_duplicate_and_preserves_the_first() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (applicant, app_id) = apply_for(&env, &client, &program_id);

    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 99),
            &true
        ),
        Err(Ok(ContractError::DuplicateApplication))
    );
    assert_eq!(client.get_application(&app_id).data_hash, id(&env, 60));
    assert_eq!(client.get_application_id(&applicant, &program_id), app_id);
}

#[test]
fn test_different_applicants_and_programs_coexist() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_a = ready_program(&env, &client, &admin);
    let program_b = program(&env, 2);
    add_program(&env, &client, &admin, &program_b, 1_000_000);

    let (_, app_a1) = apply_for(&env, &client, &program_a);
    let (_, app_a2) = apply_for(&env, &client, &program_a);
    let (_, app_b1) = apply_for(&env, &client, &program_b);
    assert_eq!(app_a1, 1);
    assert_eq!(app_a2, 2);
    assert_eq!(app_b1, 3);

    let multi = Address::generate(&env);
    client.record_consent(&multi, &program_a);
    client.record_consent(&multi, &program_b);
    let first = client.submit_application(
        &multi,
        &program_a,
        &1u32,
        &default_answers(&env),
        &id(&env, 61),
        &true,
    );
    let second = client.submit_application(
        &multi,
        &program_b,
        &1u32,
        &default_answers(&env),
        &id(&env, 62),
        &true,
    );
    assert_ne!(first, second);
}

#[test]
fn test_submit_program_window_and_lookup_failures() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    boot(&client, &admin);
    let program_id = program(&env, 1);
    register(&client, &admin, &program_id, 100);
    publish_default_form(&env, &client, &admin, &program_id);
    publish_terms(&env, &client, &admin, &program_id);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    env.ledger().set_timestamp(100);
    client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 60),
        &true,
    );

    let second = Address::generate(&env);
    client.record_consent(&second, &program_id);
    env.ledger().set_timestamp(101);
    assert_eq!(
        client.try_submit_application(
            &second,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::DeadlinePassed))
    );

    client.set_program_active(&admin, &program_id, &false);
    let third = Address::generate(&env);
    client.record_consent(&third, &program_id);
    assert_eq!(
        client.try_submit_application(
            &third,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::ProgramInactive))
    );

    let fourth = Address::generate(&env);
    client.record_consent(&fourth, &program_id);
    env.ledger().set_timestamp(1);
    client.set_program_active(&admin, &program_id, &true);
    assert_eq!(
        client.try_submit_application(
            &fourth,
            &program_id,
            &9u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::FormVersionNotFound))
    );

    assert_eq!(
        client.try_submit_application(
            &fourth,
            &program(&env, 9),
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::ProgramNotFound))
    );
}

#[test]
fn test_failed_submission_leaves_no_partial_state() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    assert!(client
        .try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &Vec::new(&env),
            &id(&env, 60),
            &true
        )
        .is_err());
    assert!(!client.has_applied(&applicant, &program_id));

    // No id was consumed, no queue row was written, and a later valid call
    // still gets the first id.
    let queue = client.list_queue(&admin, &program_id, &empty_filter());
    assert_eq!(queue.len(), 0);
    let app_id = client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 61),
        &true,
    );
    assert_eq!(app_id, 1);
}

#[test]
fn test_submit_rejects_zero_commitments() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 0),
            &true
        ),
        Err(Ok(ContractError::InvalidDataHash))
    );
    assert_eq!(
        client.try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &vec![
                &env,
                Answer {
                    question_id: 1,
                    answer_hash: id(&env, 0)
                }
            ],
            &id(&env, 60),
            &true
        ),
        Err(Ok(ContractError::InvalidAnswerHash))
    );
}

#[test]
fn test_reapply_cooldown_is_enforced() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&ProgramPolicy {
        allow_reapplication: true,
        reapply_cooldown: 100,
        ..base_policy()
    });
    let applicant = Address::generate(&ctx.env);
    ctx.env.ledger().set_timestamp(1_000);
    let first_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());

    ctx.env.ledger().set_timestamp(1_099);
    assert_eq!(
        client.try_reapply(&applicant, &first_id, &1u32),
        Err(Ok(ApplicationError::ReapplicationTooSoon))
    );

    ctx.env.ledger().set_timestamp(1_100);
    let second_id = client.reapply(&applicant, &first_id, &1u32);
    assert_eq!(client.get_application(&second_id).attempt, 2);
}

#[test]
fn test_reapply_attempt_limit_is_enforced() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&ProgramPolicy {
        allow_reapplication: true,
        max_attempts: 1,
        ..base_policy()
    });
    let applicant = Address::generate(&ctx.env);
    let first_id = ctx.open(&applicant);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());
    assert_eq!(
        client.try_reapply(&applicant, &first_id, &1u32),
        Err(Ok(ApplicationError::ReapplicationLimitReached))
fn test_application_keeps_the_accepted_form_version() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);

    client.publish_form(
        &admin,
        &program_id,
        &id(&env, 90),
        &vec![&env, question(&env, 7, QuestionType::Email, true, 1)],
    );
    assert_eq!(client.get_latest_form_version(&program_id), 2);
    assert_eq!(client.get_application(&app_id).form_version, 1);

    // The historical form version is still fully renderable.
    let historical = client.get_form(&program_id, &1);
    assert_eq!(historical.questions.len(), 3);
    assert_eq!(historical.questions.get(0).unwrap().id, 1);
}

#[test]
fn test_submit_emits_its_documented_event_schema() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);
    let app_id = client.submit_application(
        &applicant,
        &program_id,
        &1u32,
        &default_answers(&env),
        &id(&env, 60),
        &true,
    );

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (emitter, topics, data) = events.get(0).unwrap();
    assert_eq!(emitter, contract_id);
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("SUBMIT"));
    let (event_id, event_applicant, event_program): (u64, Address, BytesN<32>) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_id, app_id);
    assert_eq!(event_applicant, applicant);
    assert_eq!(event_program, program_id);
}

// ── #1230/MENTOR-034 — scoped review queues ───────────────────────────────

fn grant(
    client: &MentorshipApplicationsContractClient,
    admin: &Address,
    program_id: &BytesN<32>,
    reviewer: &Address,
    capacity: u32,
) {
    client.grant_reviewer(admin, program_id, reviewer, &capacity);
}

#[test]
fn test_grant_reviewer_validation() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let reviewer = Address::generate(&env);

    assert_eq!(
        client.try_grant_reviewer(&admin, &program(&env, 9), &reviewer, &1u32),
        Err(Ok(ContractError::ProgramNotFound))
    );
    assert_eq!(
        client.try_grant_reviewer(&admin, &program_id, &reviewer, &0u32),
        Err(Ok(ContractError::InvalidCapacity))
    );
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_grant_reviewer(&attacker, &program_id, &reviewer, &1u32),
        Err(Ok(ContractError::NotAdmin))
    );

    grant(&client, &admin, &program_id, &reviewer, 2);
    let profile = client.get_reviewer(&program_id, &reviewer);
    assert!(profile.active);
    assert_eq!(profile.capacity, 2);
    assert_eq!(
        client.try_grant_reviewer(&admin, &program_id, &reviewer, &2u32),
        Err(Ok(ContractError::AlreadyReviewer))
    );
}

#[test]
fn test_revoke_reviewer_stops_work_but_keeps_the_profile() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let reviewer = Address::generate(&env);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    grant(&client, &admin, &program_id, &reviewer, 2);
    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &0u64);

    client.revoke_reviewer(&admin, &program_id, &reviewer);
    assert!(!client.get_reviewer(&program_id, &reviewer).active);
    assert_eq!(
        client.try_record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted),
        Err(Ok(ContractError::NotReviewer))
    );
    assert_eq!(
        client.try_revoke_reviewer(&admin, &program_id, &reviewer),
        Err(Ok(ContractError::NotReviewer))
    );

    // Re-granting is explicit and restores the scope.
    grant(&client, &admin, &program_id, &reviewer, 2);
    assert!(client.get_reviewer(&program_id, &reviewer).active);
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted);
}

#[test]
fn test_reviewer_pool_is_bounded() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);

    let mut i: u32 = 0;
    while i < 32 {
        let reviewer = Address::generate(&env);
        grant(&client, &admin, &program_id, &reviewer, 1);
        i += 1;
    }
    let overflow = Address::generate(&env);
    assert_eq!(
        client.try_grant_reviewer(&admin, &program_id, &overflow, &1u32),
        Err(Ok(ContractError::ReviewerPoolFull))
    );
}

#[test]
fn test_reapply_rejects_stale_or_missing_consent() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&reapply_policy());
    let applicant = Address::generate(&ctx.env);
    let first_id = client.open_application(&applicant, &ctx.program, &3u32);
    client.withdraw_application(&applicant, &first_id, &ctx.withdraw_input());

    assert_eq!(
        client.try_reapply(&applicant, &first_id, &0u32),
        Err(Ok(ApplicationError::InvalidConsent))
    );
    assert_eq!(
        client.try_reapply(&applicant, &first_id, &2u32),
        Err(Ok(ApplicationError::StaleConsent))
    );
    let second_id = client.reapply(&applicant, &first_id, &3u32);
    assert_eq!(client.get_application(&second_id).consent_version, 3);
}

// ===== Reservations and storage bounds =====

#[test]
fn test_capacity_is_enforced_and_released() {
    let ctx = setup();
    let client = ctx.client();
    ctx.set_policy(&ProgramPolicy {
        max_active_applications: 1,
        ..base_policy()
    });
    let first_applicant = Address::generate(&ctx.env);
    let second_applicant = Address::generate(&ctx.env);
    let first_id = ctx.open(&first_applicant);
    assert_eq!(
        client.try_open_application(&second_applicant, &ctx.program, &1u32),
        Err(Ok(ApplicationError::CapacityExceeded))
    );

    client.withdraw_application(&first_applicant, &first_id, &ctx.withdraw_input());
    assert_eq!(client.active_application_count(&ctx.program), 0);
    let second_id = client.open_application(&second_applicant, &ctx.program, &1u32);
    assert_eq!(
        client.get_application(&second_id).status,
        ApplicationStatus::Submitted
fn test_set_priority_scope_and_idempotency() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);

    client.set_priority(&reviewer, &app_id, &Priority::High);
    assert_eq!(client.get_review_state(&app_id).priority, Priority::High);
    assert_eq!(client.get_audit_trail(&app_id).len(), 1);

    // Re-setting the same value is a no-op, not a duplicate audit entry.
    client.set_priority(&reviewer, &app_id, &Priority::High);
    assert_eq!(client.get_audit_trail(&app_id).len(), 1);

    let outsider = Address::generate(&env);
    assert_eq!(
        client.try_set_priority(&outsider, &app_id, &Priority::Urgent),
        Err(Ok(ContractError::NotAuthorized))
    );

    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &0u64);
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted);
    assert_eq!(
        client.try_set_priority(&admin, &app_id, &Priority::Urgent),
        Err(Ok(ContractError::ApplicationAlreadyDecided))
    );
}

#[test]
fn test_assignment_is_concurrency_safe() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    let other = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);
    grant(&client, &admin, &program_id, &other, 1);

    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &100u64);
    let expected = Some(reviewer.clone());
    assert_eq!(
        client.try_assign_reviewer(&admin, &app_id, &other, &none_address(), &200u64),
        Err(Ok(ContractError::AssignmentConflict))
    );
    assert_eq!(
        client.get_review_state(&app_id).assignee,
        Some(reviewer.clone())
    );

    // Explicitly moving from the current assignee succeeds.
    client.assign_reviewer(&admin, &app_id, &other, &expected, &200u64);
    assert_eq!(
        client.get_review_state(&app_id).assignee,
        Some(other.clone())
    );
    assert_eq!(client.get_review_state(&app_id).due_at, 200);
}

#[test]
fn test_assignment_is_idempotent() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);

    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &100u64);
    let expected = Some(reviewer.clone());
    client.assign_reviewer(&admin, &app_id, &reviewer, &expected, &100u64);
    assert_eq!(client.get_audit_trail(&app_id).len(), 1);
}

#[test]
fn test_assignment_enforces_capacity_and_scope() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, first) = apply_for(&env, &client, &program_id);
    let (_, second) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    let other = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);
    grant(&client, &admin, &program_id, &other, 1);

    client.assign_reviewer(&admin, &first, &reviewer, &none_address(), &0u64);
    assert_eq!(
        client.try_assign_reviewer(&admin, &second, &reviewer, &none_address(), &0u64),
        Err(Ok(ContractError::ReviewerAtCapacity))
    );
    client.assign_reviewer(&admin, &second, &other, &none_address(), &0u64);

    let unscoped = Address::generate(&env);
    assert_eq!(
        client.try_assign_reviewer(&admin, &first, &unscoped, &Some(reviewer.clone()), &0u64),
        Err(Ok(ContractError::NotReviewer))
    );

    // A reviewer cannot assign work in a program they are not scoped to.
    let outsider = Address::generate(&env);
    assert_eq!(
        client.try_assign_reviewer(&outsider, &first, &other, &Some(reviewer.clone()), &0u64),
        Err(Ok(ContractError::NotAuthorized))
    );
}

#[test]
fn test_reject_releases_reservation() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);
    let app_id = ctx.open(&applicant);
    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Reject,
        &ctx.reject_input(),
    );
    assert_eq!(client.active_application_count(&ctx.program), 0);
    let other = Address::generate(&ctx.env);
    let _ = ctx.open(&other);
    assert_eq!(client.active_application_count(&ctx.program), 1);
}

/// `Env::events().all()` reports the events of the most recent invocation,
/// so each mutation is asserted to emit exactly one schema-stable event.
fn only_event_topic(env: &Env) -> Symbol {
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_contract, topics, _data) = events.get(0).unwrap();
    assert_eq!(topics.len(), 1);
    Symbol::try_from_val(env, &topics.get(0).unwrap()).unwrap()
}

#[test]
fn test_actions_emit_one_event_each() {
    let ctx = setup();
    let client = ctx.client();
    let applicant = Address::generate(&ctx.env);

    let app_id = ctx.open(&applicant);
    assert_eq!(
        only_event_topic(&ctx.env),
        Symbol::new(&ctx.env, "MNT_OPEN")
    );

    client.record_decision(
        &ctx.coordinator,
        &app_id,
        &DecisionAction::Review,
        &ctx.accept_input(),
    );
    assert_eq!(only_event_topic(&ctx.env), Symbol::new(&ctx.env, "MNT_DEC"));

    client.withdraw_application(&applicant, &app_id, &ctx.withdraw_input());
    assert_eq!(
        only_event_topic(&ctx.env),
        Symbol::new(&ctx.env, "MNT_WDRAW")
    );
}
fn test_conflict_blocks_decisions() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);
    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &0u64);

    client.declare_conflict(&reviewer, &app_id, &true);
    assert!(client.has_conflict(&app_id, &reviewer));
    assert_eq!(
        client.try_record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted),
        Err(Ok(ContractError::ConflictOfInterest))
    );
    assert_eq!(
        client.get_review_state(&app_id).status,
        ReviewStatus::Assigned,
        "a blocked decision must not change status"
    );

    client.declare_conflict(&reviewer, &app_id, &false);
    assert!(!client.has_conflict(&app_id, &reviewer));
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted);
    assert_eq!(
        client.get_review_state(&app_id).outcome,
        DecisionOutcome::Accepted
    );

    let unscoped = Address::generate(&env);
    assert_eq!(
        client.try_declare_conflict(&unscoped, &app_id, &true),
        Err(Ok(ContractError::NotReviewer))
    );
}

#[test]
fn test_queue_events_have_documented_payloads() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);

    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &100u64);
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("ASSIGN"));
    let (event_id, event_reviewer, due_at): (u64, Address, u64) = data.try_into_val(&env).unwrap();
    assert_eq!(event_id, app_id);
    assert_eq!(event_reviewer, reviewer);
    assert_eq!(due_at, 100);

    client.set_priority(&admin, &app_id, &Priority::High);
    let events = env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("PRIO"));
    let (event_id, priority): (u64, Priority) = data.try_into_val(&env).unwrap();
    assert_eq!(event_id, app_id);
    assert_eq!(priority, Priority::High);

    client.declare_conflict(&reviewer, &app_id, &true);
    let events = env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("COI"));
    let (event_id, event_reviewer, declared): (u64, Address, bool) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_id, app_id);
    assert_eq!(event_reviewer, reviewer);
    assert!(declared);

    client.declare_conflict(&reviewer, &app_id, &false);
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted);
    let events = env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("DECIDE"));
    let (event_id, event_reviewer, outcome): (u64, Address, DecisionOutcome) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_id, app_id);
    assert_eq!(event_reviewer, reviewer);
    assert_eq!(outcome, DecisionOutcome::Accepted);
}

#[test]
fn test_decision_requires_the_assignee_and_is_final() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    let other = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);
    grant(&client, &admin, &program_id, &other, 1);

    // Unassigned.
    assert_eq!(
        client.try_record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted),
        Err(Ok(ContractError::NotAssignedReviewer))
    );

    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &0u64);
    assert_eq!(
        client.try_record_decision(&other, &app_id, &DecisionOutcome::Accepted),
        Err(Ok(ContractError::NotAssignedReviewer))
    );

    env.ledger().set_timestamp(42);
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Rejected);
    let review = client.get_review_state(&app_id);
    assert_eq!(review.status, ReviewStatus::Decided);
    assert_eq!(review.outcome, DecisionOutcome::Rejected);
    assert_eq!(review.decided_by, Some(reviewer.clone()));
    assert_eq!(review.decided_at, 42);

    assert_eq!(
        client.try_record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted),
        Err(Ok(ContractError::ApplicationAlreadyDecided))
    );
    assert_eq!(
        client.try_assign_reviewer(&admin, &app_id, &other, &Some(reviewer.clone()), &0u64),
        Err(Ok(ContractError::ApplicationAlreadyDecided))
    );
}

#[test]
fn test_queue_is_scoped_and_filters_are_stable() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_a = ready_program(&env, &client, &admin);
    let program_b = program(&env, 2);
    add_program(&env, &client, &admin, &program_b, 1_000_000);

    let (applicant_a1, app_a1) = apply_for(&env, &client, &program_a);
    let (_, app_a2) = apply_for(&env, &client, &program_a);
    let (_, app_a3) = apply_for(&env, &client, &program_a);
    let (_, app_b) = apply_for(&env, &client, &program_b);

    let reviewer = Address::generate(&env);
    let second = Address::generate(&env);
    grant(&client, &admin, &program_a, &reviewer, 3);
    grant(&client, &admin, &program_a, &second, 3);

    client.assign_reviewer(&admin, &app_a1, &reviewer, &none_address(), &100u64);
    client.set_priority(&admin, &app_a1, &Priority::Urgent);
    client.assign_reviewer(&admin, &app_a3, &second, &none_address(), &50u64);
    client.set_priority(&admin, &app_a2, &Priority::High);

    let queue = client.list_queue(&reviewer, &program_a, &empty_filter());
    assert_eq!(queue.len(), 3, "only program A applications appear");
    assert_eq!(queue.get(0).unwrap().application_id, app_a1);
    assert_eq!(queue.get(1).unwrap().application_id, app_a2);
    assert_eq!(queue.get(2).unwrap().application_id, app_a3);
    assert_eq!(queue.get(0).unwrap().applicant, applicant_a1);
    assert_eq!(queue.get(0).unwrap().status, ReviewStatus::Assigned);
    assert!(!queue.get(0).unwrap().overdue);

    // Repeated reads are identical.
    let again = client.list_queue(&reviewer, &program_a, &empty_filter());
    let mut i: u32 = 0;
    while i < queue.len() {
        assert_eq!(
            queue.get(i).unwrap().application_id,
            again.get(i).unwrap().application_id
        );
        i += 1;
    }

    // Other program / unscoped caller are denied.
    assert_eq!(
        client.try_list_queue(&reviewer, &program_b, &empty_filter()),
        Err(Ok(ContractError::NotAuthorized))
    );
    let outsider = Address::generate(&env);
    assert_eq!(
        client.try_list_queue(&outsider, &program_a, &empty_filter()),
        Err(Ok(ContractError::NotAuthorized))
    );

    // Admin may read either queue, and B's queue only holds B's application.
    let queue_b = client.list_queue(&admin, &program_b, &empty_filter());
    assert_eq!(queue_b.len(), 1);
    assert_eq!(queue_b.get(0).unwrap().application_id, app_b);

    // Filters.
    let mut pending = empty_filter();
    pending.status = QueueStatusFilter::Pending;
    let pending_rows = client.list_queue(&admin, &program_a, &pending);
    assert_eq!(pending_rows.len(), 1);
    assert_eq!(pending_rows.get(0).unwrap().application_id, app_a2);

    let mut assigned_to_reviewer = empty_filter();
    assigned_to_reviewer.assignee = Some(reviewer.clone());
    let assigned_rows = client.list_queue(&admin, &program_a, &assigned_to_reviewer);
    assert_eq!(assigned_rows.len(), 1);
    assert_eq!(assigned_rows.get(0).unwrap().application_id, app_a1);

    let mut unassigned = empty_filter();
    unassigned.unassigned_only = true;
    let unassigned_rows = client.list_queue(&admin, &program_a, &unassigned);
    assert_eq!(unassigned_rows.len(), 1);
    assert_eq!(unassigned_rows.get(0).unwrap().application_id, app_a2);

    let mut due_soon = empty_filter();
    due_soon.due_before = Some(60);
    let due_rows = client.list_queue(&admin, &program_a, &due_soon);
    assert_eq!(due_rows.len(), 1);
    assert_eq!(due_rows.get(0).unwrap().application_id, app_a3);

    let mut high = empty_filter();
    high.priority = QueuePriorityFilter::High;
    let high_rows = client.list_queue(&admin, &program_a, &high);
    assert_eq!(high_rows.len(), 1);
    assert_eq!(high_rows.get(0).unwrap().application_id, app_a2);

    let mut first_two = empty_filter();
    first_two.limit = 2;
    assert_eq!(client.list_queue(&admin, &program_a, &first_two).len(), 2);
}

#[test]
fn test_queue_rejects_invalid_filters() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);

    let mut zero = empty_filter();
    zero.limit = 0;
    assert_eq!(
        client.try_list_queue(&admin, &program_id, &zero),
        Err(Ok(ContractError::InvalidFilter))
    );

    let mut too_large = empty_filter();
    too_large.limit = 65;
    assert_eq!(
        client.try_list_queue(&admin, &program_id, &too_large),
        Err(Ok(ContractError::InvalidFilter))
    );

    let mut contradictory = empty_filter();
    contradictory.unassigned_only = true;
    contradictory.assignee = Some(Address::generate(&env));
    assert_eq!(
        client.try_list_queue(&admin, &program_id, &contradictory),
        Err(Ok(ContractError::InvalidFilter))
    );
}

#[test]
fn test_overdue_and_workload_indicators() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, first) = apply_for(&env, &client, &program_id);
    let (_, second) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 3);

    env.ledger().set_timestamp(100);
    client.assign_reviewer(&admin, &first, &reviewer, &none_address(), &150u64);
    client.assign_reviewer(&admin, &second, &reviewer, &none_address(), &250u64);

    let workload = client.reviewer_workload(&program_id, &reviewer);
    assert_eq!(workload.capacity, 3);
    assert_eq!(workload.assigned_active, 2);
    assert_eq!(workload.decided, 0);
    assert_eq!(workload.overdue, 0);

    client.record_decision(&reviewer, &first, &DecisionOutcome::Accepted);
    let workload = client.reviewer_workload(&program_id, &reviewer);
    assert_eq!(workload.assigned_active, 1);
    assert_eq!(workload.decided, 1);

    env.ledger().set_timestamp(300);
    let workload = client.reviewer_workload(&program_id, &reviewer);
    assert_eq!(workload.assigned_active, 1);
    assert_eq!(workload.overdue, 1);

    let mut still_assigned = empty_filter();
    still_assigned.status = QueueStatusFilter::Assigned;
    let queue = client.list_queue(&admin, &program_id, &still_assigned);
    assert_eq!(queue.len(), 1);
    assert!(queue.get(0).unwrap().overdue);
}

#[test]
fn test_queue_actions_are_audited_in_order() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);
    let reviewer = Address::generate(&env);
    grant(&client, &admin, &program_id, &reviewer, 1);

    env.ledger().set_timestamp(10);
    client.assign_reviewer(&admin, &app_id, &reviewer, &none_address(), &100u64);
    env.ledger().set_timestamp(20);
    client.set_priority(&admin, &app_id, &Priority::Urgent);
    env.ledger().set_timestamp(30);
    client.declare_conflict(&reviewer, &app_id, &true);
    client.declare_conflict(&reviewer, &app_id, &false);
    env.ledger().set_timestamp(40);
    client.record_decision(&reviewer, &app_id, &DecisionOutcome::Accepted);

    let trail: Vec<AuditEntry> = client.get_audit_trail(&app_id);
    assert_eq!(trail.len(), 5);
    assert_eq!(trail.get(0).unwrap().action, QueueAction::Assigned);
    assert_eq!(trail.get(0).unwrap().at, 10);
    assert_eq!(trail.get(1).unwrap().action, QueueAction::PriorityChanged);
    assert_eq!(trail.get(2).unwrap().action, QueueAction::ConflictDeclared);
    assert_eq!(trail.get(3).unwrap().action, QueueAction::ConflictCleared);
    assert_eq!(trail.get(4).unwrap().action, QueueAction::DecisionRecorded);
    assert_eq!(trail.get(2).unwrap().actor, reviewer);
    assert_eq!(trail.get(4).unwrap().at, 40);

    assert_eq!(
        client.try_get_audit_trail(&999u64),
        Err(Ok(ContractError::ApplicationNotFound))
    );
}

#[test]
fn test_audit_trail_is_bounded_without_partial_writes() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let (_, app_id) = apply_for(&env, &client, &program_id);

    let mut i: u32 = 0;
    while i < 64 {
        let priority = if i % 2 == 0 {
            Priority::Low
        } else {
            Priority::Normal
        };
        client.set_priority(&admin, &app_id, &priority);
        i += 1;
    }
    assert_eq!(client.get_audit_trail(&app_id).len(), 64);
    let before = client.get_review_state(&app_id).priority;
    assert_eq!(before, Priority::Normal);

    assert_eq!(
        client.try_set_priority(&admin, &app_id, &Priority::Low),
        Err(Ok(ContractError::AuditTrailFull))
    );
    assert_eq!(
        client.get_review_state(&app_id).priority,
        Priority::Normal,
        "a failed audit must not leave the priority changed"
    );
}

// ── Authorization ─────────────────────────────────────────────────────────

#[test]
fn test_unauthenticated_calls_are_rejected() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);

    env.set_auths(&[]);
    assert!(client
        .try_submit_application(
            &applicant,
            &program_id,
            &1u32,
            &default_answers(&env),
            &id(&env, 60),
            &true
        )
        .is_err());
    assert!(!client.has_applied(&applicant, &program_id));

    assert!(client
        .try_autosave_draft(
            &applicant,
            &program_id,
            &0u32,
            &id(&env, 200),
            &vec![&env, patch(&env, 1, 11)]
        )
        .is_err());
}

#[test]
fn test_submit_auth_is_bound_to_the_applicant() {
    let (env, contract_id, admin) = setup();
    let client = client(&env, &contract_id);
    let program_id = ready_program(&env, &client, &admin);
    let applicant = Address::generate(&env);
    client.record_consent(&applicant, &program_id);
    let answers = default_answers(&env);
    let data_hash = id(&env, 60);

    env.mock_auths(&[MockAuth {
        address: &applicant,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "submit_application",
            args: (
                applicant.clone(),
                program_id.clone(),
                1u32,
                answers.clone(),
                data_hash.clone(),
                true,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.submit_application(&applicant, &program_id, &1u32, &answers, &data_hash, &true);
    let auths = env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, applicant);
}
