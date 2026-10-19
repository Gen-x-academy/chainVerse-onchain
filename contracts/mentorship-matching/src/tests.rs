use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::{
    CoordinatorActionKind, MatchingError, MentorshipMatching, MentorshipMatchingClient,
    ParticipantRole, ParticipantStatus, ProposalOrigin, ProposalStatus, ProposalTerms,
};

fn hash(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

struct Ctx {
    env: Env,
    contract_id: Address,
    admin: Address,
    coordinator: Address,
    program: BytesN<32>,
    mentor: Address,
    mentee: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(MentorshipMatching, ());
    let client = MentorshipMatchingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let coordinator = Address::generate(&env);
    let mentor = Address::generate(&env);
    let mentee = Address::generate(&env);
    client.initialize(&admin);
    client.set_coordinator(&admin, &coordinator, &true);
    client.register_participant(&admin, &mentor, &ParticipantRole::Mentor);
    client.register_participant(&admin, &mentee, &ParticipantRole::Mentee);
    let program = hash(&env, 3);
    Ctx {
        env,
        contract_id,
        admin,
        coordinator,
        program,
        mentor,
        mentee,
    }
}

impl Ctx {
    fn client(&self) -> MentorshipMatchingClient<'_> {
        MentorshipMatchingClient::new(&self.env, &self.contract_id)
    }

    fn terms(&self) -> ProposalTerms {
        ProposalTerms {
            terms_hash: hash(&self.env, 11),
            proposed_fee: 1_000,
            expires_at: self.env.ledger().timestamp() + 1_000,
        }
    }

    fn propose(&self) -> BytesN<32> {
        self.client()
            .create_proposal(&self.mentor, &self.program, &self.mentee, &self.terms())
    }

    fn add_mentee(&self) -> Address {
        let mentee = Address::generate(&self.env);
        self.client()
            .register_participant(&self.admin, &mentee, &ParticipantRole::Mentee);
        mentee
    }
}

fn only_event_topic(env: &Env) -> Symbol {
    let topics = event_topics(env);
    assert_eq!(topics.len(), 1);
    topics.get(0).unwrap()
}

fn event_topics(env: &Env) -> soroban_sdk::Vec<Symbol> {
    let events = env.events().all();
    let mut topics = soroban_sdk::Vec::new(env);
    for i in 0..events.len() {
        let (_contract, event_topics, _data) = events.get(i).unwrap();
        topics.push_back(Symbol::try_from_val(env, &event_topics.get(0).unwrap()).unwrap());
    }
    topics
}

// ===== Bootstrap and participant registry =====

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    let other = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_initialize(&other),
        Err(Ok(MatchingError::AlreadyInitialized))
    );
}

#[test]
fn test_register_participant_requires_admin() {
    let ctx = setup();
    let newcomer = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_register_participant(
            &ctx.coordinator,
            &newcomer,
            &ParticipantRole::Mentee
        ),
        Err(Ok(MatchingError::Unauthorized))
    );
}

#[test]
fn test_register_participant_twice_rejected() {
    let ctx = setup();
    assert_eq!(
        ctx.client()
            .try_register_participant(&ctx.admin, &ctx.mentor, &ParticipantRole::Mentor),
        Err(Ok(MatchingError::ParticipantExists))
    );
    let profile = ctx.client().get_participant(&ctx.mentor);
    assert_eq!(profile.role, ParticipantRole::Mentor);
    assert_eq!(profile.status, ParticipantStatus::Active);
}

#[test]
fn test_set_participant_status_requires_admin() {
    let ctx = setup();
    assert_eq!(
        ctx.client().try_set_participant_status(
            &ctx.coordinator,
            &ctx.mentee,
            &ParticipantStatus::Suspended
        ),
        Err(Ok(MatchingError::Unauthorized))
    );
    let unknown = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client()
            .try_set_participant_status(&ctx.admin, &unknown, &ParticipantStatus::Inactive),
        Err(Ok(MatchingError::ParticipantNotFound))
    );
}

// ===== Proposal creation (MENTOR-037) =====

#[test]
fn test_create_proposal_records_terms_and_pending_slot() {
    let ctx = setup();
    let client = ctx.client();
    ctx.env.ledger().set_timestamp(500);
    let id = ctx.propose();

    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.program_id, ctx.program);
    assert_eq!(proposal.mentor, ctx.mentor);
    assert_eq!(proposal.mentee, ctx.mentee);
    assert_eq!(proposal.origin, ProposalOrigin::Mentor);
    assert_eq!(proposal.created_by, ctx.mentor);
    assert_eq!(proposal.last_adjusted_by, None);
    assert_eq!(proposal.status, ProposalStatus::Proposed);
    assert_eq!(proposal.version, 1);
    assert!(!proposal.mentor_approved);
    assert!(!proposal.mentee_approved);
    assert_eq!(proposal.created_at, 500);
    assert_eq!(client.pending_proposal_count(&ctx.program), 1);
    assert_eq!(
        client.open_proposal_for(&ctx.program, &ctx.mentor, &ctx.mentee),
        Some(id.clone())
    );
    assert!(client.is_proposal_open(&id));
}

#[test]
fn test_create_proposal_rejects_invalid_inputs() {
    let ctx = setup();
    let client = ctx.client();
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentor, &ctx.terms()),
        Err(Ok(MatchingError::SelfMatch))
    );
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &zero(&ctx.env), &ctx.mentee, &ctx.terms()),
        Err(Ok(MatchingError::InvalidProgram))
    );
    let zero_terms = ProposalTerms {
        terms_hash: zero(&ctx.env),
        ..ctx.terms()
    };
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &zero_terms),
        Err(Ok(MatchingError::InvalidTerms))
    );
    let negative_fee = ProposalTerms {
        proposed_fee: -1,
        ..ctx.terms()
    };
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &negative_fee),
        Err(Ok(MatchingError::InvalidTerms))
    );
    let past_expiry = ProposalTerms {
        expires_at: 0,
        ..ctx.terms()
    };
    ctx.env.ledger().set_timestamp(1);
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &past_expiry),
        Err(Ok(MatchingError::InvalidExpiry))
    );
    let too_long = ProposalTerms {
        expires_at: 90 * 24 * 60 * 60 + 2,
        ..ctx.terms()
    };
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &too_long),
        Err(Ok(MatchingError::InvalidExpiry))
    );
}

#[test]
fn test_create_proposal_enforces_hard_eligibility() {
    let ctx = setup();
    let client = ctx.client();
    let mentor_as_mentee = Address::generate(&ctx.env);
    client.register_participant(&ctx.admin, &mentor_as_mentee, &ParticipantRole::Mentor);
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &mentor_as_mentee, &ctx.terms()),
        Err(Ok(MatchingError::RoleMismatch))
    );

    client.set_participant_status(&ctx.admin, &ctx.mentee, &ParticipantStatus::Suspended);
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &ctx.terms()),
        Err(Ok(MatchingError::ParticipantInactive))
    );

    let unregistered = Address::generate(&ctx.env);
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &unregistered, &ctx.terms()),
        Err(Ok(MatchingError::ParticipantNotFound))
    );
}

#[test]
fn test_duplicate_active_proposal_is_prevented() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &ctx.terms()),
        Err(Ok(MatchingError::DuplicateActiveProposal))
    );
    // A different program is a different pair scope.
    let other_program = hash(&ctx.env, 4);
    let other_id = client.create_proposal(&ctx.mentor, &other_program, &ctx.mentee, &ctx.terms());
    assert_ne!(id, other_id);
}

#[test]
fn test_duplicate_allowed_after_decline() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.decline_proposal(&ctx.mentee, &id, &9u32);
    let replacement = client.create_proposal(
        &ctx.mentor,
        &ctx.program,
        &ctx.mentee,
        &ProposalTerms {
            terms_hash: hash(&ctx.env, 12),
            ..ctx.terms()
        },
    );
    assert_ne!(id, replacement);
    assert_eq!(
        client.get_proposal(&replacement).status,
        ProposalStatus::Proposed
    );
}

#[test]
fn test_expired_proposal_can_be_replaced() {
    let ctx = setup();
    let client = ctx.client();
    ctx.env.ledger().set_timestamp(100);
    let first = ctx.propose();
    ctx.env.ledger().set_timestamp(1_100);
    assert!(!client.is_proposal_open(&first));
    let second = ctx.propose();
    assert_ne!(first, second);
    assert_eq!(client.get_proposal(&first).status, ProposalStatus::Expired);
    // The expired proposal released its pending slot; only the new one counts.
    assert_eq!(client.pending_proposal_count(&ctx.program), 1);
}

#[test]
fn test_creation_is_auditable_via_event() {
    let ctx = setup();
    let _ = ctx.propose();
    assert_eq!(
        only_event_topic(&ctx.env),
        Symbol::new(&ctx.env, "MNT_PROP")
    );
}

// ===== Blocks =====

#[test]
fn test_block_is_enforced_symmetrically() {
    let ctx = setup();
    let client = ctx.client();
    client.set_block(&ctx.mentee, &ctx.mentor, &true);
    assert!(client.is_blocked(&ctx.mentor, &ctx.mentee));
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &ctx.mentee, &ctx.terms()),
        Err(Ok(MatchingError::Blocked))
    );
    client.set_block(&ctx.mentee, &ctx.mentor, &false);
    assert!(!client.is_blocked(&ctx.mentor, &ctx.mentee));
    let _ = ctx.propose();
}

#[test]
fn test_block_after_proposal_blocks_approval() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.set_block(&ctx.mentee, &ctx.mentor, &true);
    assert_eq!(
        client.try_approve_proposal(&ctx.mentee, &id, &1u32),
        Err(Ok(MatchingError::Blocked))
    );
}

#[test]
fn test_self_block_rejected() {
    let ctx = setup();
    assert_eq!(
        ctx.client().try_set_block(&ctx.mentor, &ctx.mentor, &true),
        Err(Ok(MatchingError::SelfMatch))
    );
}

// ===== Mutual approval =====

#[test]
fn test_both_approvals_activate_the_match() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.approve_proposal(&ctx.mentor, &id, &1u32);
    assert_eq!(
        only_event_topic(&ctx.env),
        Symbol::new(&ctx.env, "MNT_APPR")
    );
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Proposed);

    client.approve_proposal(&ctx.mentee, &id, &1u32);
    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.status, ProposalStatus::Active);
    assert!(proposal.mentor_approved);
    assert!(proposal.mentee_approved);
    assert_eq!(client.pending_proposal_count(&ctx.program), 0);
}

#[test]
fn test_approve_requires_participant_and_one_time_consent() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        client.try_approve_proposal(&outsider, &id, &1u32),
        Err(Ok(MatchingError::NotParticipant))
    );
    client.approve_proposal(&ctx.mentor, &id, &1u32);
    assert_eq!(
        client.try_approve_proposal(&ctx.mentor, &id, &1u32),
        Err(Ok(MatchingError::AlreadyApproved))
    );
}

#[test]
fn test_stale_version_approval_rejected() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    assert_eq!(
        client.try_approve_proposal(&ctx.mentee, &id, &2u32),
        Err(Ok(MatchingError::StaleProposalVersion))
    );
}

#[test]
fn test_expired_proposal_cannot_be_approved() {
    let ctx = setup();
    let client = ctx.client();
    ctx.env.ledger().set_timestamp(100);
    let id = ctx.propose();
    ctx.env.ledger().set_timestamp(1_100);
    assert_eq!(
        client.try_approve_proposal(&ctx.mentor, &id, &1u32),
        Err(Ok(MatchingError::ProposalExpired))
    );
    // The failing call reverts, so expiry is only finalized by the next
    // successful action on the pair; the proposal is already unusable.
    assert_eq!(
        client.open_proposal_for(&ctx.program, &ctx.mentor, &ctx.mentee),
        None
    );
    let replacement = ctx.propose();
    assert_ne!(replacement, id);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Expired);
    assert_eq!(client.pending_proposal_count(&ctx.program), 1);
}

#[test]
fn test_suspension_after_proposal_blocks_approval() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.set_participant_status(&ctx.admin, &ctx.mentee, &ParticipantStatus::Inactive);
    assert_eq!(
        client.try_approve_proposal(&ctx.mentee, &id, &1u32),
        Err(Ok(MatchingError::ParticipantInactive))
    );
}

// ===== Declining =====

#[test]
fn test_decline_requires_party_and_reason() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        client.try_decline_proposal(&outsider, &id, &1u32),
        Err(Ok(MatchingError::NotParticipant))
    );
    assert_eq!(
        client.try_decline_proposal(&ctx.mentee, &id, &0u32),
        Err(Ok(MatchingError::InvalidReasonCode))
    );
}

#[test]
fn test_decline_is_terminal_and_idempotent() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.decline_proposal(&ctx.mentee, &id, &7u32);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Declined);
    assert_eq!(client.pending_proposal_count(&ctx.program), 0);
    // Retried decline is a safe no-op that emits nothing.
    client.decline_proposal(&ctx.mentee, &id, &7u32);
    assert_eq!(ctx.env.events().all().len(), 0);
    assert_eq!(
        client.try_approve_proposal(&ctx.mentor, &id, &1u32),
        Err(Ok(MatchingError::InvalidTransition))
    );
}

// ===== Coordinator-assisted matching (MENTOR-039) =====

#[test]
fn test_coordinator_proposal_requires_authority_and_reason() {
    let ctx = setup();
    let client = ctx.client();
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        client.try_propose_as_coordinator(
            &outsider,
            &ctx.program,
            &ctx.mentor,
            &ctx.mentee,
            &ctx.terms(),
            &hash(&ctx.env, 21)
        ),
        Err(Ok(MatchingError::Unauthorized))
    );
    assert_eq!(
        client.try_propose_as_coordinator(
            &ctx.coordinator,
            &ctx.program,
            &ctx.mentor,
            &ctx.mentee,
            &ctx.terms(),
            &zero(&ctx.env)
        ),
        Err(Ok(MatchingError::ReasonRequired))
    );
}

#[test]
fn test_coordinator_proposal_is_logged_for_reporting() {
    let ctx = setup();
    let client = ctx.client();
    let id = client.propose_as_coordinator(
        &ctx.coordinator,
        &ctx.program,
        &ctx.mentor,
        &ctx.mentee,
        &ctx.terms(),
        &hash(&ctx.env, 21),
    );
    assert_eq!(
        event_topics(&ctx.env),
        soroban_sdk::vec![
            &ctx.env,
            Symbol::new(&ctx.env, "MNT_PROP"),
            Symbol::new(&ctx.env, "MNT_CACT")
        ]
    );

    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.origin, ProposalOrigin::Coordinator);
    assert_eq!(proposal.created_by, ctx.coordinator);
    assert_eq!(client.proposal_reason(&id), hash(&ctx.env, 21));

    assert_eq!(client.coordinator_action_count(&ctx.coordinator), 1);
    let action = client.coordinator_action(&ctx.coordinator, &1u32);
    assert_eq!(action.kind, CoordinatorActionKind::Proposed);
    assert_eq!(action.proposal_id, id);
    assert_eq!(action.reason_hash, hash(&ctx.env, 21));
    assert_eq!(action.version, 1);
}

#[test]
fn test_coordinator_cannot_bypass_eligibility_or_blocks() {
    let ctx = setup();
    let client = ctx.client();
    client.set_block(&ctx.mentor, &ctx.mentee, &true);
    assert_eq!(
        client.try_propose_as_coordinator(
            &ctx.coordinator,
            &ctx.program,
            &ctx.mentor,
            &ctx.mentee,
            &ctx.terms(),
            &hash(&ctx.env, 22)
        ),
        Err(Ok(MatchingError::Blocked))
    );

    client.set_block(&ctx.mentor, &ctx.mentee, &false);
    client.set_participant_status(&ctx.admin, &ctx.mentee, &ParticipantStatus::Suspended);
    assert_eq!(
        client.try_propose_as_coordinator(
            &ctx.coordinator,
            &ctx.program,
            &ctx.mentor,
            &ctx.mentee,
            &ctx.terms(),
            &hash(&ctx.env, 22)
        ),
        Err(Ok(MatchingError::ParticipantInactive))
    );
}

#[test]
fn test_adjust_requires_coordinator_and_reason() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    let outsider = Address::generate(&ctx.env);
    assert_eq!(
        client.try_adjust_proposal(&outsider, &id, &ctx.terms(), &hash(&ctx.env, 31)),
        Err(Ok(MatchingError::Unauthorized))
    );
    assert_eq!(
        client.try_adjust_proposal(&ctx.coordinator, &id, &ctx.terms(), &zero(&ctx.env)),
        Err(Ok(MatchingError::ReasonRequired))
    );
}

#[test]
fn test_adjustment_requires_fresh_participant_approval() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.approve_proposal(&ctx.mentor, &id, &1u32);

    let adjusted = client.adjust_proposal(
        &ctx.coordinator,
        &id,
        &ProposalTerms {
            terms_hash: hash(&ctx.env, 41),
            proposed_fee: 2_000,
            ..ctx.terms()
        },
        &hash(&ctx.env, 42),
    );
    assert_eq!(adjusted, id);
    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.version, 2);
    assert_eq!(proposal.status, ProposalStatus::Proposed);
    assert_eq!(proposal.proposed_fee, 2_000);
    assert_eq!(proposal.terms_hash, hash(&ctx.env, 41));
    assert_eq!(proposal.last_adjusted_by, Some(ctx.coordinator.clone()));
    assert!(!proposal.mentor_approved);
    assert!(!proposal.mentee_approved);

    // The mentor's earlier approval does not carry over: the stale version is
    // rejected, the current version must be approved again.
    assert_eq!(
        client.try_approve_proposal(&ctx.mentor, &id, &1u32),
        Err(Ok(MatchingError::StaleProposalVersion))
    );
    client.approve_proposal(&ctx.mentor, &id, &2u32);
    client.approve_proposal(&ctx.mentee, &id, &2u32);
    assert_eq!(client.get_proposal(&id).status, ProposalStatus::Active);

    assert_eq!(client.coordinator_action_count(&ctx.coordinator), 1);
    let action = client.coordinator_action(&ctx.coordinator, &1u32);
    assert_eq!(action.kind, CoordinatorActionKind::Adjusted);
    assert_eq!(action.version, 2);
    assert_eq!(client.proposal_reason(&id), hash(&ctx.env, 42));
}

#[test]
fn test_adjusting_active_match_reopens_consent() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.approve_proposal(&ctx.mentor, &id, &1u32);
    client.approve_proposal(&ctx.mentee, &id, &1u32);
    assert_eq!(client.pending_proposal_count(&ctx.program), 0);

    client.adjust_proposal(&ctx.coordinator, &id, &ctx.terms(), &hash(&ctx.env, 43));
    let proposal = client.get_proposal(&id);
    assert_eq!(proposal.status, ProposalStatus::Proposed);
    assert_eq!(proposal.version, 2);
    // Back to awaiting approval, so it consumes a pending slot again.
    assert_eq!(client.pending_proposal_count(&ctx.program), 1);
}

#[test]
fn test_adjustment_cannot_bypass_blocks_or_suspension() {
    let ctx = setup();
    let client = ctx.client();
    let id = ctx.propose();
    client.set_block(&ctx.mentee, &ctx.mentor, &true);
    assert_eq!(
        client.try_adjust_proposal(&ctx.coordinator, &id, &ctx.terms(), &hash(&ctx.env, 44)),
        Err(Ok(MatchingError::Blocked))
    );

    client.set_block(&ctx.mentee, &ctx.mentor, &false);
    client.set_participant_status(&ctx.admin, &ctx.mentor, &ParticipantStatus::Inactive);
    assert_eq!(
        client.try_adjust_proposal(&ctx.coordinator, &id, &ctx.terms(), &hash(&ctx.env, 44)),
        Err(Ok(MatchingError::ParticipantInactive))
    );
}

#[test]
fn test_adjustment_of_declined_or_expired_proposal_rejected() {
    let ctx = setup();
    let client = ctx.client();
    ctx.env.ledger().set_timestamp(100);
    let declined = ctx.propose();
    client.decline_proposal(&ctx.mentee, &declined, &5u32);
    assert_eq!(
        client.try_adjust_proposal(
            &ctx.coordinator,
            &declined,
            &ctx.terms(),
            &hash(&ctx.env, 45)
        ),
        Err(Ok(MatchingError::InvalidTransition))
    );

    let expiring =
        client.create_proposal(&ctx.mentor, &hash(&ctx.env, 6), &ctx.mentee, &ctx.terms());
    ctx.env.ledger().set_timestamp(2_000);
    assert_eq!(
        client.try_adjust_proposal(
            &ctx.coordinator,
            &expiring,
            &ctx.terms(),
            &hash(&ctx.env, 45)
        ),
        Err(Ok(MatchingError::ProposalExpired))
    );
}

// ===== Storage bounds, reads and upgrade =====

#[test]
fn test_pending_proposal_capacity_is_bounded() {
    let ctx = setup();
    let client = ctx.client();
    // The contract caps pending proposals per program at 128; drive it to the
    // bound with distinct mentees so each pair is unique.
    for _ in 0..128 {
        let mentee = ctx.add_mentee();
        client.create_proposal(&ctx.mentor, &ctx.program, &mentee, &ctx.terms());
    }
    assert_eq!(client.pending_proposal_count(&ctx.program), 128);
    let extra = ctx.add_mentee();
    assert_eq!(
        client.try_create_proposal(&ctx.mentor, &ctx.program, &extra, &ctx.terms()),
        Err(Ok(MatchingError::ProgramCapacityReached))
    );
}

#[test]
fn test_unknown_reads_and_action_errors() {
    let ctx = setup();
    let client = ctx.client();
    let missing = hash(&ctx.env, 99);
    assert_eq!(
        client.try_get_proposal(&missing),
        Err(Ok(MatchingError::ProposalNotFound))
    );
    assert_eq!(
        client.try_is_proposal_open(&missing),
        Err(Ok(MatchingError::ProposalNotFound))
    );
    assert_eq!(
        client.try_proposal_reason(&missing),
        Err(Ok(MatchingError::ReasonNotFound))
    );
    assert_eq!(
        client.try_coordinator_action(&ctx.coordinator, &1u32),
        Err(Ok(MatchingError::ActionNotFound))
    );
    assert_eq!(client.coordinator_action_count(&ctx.coordinator), 0);
}

#[test]
fn test_upgrade_requires_admin() {
    let ctx = setup();
    let attacker = Address::generate(&ctx.env);
    assert_eq!(
        ctx.client().try_upgrade(&attacker, &hash(&ctx.env, 5)),
        Err(Ok(MatchingError::Unauthorized))
    );
}

#[test]
fn test_coordinator_identity_is_recorded_on_adjustment() {
    let ctx = setup();
    let client = ctx.client();
    let second = Address::generate(&ctx.env);
    client.set_coordinator(&ctx.admin, &second, &true);
    let id = ctx.propose();
    client.adjust_proposal(&second, &id, &ctx.terms(), &hash(&ctx.env, 51));
    assert_eq!(
        client.get_proposal(&id).last_adjusted_by,
        Some(second.clone())
    );
    assert_eq!(client.coordinator_action_count(&second), 1);
    assert_eq!(client.coordinator_action_count(&ctx.coordinator), 0);
    let action = client.coordinator_action(&second, &1u32);
    assert_eq!(action.coordinator, second);
    assert_eq!(action.kind, CoordinatorActionKind::Adjusted);
}
