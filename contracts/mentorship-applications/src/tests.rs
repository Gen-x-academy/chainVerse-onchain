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
