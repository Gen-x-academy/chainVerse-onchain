use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Events, testutils::Ledger, Address, BytesN,
    Env, Symbol, TryFromVal, Val, Vec,
};

use crate::{
    Audience, AudienceVisibility, CapstoneFieldPrivacyContract, CapstoneFieldPrivacyContractClient,
    ContractError, PolicyState, ProjectField, RevokeFieldVisibilityInput, RuleState,
    SetFieldPolicyInput, SetFieldVisibilityInput, MAX_RULES_PER_PROJECT,
};

const NOW: u64 = 1_700_000_000;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn hash_u32(env: &Env, value: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0..4].copy_from_slice(&value.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn allow(audience: Audience) -> AudienceVisibility {
    let mut allowed = AudienceVisibility::none();
    match audience {
        Audience::Public => allowed.public = true,
        Audience::Participants => allowed.participants = true,
        Audience::Supervisors => allowed.supervisors = true,
        Audience::Reviewers => allowed.reviewers = true,
        Audience::Sponsors => allowed.sponsors = true,
        Audience::Administrators => allowed.administrators = true,
    }
    allowed
}

fn allow_many(audiences: &[Audience]) -> AudienceVisibility {
    let mut allowed = AudienceVisibility::none();
    for audience in audiences.iter() {
        match audience {
            Audience::Public => allowed.public = true,
            Audience::Participants => allowed.participants = true,
            Audience::Supervisors => allowed.supervisors = true,
            Audience::Reviewers => allowed.reviewers = true,
            Audience::Sponsors => allowed.sponsors = true,
            Audience::Administrators => allowed.administrators = true,
        }
    }
    allowed
}

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    BytesN<32>,
    BytesN<32>,
    CapstoneFieldPrivacyContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let id = env.register(CapstoneFieldPrivacyContract, ());
    let client = CapstoneFieldPrivacyContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let participant = Address::generate(&env);
    let supervisor = Address::generate(&env);
    let reviewer = Address::generate(&env);
    let outsider = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin);
    client.set_role(
        &admin,
        &participant,
        &tenant,
        &project,
        &Audience::Participants,
        &true,
    );
    client.set_role(
        &admin,
        &supervisor,
        &tenant,
        &project,
        &Audience::Supervisors,
        &true,
    );
    client.set_role(
        &admin,
        &reviewer,
        &tenant,
        &project,
        &Audience::Reviewers,
        &true,
    );
    (
        env,
        admin,
        subject,
        participant,
        supervisor,
        reviewer,
        outsider,
        tenant,
        project,
        client,
    )
}

fn visibility_input(
    env: &Env,
    field: ProjectField,
    request: u8,
    allowed: AudienceVisibility,
) -> SetFieldVisibilityInput {
    SetFieldVisibilityInput {
        field,
        allowed,
        valid_until: 0,
        request_id: hash(env, request),
        expected_version: 0,
    }
}

#[test]
fn initialize_is_one_time_and_admin_scoped() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(CapstoneFieldPrivacyContract, ());
    let client = CapstoneFieldPrivacyContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let other = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    assert_eq!(
        client
            .try_set_role(
                &other,
                &other,
                &tenant,
                &project,
                &Audience::Participants,
                &true,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotInitialized
    );
    client.initialize(&admin);
    assert_eq!(
        client.try_initialize(&admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
    assert_eq!(
        client
            .try_set_role(
                &other,
                &other,
                &tenant,
                &project,
                &Audience::Participants,
                &true,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn roles_are_tenant_and_project_scoped_and_deny_by_default() {
    let (
        env,
        _admin,
        subject,
        participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    let other_tenant = hash(&env, 3);
    let other_project = hash(&env, 4);
    assert!(client.has_role(&participant, &tenant, &project, &Audience::Participants));
    assert!(!client.has_role(
        &participant,
        &other_tenant,
        &project,
        &Audience::Participants
    ));
    assert!(!client.has_role(
        &participant,
        &tenant,
        &other_project,
        &Audience::Participants
    ));
    assert!(!client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Notes
    ));
    assert_eq!(
        client
            .try_get_rule(&tenant, &project, &subject, &ProjectField::Notes)
            .unwrap_err()
            .unwrap(),
        ContractError::RuleNotFound
    );
}

#[test]
fn field_visibility_is_enforced_per_audience() {
    let (
        env,
        _admin,
        subject,
        participant,
        supervisor,
        reviewer,
        outsider,
        tenant,
        project,
        client,
    ) = setup();
    let allowed = allow_many(&[Audience::Participants, Audience::Reviewers]);
    client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(&env, ProjectField::Notes, 10, allowed),
    );
    assert!(client.is_visible(&subject, &tenant, &project, &subject, &ProjectField::Notes));
    assert!(client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Notes
    ));
    assert!(client.is_visible(&reviewer, &tenant, &project, &subject, &ProjectField::Notes));
    assert!(!client.is_visible(
        &supervisor,
        &tenant,
        &project,
        &subject,
        &ProjectField::Notes
    ));
    assert!(!client.is_visible(&outsider, &tenant, &project, &subject, &ProjectField::Notes));
    let other_project = hash(&env, 4);
    assert!(!client.is_visible(
        &participant,
        &tenant,
        &other_project,
        &subject,
        &ProjectField::Notes
    ));
}

#[test]
fn policy_limits_and_freeze_transitions_are_actionable() {
    let (
        env,
        admin,
        subject,
        _participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    let restricted = allow(Audience::Participants);
    client.set_policy(
        &admin,
        &tenant,
        &project,
        &SetFieldPolicyInput {
            field: ProjectField::Funding,
            allowed: restricted,
            request_id: hash(&env, 20),
            expected_version: 0,
        },
    );
    assert_eq!(
        client
            .try_set_visibility(
                &subject,
                &tenant,
                &project,
                &visibility_input(&env, ProjectField::Funding, 21, allow(Audience::Public)),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::PolicyLimit
    );
    let frozen = client.freeze_policy(&admin, &tenant, &project, &ProjectField::Funding);
    assert_eq!(frozen.state, PolicyState::Frozen);
    assert_eq!(
        client
            .try_freeze_policy(&admin, &tenant, &project, &ProjectField::Funding)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTransition
    );
    assert_eq!(
        client
            .try_set_visibility(
                &subject,
                &tenant,
                &project,
                &visibility_input(&env, ProjectField::Funding, 22, restricted),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::PolicyFrozen
    );
    let active = client.unfreeze_policy(&admin, &tenant, &project, &ProjectField::Funding);
    assert_eq!(active.state, PolicyState::Active);
    let rule = client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(&env, ProjectField::Funding, 22, restricted),
    );
    assert_eq!(rule.field, ProjectField::Funding);
}

#[test]
fn retries_are_idempotent_and_request_ids_cannot_be_reused() {
    let (
        env,
        _admin,
        subject,
        _participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    let input = visibility_input(&env, ProjectField::Proposal, 30, allow(Audience::Public));
    let first = client.set_visibility(&subject, &tenant, &project, &input);
    assert_eq!(env.events().all().len(), 1);
    let retry = client.set_visibility(&subject, &tenant, &project, &input);
    assert_eq!(retry, first);
    assert_eq!(env.events().all().len(), 0);
    let mut conflict = input;
    conflict.allowed = allow(Audience::Participants);
    assert_eq!(
        client
            .try_set_visibility(&subject, &tenant, &project, &conflict)
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
}

#[test]
fn stale_versions_are_rejected() {
    let (
        env,
        _admin,
        subject,
        _participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(&env, ProjectField::Progress, 40, allow(Audience::Public)),
    );
    let mut stale = visibility_input(&env, ProjectField::Progress, 41, allow(Audience::Public));
    stale.expected_version = 0;
    assert_eq!(
        client
            .try_set_visibility(&subject, &tenant, &project, &stale)
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    stale.expected_version = 9;
    stale.request_id = hash(&env, 42);
    assert_eq!(
        client
            .try_set_visibility(&subject, &tenant, &project, &stale)
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    stale.expected_version = 1;
    stale.request_id = hash(&env, 43);
    let updated = client.set_visibility(&subject, &tenant, &project, &stale);
    assert_eq!(updated.expected_version, 2);
}

#[test]
fn expiry_hides_fields_and_can_be_extended() {
    let (
        env,
        admin,
        subject,
        participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    let _ = admin;
    let mut input = visibility_input(
        &env,
        ProjectField::Location,
        50,
        allow(Audience::Participants),
    );
    input.valid_until = NOW + 100;
    client.set_visibility(&subject, &tenant, &project, &input);
    assert!(client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Location
    ));
    env.ledger().set_timestamp(NOW + 100);
    assert!(!client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Location
    ));
    assert!(!client.is_visible(
        &subject,
        &tenant,
        &project,
        &subject,
        &ProjectField::Location
    ));
    let mut renew = visibility_input(
        &env,
        ProjectField::Location,
        51,
        allow(Audience::Participants),
    );
    renew.expected_version = 1;
    renew.valid_until = NOW + 200;
    let renewed = client.set_visibility(&subject, &tenant, &project, &renew);
    assert_eq!(renewed.valid_until, NOW + 200);
    assert!(client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Location
    ));
}

#[test]
fn storage_bounds_are_enforced() {
    let (
        env,
        _admin,
        _subject,
        _participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    for i in 0..MAX_RULES_PER_PROJECT {
        let owner = Address::generate(&env);
        client.set_visibility(
            &owner,
            &tenant,
            &project,
            &SetFieldVisibilityInput {
                field: ProjectField::Profile,
                allowed: allow(Audience::Public),
                valid_until: 0,
                request_id: hash_u32(&env, i),
                expected_version: 0,
            },
        );
    }
    let extra = Address::generate(&env);
    assert_eq!(
        client
            .try_set_visibility(
                &extra,
                &tenant,
                &project,
                &SetFieldVisibilityInput {
                    field: ProjectField::Profile,
                    allowed: allow(Audience::Public),
                    valid_until: 0,
                    request_id: hash_u32(&env, MAX_RULES_PER_PROJECT),
                    expected_version: 0,
                },
            )
            .unwrap_err()
            .unwrap(),
        ContractError::LimitReached
    );
}

#[test]
fn event_topics_and_schemas_are_stable() {
    let (
        env,
        admin,
        subject,
        participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    let topic = |index: u32| -> Symbol {
        let (_, topics, _) = env.events().all().get(index).unwrap();
        Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap()
    };
    client.set_policy(
        &admin,
        &tenant,
        &project,
        &SetFieldPolicyInput {
            field: ProjectField::Outputs,
            allowed: allow(Audience::Public),
            request_id: hash(&env, 60),
            expected_version: 0,
        },
    );
    assert_eq!(topic(0), symbol_short!("CAPFLDPOL"));
    client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(&env, ProjectField::Outputs, 61, allow(Audience::Public)),
    );
    assert_eq!(topic(0), symbol_short!("CAPFLDRUL"));
    let (_, _, data) = env.events().all().get(0).unwrap();
    let data = Vec::<Val>::try_from_val(&env, &data).unwrap();
    assert_eq!(data.len(), 8);
    client.revoke_visibility(
        &subject,
        &tenant,
        &project,
        &RevokeFieldVisibilityInput {
            field: ProjectField::Outputs,
            request_id: hash(&env, 62),
            expected_version: 1,
        },
    );
    assert_eq!(topic(0), symbol_short!("CAPFLDREV"));
    client.set_role(
        &admin,
        &participant,
        &tenant,
        &project,
        &Audience::Participants,
        &true,
    );
    assert_eq!(topic(0), symbol_short!("CAPFLDROL"));
}

#[test]
fn error_codes_are_stable() {
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::AlreadyInitialized as u32, 2);
    assert_eq!(ContractError::Unauthorized as u32, 3);
    assert_eq!(ContractError::PolicyLimit as u32, 4);
    assert_eq!(ContractError::RuleNotFound as u32, 5);
    assert_eq!(ContractError::PolicyNotFound as u32, 6);
    assert_eq!(ContractError::StaleVersion as u32, 7);
    assert_eq!(ContractError::RequestExists as u32, 8);
    assert_eq!(ContractError::RequestConflict as u32, 9);
    assert_eq!(ContractError::LimitReached as u32, 10);
    assert_eq!(ContractError::Expired as u32, 11);
    assert_eq!(ContractError::PolicyFrozen as u32, 12);
    assert_eq!(ContractError::InvalidExpiry as u32, 13);
    assert_eq!(ContractError::InvalidTransition as u32, 14);
    assert_eq!(ContractError::RuleRevoked as u32, 15);
    assert_eq!(ContractError::InvalidInput as u32, 16);
}

#[test]
fn bounded_updates_stay_within_default_resource_budget() {
    let (
        env,
        _admin,
        subject,
        _participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    env.cost_estimate().budget().reset_default();
    let fields = [
        ProjectField::Profile,
        ProjectField::Proposal,
        ProjectField::Membership,
        ProjectField::Notes,
        ProjectField::Progress,
        ProjectField::Outputs,
        ProjectField::Location,
        ProjectField::Funding,
        ProjectField::Achievements,
    ];
    for (index, field) in fields.iter().enumerate() {
        client.set_visibility(
            &subject,
            &tenant,
            &project,
            &SetFieldVisibilityInput {
                field: *field,
                allowed: allow(Audience::Public),
                valid_until: 0,
                request_id: hash_u32(&env, 1_000 + index as u32),
                expected_version: 0,
            },
        );
    }
    let rule = client.get_rule(&tenant, &project, &subject, &ProjectField::Achievements);
    assert_eq!(rule.field, ProjectField::Achievements);
    assert_eq!(rule.expected_version, 1);
}

#[test]
fn upgrade_metadata_and_stored_rules_are_compatible() {
    let (
        env,
        _admin,
        subject,
        participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(
            &env,
            ProjectField::Membership,
            70,
            allow(Audience::Participants),
        ),
    );
    assert_eq!(client.version(), 1);
    assert!(client.supports_version(&1));
    assert!(!client.supports_version(&2));
    let rule = client.get_rule(&tenant, &project, &subject, &ProjectField::Membership);
    assert_eq!(rule.expected_version, 1);
    assert!(client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Membership
    ));
}

#[test]
fn revocation_and_reactivation_follow_valid_transitions() {
    let (
        env,
        _admin,
        subject,
        participant,
        _supervisor,
        _reviewer,
        _outsider,
        tenant,
        project,
        client,
    ) = setup();
    client.set_visibility(
        &subject,
        &tenant,
        &project,
        &visibility_input(
            &env,
            ProjectField::Achievements,
            80,
            allow(Audience::Participants),
        ),
    );
    let revoked = client.revoke_visibility(
        &subject,
        &tenant,
        &project,
        &RevokeFieldVisibilityInput {
            field: ProjectField::Achievements,
            request_id: hash(&env, 81),
            expected_version: 1,
        },
    );
    assert_eq!(revoked.state, RuleState::Revoked);
    assert_eq!(revoked.expected_version, 2);
    assert!(!client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Achievements
    ));
    assert_eq!(
        client
            .try_revoke_visibility(
                &subject,
                &tenant,
                &project,
                &RevokeFieldVisibilityInput {
                    field: ProjectField::Achievements,
                    request_id: hash(&env, 82),
                    expected_version: 2,
                },
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RuleRevoked
    );
    let mut reactivate = visibility_input(
        &env,
        ProjectField::Achievements,
        83,
        allow(Audience::Participants),
    );
    reactivate.expected_version = 2;
    let active = client.set_visibility(&subject, &tenant, &project, &reactivate);
    assert_eq!(active.state, RuleState::Active);
    assert!(client.is_visible(
        &participant,
        &tenant,
        &project,
        &subject,
        &ProjectField::Achievements
    ));
}
