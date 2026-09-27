extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::{
    Action, ContractError, RiskTier, ScholarshipRateLimitsContract,
    ScholarshipRateLimitsContractClient,
};

fn setup() -> (Env, Address, ScholarshipRateLimitsContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipRateLimitsContract, ());
    let client = ScholarshipRateLimitsContractClient::new(&env, &id);
    (env, id, client)
}

// ── initialization ────────────────────────────────────────────────────────

#[test]
fn test_initialize_seeds_defaults() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let cfg = client.get_action_config(&Action::Payout);
    assert_eq!(cfg.risk_tier, RiskTier::High);
    assert_eq!(cfg.max_count, 5);
}

#[test]
fn test_double_initialize_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, ContractError::AlreadyInitialized);
}

// ── action config validation ──────────────────────────────────────────────

#[test]
fn test_set_action_config_validation() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Zero window rejected.
    let err = client
        .try_set_action_config(&admin, &Action::Discovery, &RiskTier::Low, &0, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidWindow);

    // Zero max_count rejected.
    let err2 = client
        .try_set_action_config(&admin, &Action::Discovery, &RiskTier::Low, &3600, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err2, ContractError::InvalidMaxCount);
}

// ── low-tier rate limiting ────────────────────────────────────────────────

#[test]
fn test_low_tier_limit_enforced() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    // Set discovery to max 2 per window.
    client.set_action_config(&admin, &Action::Discovery, &RiskTier::Low, &3600, &2);

    let actor = Address::generate(&env);
    client.check_and_record(&actor, &Action::Discovery);
    client.check_and_record(&actor, &Action::Discovery);
    // Third call exceeds the limit.
    let err = client
        .try_check_and_record(&actor, &Action::Discovery)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RateLimitExceeded);
}

#[test]
fn test_different_actors_have_independent_buckets() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_action_config(&admin, &Action::Messaging, &RiskTier::Low, &3600, &2);

    let actor_a = Address::generate(&env);
    let actor_b = Address::generate(&env);
    // Each actor uses 2 calls independently.
    client.check_and_record(&actor_a, &Action::Messaging);
    client.check_and_record(&actor_a, &Action::Messaging);
    client.check_and_record(&actor_b, &Action::Messaging);
    client.check_and_record(&actor_b, &Action::Messaging);

    // Both at limit now.
    let err_a = client
        .try_check_and_record(&actor_a, &Action::Messaging)
        .unwrap_err()
        .unwrap();
    assert_eq!(err_a, ContractError::RateLimitExceeded);
    let err_b = client
        .try_check_and_record(&actor_b, &Action::Messaging)
        .unwrap_err()
        .unwrap();
    assert_eq!(err_b, ContractError::RateLimitExceeded);
}

// ── high-tier service auth ────────────────────────────────────────────────

#[test]
fn test_high_tier_without_service_auth_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    // Payout is High tier by default.
    let actor = Address::generate(&env);
    let err = client
        .try_check_and_record(&actor, &Action::Payout)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::ServiceAuthRequired);
}

#[test]
fn test_high_tier_with_service_auth_succeeds() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let service = Address::generate(&env);
    client.grant_service_auth(&admin, &service);
    // Payout call should succeed (1 used of 5).
    let remaining = client.check_and_record(&service, &Action::Payout);
    assert!(remaining < 5);
}

#[test]
fn test_revoke_service_auth_removes_bypass() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let service = Address::generate(&env);
    client.grant_service_auth(&admin, &service);
    client.check_and_record(&service, &Action::Payout);
    client.revoke_service_auth(&admin, &service);
    let err = client
        .try_check_and_record(&service, &Action::Payout)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::ServiceAuthRequired);
}

// ── current_usage ─────────────────────────────────────────────────────────

#[test]
fn test_current_usage_reflects_consumption() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_action_config(&admin, &Action::Application, &RiskTier::Medium, &86400, &10);
    let actor = Address::generate(&env);
    client.check_and_record(&actor, &Action::Application);
    client.check_and_record(&actor, &Action::Application);
    let (used, remaining, _reset) = client.current_usage(&actor, &Action::Application);
    assert_eq!(used, 2);
    assert_eq!(remaining, 8);
}

// ── non-admin cannot set config ───────────────────────────────────────────

#[test]
fn test_non_admin_cannot_set_config() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let attacker = Address::generate(&env);
    let err = client
        .try_set_action_config(&attacker, &Action::Discovery, &RiskTier::Low, &3600, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::NotAdmin);
}

// ── draft safety: failed check must not increment counter ────────────────

#[test]
fn test_failed_check_does_not_increment_counter() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_action_config(&admin, &Action::Upload, &RiskTier::Medium, &3600, &1);
    let actor = Address::generate(&env);
    client.check_and_record(&actor, &Action::Upload);
    // Second call fails.
    let _ = client.try_check_and_record(&actor, &Action::Upload);
    // Usage should still be 1, not 2.
    let (used, _, _) = client.current_usage(&actor, &Action::Upload);
    assert_eq!(used, 1);
}
