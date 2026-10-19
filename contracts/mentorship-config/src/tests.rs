extern crate std;

use soroban_sdk::{
    testutils::Address as _, Address, Env, Vec,
};

use crate::{
    ContractError, Environment, EvidenceRules, MentorshipConfigContract,
    MentorshipConfigContractClient, MentorshipSettings,
};

fn setup() -> (Env, Address, MentorshipConfigContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(MentorshipConfigContract, ());
    let client = MentorshipConfigContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, admin, client)
}

fn enabled_settings(env: &Env) -> MentorshipSettings {
    MentorshipSettings {
        enabled: true,
        max_active_engagements_per_participant: 3,
        max_sessions_per_engagement: 12,
        supported_currencies: Vec::from_array(env, [Address::generate(env)]),
        session_durations_minutes: Vec::from_array(env, [30, 60]),
        cancellation_window_seconds: 86_400,
        evidence_rules: EvidenceRules {
            minimum_references: 1,
            require_tutor_attestation: true,
            require_student_confirmation: true,
        },
    }
}

#[test]
fn missing_environment_is_disabled_with_safe_defaults() {
    let (env, _, client) = setup();
    let staging = client.get_config(&Environment::Staging);
    let production = client.get_config(&Environment::Production);

    assert!(!staging.settings.enabled);
    assert!(!production.settings.enabled);
    assert_eq!(staging.version, 0);
    assert_eq!(staging.settings.max_active_engagements_per_participant, 1);
    assert_eq!(staging.settings.max_sessions_per_engagement, 1);
    assert!(staging.settings.supported_currencies.is_empty());
    assert!(staging.settings.session_durations_minutes.is_empty());
    assert_eq!(staging.settings.cancellation_window_seconds, 86_400);
    assert_eq!(staging.settings.evidence_rules.minimum_references, 1);
    assert_eq!(staging.updated_by, None);
    assert_eq!(client.is_enabled(&Environment::Production), false);
    let _ = env;
}

#[test]
fn configuration_is_environment_scoped_and_versioned() {
    let (env, admin, client) = setup();
    let settings = enabled_settings(&env);

    assert_eq!(client.set_config(&admin, &Environment::Staging, &settings), 1);
    assert!(client.is_enabled(&Environment::Staging));
    assert!(!client.is_enabled(&Environment::Production));
    let stored = client.get_config(&Environment::Staging);
    assert_eq!(stored.version, 1);
    assert_eq!(stored.settings, settings);
    assert_eq!(stored.updated_by, Some(admin.clone()));

    let mut revised = settings;
    revised.max_sessions_per_engagement = 20;
    assert_eq!(client.set_config(&admin, &Environment::Staging, &revised), 2);
    assert_eq!(client.get_config(&Environment::Staging).version, 2);
}

#[test]
fn enabled_configuration_fails_closed_without_currency_duration_or_attestation() {
    let (env, admin, client) = setup();
    let mut settings = enabled_settings(&env);
    settings.supported_currencies = Vec::new(&env);
    assert_eq!(
        client.try_set_config(&admin, &Environment::Staging, &settings),
        Err(Ok(ContractError::CurrencyRequired))
    );

    settings = enabled_settings(&env);
    settings.session_durations_minutes = Vec::new(&env);
    assert_eq!(
        client.try_set_config(&admin, &Environment::Staging, &settings),
        Err(Ok(ContractError::SessionDurationRequired))
    );

    settings = enabled_settings(&env);
    settings.evidence_rules.require_tutor_attestation = false;
    assert_eq!(
        client.try_set_config(&admin, &Environment::Staging, &settings),
        Err(Ok(ContractError::InvalidEvidenceRules))
    );
    assert!(!client.is_enabled(&Environment::Staging));
}

#[test]
fn invalid_values_and_unauthorized_updates_are_rejected() {
    let (env, admin, client) = setup();
    let mut settings = enabled_settings(&env);
    settings.session_durations_minutes = Vec::from_array(&env, [15, 15]);
    assert_eq!(
        client.try_set_config(&admin, &Environment::Development, &settings),
        Err(Ok(ContractError::DuplicateSessionDuration))
    );

    let outsider = Address::generate(&env);
    let valid = enabled_settings(&env);
    assert_eq!(
        client.try_set_config(&outsider, &Environment::Development, &valid),
        Err(Ok(ContractError::NotAdmin))
    );
    assert!(!client.is_enabled(&Environment::Development));
}