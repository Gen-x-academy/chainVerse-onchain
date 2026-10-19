use crate::{
    Audience, BreachSeverity, ContractError, EngagementState, MentorshipConductContract,
    MentorshipConductContractClient, Role,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env,
};

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Setup {
    env: Env,
    contract_id: Address,
    admin: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);
    let contract_id = env.register(MentorshipConductContract, ());
    let admin = Address::generate(&env);
    let client = MentorshipConductContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Setup {
        env,
        contract_id,
        admin,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipConductContractClient<'a> {
    MentorshipConductContractClient::new(env, contract_id)
}

fn publish(
    env: &Env,
    contract_id: &Address,
    admin: &Address,
    audience: Audience,
    hash_byte: u8,
    mandatory: bool,
) -> u32 {
    client(env, contract_id).publish_policy(
        admin,
        &audience,
        &hash(env, hash_byte),
        &2_000u64,
        &mandatory,
    )
}

// ── initialization and authorization ───────────────────────────────────────

#[test]
fn test_initialize_twice_fails() {
    let s = setup();
    assert_eq!(
        client(&s.env, &s.contract_id).try_initialize(&s.admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_only_admin_can_publish() {
    let s = setup();
    let attacker = Address::generate(&s.env);
    assert_eq!(
        client(&s.env, &s.contract_id).try_publish_policy(
            &attacker,
            &Audience::Both,
            &hash(&s.env, 1),
            &2_000u64,
            &true,
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_publication_requires_valid_commitments() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    assert_eq!(
        c.try_publish_policy(
            &s.admin,
            &Audience::Both,
            &hash(&s.env, 0),
            &2_000u64,
            &true
        ),
        Err(Ok(ContractError::InvalidPolicyHash))
    );
    assert_eq!(
        c.try_publish_policy(&s.admin, &Audience::Both, &hash(&s.env, 1), &0u64, &true),
        Err(Ok(ContractError::InvalidEffectiveTime))
    );
}

// ── MENTOR-051 — versioned policy registry ─────────────────────────────────

#[test]
fn test_versions_are_sequential_and_latest_per_role() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);

    let v1 = publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    assert_eq!(v1, 1);
    assert_eq!(c.latest_policy(&Role::Mentor).version, 1);
    assert_eq!(c.latest_policy(&Role::Mentee).version, 1);

    let v2 = publish(&s.env, &s.contract_id, &s.admin, Audience::Mentor, 2, true);
    assert_eq!(v2, 2);
    // Mentor-scoped policy is latest only for mentors.
    assert_eq!(c.latest_policy(&Role::Mentor).version, 2);
    assert_eq!(c.latest_policy(&Role::Mentee).version, 1);
    assert_eq!(c.get_policy(&2).audience, Audience::Mentor);
}

#[test]
fn test_retirement_is_reported_and_not_repeatable() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    c.retire_policy(&s.admin, &1);
    assert!(c.get_policy(&1).retired);
    assert_eq!(
        c.try_retire_policy(&s.admin, &1),
        Err(Ok(ContractError::PolicyAlreadyRetired))
    );
}

// ── MENTOR-051 — acceptance is idempotent and provable ─────────────────────

#[test]
fn test_acceptance_is_idempotent() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 7, true);
    let mentor = Address::generate(&s.env);

    c.accept_policy(&mentor, &1, &Role::Mentor);
    let first = c.get_acceptance(&mentor, &1, &Role::Mentor);
    assert_eq!(first.policy_hash, hash(&s.env, 7));
    assert_eq!(first.accepted_at, 1_000);

    s.env.ledger().set_timestamp(5_000);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    let second = c.get_acceptance(&mentor, &1, &Role::Mentor);
    // Retry must not rewrite the recorded proof or its timestamp.
    assert_eq!(second.accepted_at, first.accepted_at);
    assert!(c.has_accepted(&mentor, &1, &Role::Mentor));
}

#[test]
fn test_acceptance_respects_applicability_and_retirement() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    publish(&s.env, &s.contract_id, &s.admin, Audience::Mentor, 1, true);
    let mentee = Address::generate(&s.env);

    assert_eq!(
        c.try_accept_policy(&mentee, &1, &Role::Mentee),
        Err(Ok(ContractError::PolicyNotApplicable))
    );

    let mentor = Address::generate(&s.env);
    c.retire_policy(&s.admin, &1);
    assert_eq!(
        c.try_accept_policy(&mentor, &1, &Role::Mentor),
        Err(Ok(ContractError::PolicyRetired))
    );
    assert_eq!(
        c.try_accept_policy(&mentor, &99, &Role::Mentor),
        Err(Ok(ContractError::PolicyNotFound))
    );
}

#[test]
fn test_acceptance_proof_pins_the_policy_hash() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 4, true);
    let mentor = Address::generate(&s.env);
    c.accept_policy(&mentor, &1, &Role::Mentor);

    assert!(c.verify_acceptance(&mentor, &1, &Role::Mentor, &hash(&s.env, 4)));
    assert!(!c.verify_acceptance(&mentor, &1, &Role::Mentor, &hash(&s.env, 5)));
    assert!(!c.verify_acceptance(&mentor, &2, &Role::Mentor, &hash(&s.env, 4)));
}

#[test]
fn test_acceptance_survives_retirement_for_proof_only() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 3, true);
    let mentor = Address::generate(&s.env);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.retire_policy(&s.admin, &1);

    // The historical proof remains verifiable.
    assert!(c.verify_acceptance(&mentor, &1, &Role::Mentor, &hash(&s.env, 3)));
    // But the retired policy can no longer gate anything.
    assert!(!c.requires_acknowledgement(&mentor, &Role::Mentor));
}

// ── MENTOR-051 — engagement activation gate ────────────────────────────────

fn engagement_setup(s: &Setup) -> (Address, Address, BytesN<32>) {
    let mentor = Address::generate(&s.env);
    let mentee = Address::generate(&s.env);
    let id = hash(&s.env, 11);
    client(&s.env, &s.contract_id).create_engagement(&mentor, &id, &mentor, &mentee);
    (mentor, mentee, id)
}

#[test]
fn test_activation_blocked_without_any_policy() {
    let s = setup();
    let (mentor, _, id) = engagement_setup(&s);
    assert_eq!(
        client(&s.env, &s.contract_id).try_activate_engagement(&mentor, &id),
        Err(Ok(ContractError::PolicyNotFound))
    );
}

#[test]
fn test_activation_blocked_without_acceptance() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);

    c.accept_policy(&mentor, &1, &Role::Mentor);
    assert_eq!(
        c.try_activate_engagement(&mentor, &id),
        Err(Ok(ContractError::AcceptanceRequired))
    );

    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentor, &id);
    let engagement = c.get_engagement(&id);
    assert_eq!(engagement.state, EngagementState::Active);
    assert_eq!(engagement.mentor_policy_version, 1);
    assert_eq!(engagement.mentee_policy_version, 1);
    assert!(c.engagement_compliant(&id));
}

#[test]
fn test_non_party_cannot_activate_or_create() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);

    let attacker = Address::generate(&s.env);
    assert_eq!(
        c.try_activate_engagement(&attacker, &id),
        Err(Ok(ContractError::NotAuthorized))
    );
    assert_eq!(
        c.try_create_engagement(&attacker, &hash(&s.env, 12), &mentor, &mentee),
        Err(Ok(ContractError::NotAuthorized))
    );
}

#[test]
fn test_engagement_creation_validation() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let mentor = Address::generate(&s.env);

    assert_eq!(
        c.try_create_engagement(&mentor, &hash(&s.env, 1), &mentor, &mentor),
        Err(Ok(ContractError::InvalidParties))
    );
    assert_eq!(
        c.try_create_engagement(
            &mentor,
            &BytesN::from_array(&s.env, &[0u8; 32]),
            &mentor,
            &Address::generate(&s.env),
        ),
        Err(Ok(ContractError::InvalidEngagementId))
    );

    let mentee = Address::generate(&s.env);
    c.create_engagement(&mentor, &hash(&s.env, 2), &mentor, &mentee);
    assert_eq!(
        c.try_create_engagement(&mentor, &hash(&s.env, 2), &mentor, &mentee),
        Err(Ok(ContractError::EngagementAlreadyExists))
    );
    assert_eq!(
        c.try_activate_engagement(&mentor, &hash(&s.env, 99)),
        Err(Ok(ContractError::EngagementNotFound))
    );
}

#[test]
fn test_activation_is_one_way() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentee, &id);
    assert_eq!(
        c.try_activate_engagement(&mentee, &id),
        Err(Ok(ContractError::EngagementNotPending))
    );
}

// ── MENTOR-051 — updated mandatory versions prompt re-acknowledgement ──────

#[test]
fn test_new_mandatory_policy_requires_reacknowledgement() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentor, &id);
    assert!(c.engagement_compliant(&id));

    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 2, true);
    assert!(!c.engagement_compliant(&id));
    assert!(c.requires_acknowledgement(&mentor, &Role::Mentor));
    assert!(c.requires_acknowledgement(&mentee, &Role::Mentee));

    c.accept_policy(&mentor, &2, &Role::Mentor);
    assert!(!c.engagement_compliant(&id)); // mentee still stale
    c.accept_policy(&mentee, &2, &Role::Mentee);
    assert!(c.engagement_compliant(&id));
    assert!(!c.requires_acknowledgement(&mentor, &Role::Mentor));
}

#[test]
fn test_non_mandatory_policy_needs_no_reacknowledgement() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentor, &id);

    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 2, false);
    assert!(c.engagement_compliant(&id));
    assert!(!c.requires_acknowledgement(&mentor, &Role::Mentor));
    assert_eq!(c.latest_policy(&Role::Mentor).version, 2);
}

#[test]
fn test_suspend_and_resume_require_reacknowledgement() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentor, &id);

    c.suspend_engagement(&s.admin, &id);
    assert_eq!(c.get_engagement(&id).state, EngagementState::Suspended);
    assert_eq!(
        c.try_suspend_engagement(&s.admin, &id),
        Err(Ok(ContractError::EngagementNotActive))
    );

    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 2, true);
    assert_eq!(
        c.try_resume_engagement(&mentor, &id),
        Err(Ok(ContractError::AcceptanceRequired))
    );

    c.accept_policy(&mentor, &2, &Role::Mentor);
    c.accept_policy(&mentee, &2, &Role::Mentee);
    c.resume_engagement(&mentor, &id);
    assert_eq!(c.get_engagement(&id).state, EngagementState::Active);
}

#[test]
fn test_end_engagement_is_terminal() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let (mentor, mentee, id) = engagement_setup(&s);
    let c = client(&s.env, &s.contract_id);
    c.accept_policy(&mentor, &1, &Role::Mentor);
    c.accept_policy(&mentee, &1, &Role::Mentee);
    c.activate_engagement(&mentor, &id);

    c.end_engagement(&mentee, &id);
    assert_eq!(c.get_engagement(&id).state, EngagementState::Ended);
    assert_eq!(
        c.try_end_engagement(&mentee, &id),
        Err(Ok(ContractError::EngagementNotActive))
    );
    assert_eq!(
        c.try_resume_engagement(&mentee, &id),
        Err(Ok(ContractError::EngagementNotSuspended))
    );
}

// ── MENTOR-051 — breach references ─────────────────────────────────────────

#[test]
fn test_breach_reference_is_recorded_once() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let c = client(&s.env, &s.contract_id);
    let subject = Address::generate(&s.env);

    c.record_breach(
        &s.admin,
        &hash(&s.env, 20),
        &subject,
        &1,
        &BreachSeverity::High,
        &hash(&s.env, 21),
    );
    let record = c.get_breach(&hash(&s.env, 20));
    assert_eq!(record.subject, subject);
    assert_eq!(record.policy_version, 1);
    assert_eq!(record.severity, BreachSeverity::High);
    assert_eq!(record.evidence_hash, hash(&s.env, 21));
    assert!(!record.resolved);

    assert_eq!(
        c.try_record_breach(
            &s.admin,
            &hash(&s.env, 20),
            &subject,
            &1,
            &BreachSeverity::High,
            &hash(&s.env, 21),
        ),
        Err(Ok(ContractError::BreachAlreadyRecorded))
    );
}

#[test]
fn test_breach_requires_policy_and_evidence_and_is_resolvable() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let subject = Address::generate(&s.env);

    assert_eq!(
        c.try_record_breach(
            &s.admin,
            &hash(&s.env, 30),
            &subject,
            &1,
            &BreachSeverity::Low,
            &hash(&s.env, 31),
        ),
        Err(Ok(ContractError::PolicyNotFound))
    );

    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    assert_eq!(
        c.try_record_breach(
            &s.admin,
            &hash(&s.env, 30),
            &subject,
            &1,
            &BreachSeverity::Low,
            &hash(&s.env, 0),
        ),
        Err(Ok(ContractError::InvalidEvidenceHash))
    );

    c.record_breach(
        &s.admin,
        &hash(&s.env, 30),
        &subject,
        &1,
        &BreachSeverity::Critical,
        &hash(&s.env, 31),
    );
    c.resolve_breach(&s.admin, &hash(&s.env, 30));
    assert!(c.get_breach(&hash(&s.env, 30)).resolved);
    assert_eq!(
        c.try_resolve_breach(&s.admin, &hash(&s.env, 30)),
        Err(Ok(ContractError::BreachAlreadyResolved))
    );
    assert_eq!(
        c.try_resolve_breach(&s.admin, &hash(&s.env, 99)),
        Err(Ok(ContractError::BreachNotFound))
    );
}

#[test]
fn test_only_admin_records_breaches() {
    let s = setup();
    publish(&s.env, &s.contract_id, &s.admin, Audience::Both, 1, true);
    let c = client(&s.env, &s.contract_id);
    let attacker = Address::generate(&s.env);
    assert_eq!(
        c.try_record_breach(
            &attacker,
            &hash(&s.env, 40),
            &Address::generate(&s.env),
            &1,
            &BreachSeverity::Medium,
            &hash(&s.env, 41),
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_accepted_versions_filters_unknown_versions() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let mentor = Address::generate(&s.env);
    // No policy exists yet, so nothing can be accepted.
    let mut candidates = soroban_sdk::Vec::new(&s.env);
    candidates.push_back(1u32);
    candidates.push_back(2u32);
    assert_eq!(
        c.accepted_versions(&mentor, &Role::Mentor, &candidates)
            .len(),
        0
    );
}

#[test]
fn test_version() {
    let s = setup();
    assert_eq!(client(&s.env, &s.contract_id).version(), 1);
}
