#![cfg(test)]

use mentor_matching::{MentorMatching, MentorError};
use mentor_types::{DeliveryMode, MenteeProfile};
use soroban_sdk::{
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env,
};

fn setup_env() -> Env {
    Env::default()
}

#[test]
fn test_init_success() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery = Address::random(&env);

    let result = MentorMatching::init(
        env.clone(),
        admin.clone(),
        registry.clone(),
        discovery.clone(),
    );

    assert!(result.is_ok());
}

#[test]
fn test_init_twice_fails() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin.clone(),
        registry.clone(),
        discovery.clone(),
    );

    let result = MentorMatching::init(
        env.clone(),
        admin.clone(),
        registry.clone(),
        discovery.clone(),
    );

    assert_eq!(result, Err(MentorError::AlreadyInitialized));
}

#[test]
fn test_get_recommendations() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin,
        registry,
        discovery,
    );

    let mentee_profile = MenteeProfile {
        goal_hash: BytesN::<32>::random(&env),
        expertise_level: 50,
        language_preferences: soroban_sdk::Vec::new(&env),
        available_hours: 0xFF_u32,
        preferred_mode: DeliveryMode::Synchronous,
    };

    let result = MentorMatching::get_recommendations(
        env.clone(),
        mentee_profile,
        1_u32,
        10_u32,
    );

    assert!(result.is_ok());

    let recommendations = result.unwrap();
    // Should be empty for now (no mentors registered)
    assert_eq!(recommendations.len(), 0);
}

#[test]
fn test_get_recommendations_invalid_limit() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin,
        registry,
        discovery,
    );

    let mentee_profile = MenteeProfile {
        goal_hash: BytesN::<32>::random(&env),
        expertise_level: 50,
        language_preferences: soroban_sdk::Vec::new(&env),
        available_hours: 0xFF_u32,
        preferred_mode: DeliveryMode::Synchronous,
    };

    let result = MentorMatching::get_recommendations(
        env.clone(),
        mentee_profile,
        1_u32,
        0_u32, // Invalid: 0 limit
    );

    assert_eq!(result, Err(MentorError::InvalidInput));
}

#[test]
fn test_calculate_fit_score() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery = Address::random(&env);
    let mentor = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin,
        registry,
        discovery,
    );

    let mentee_profile = MenteeProfile {
        goal_hash: BytesN::<32>::random(&env),
        expertise_level: 50,
        language_preferences: soroban_sdk::Vec::new(&env),
        available_hours: 0xFF_u32,
        preferred_mode: DeliveryMode::Synchronous,
    };

    let result = MentorMatching::calculate_fit_score(
        env.clone(),
        mentee_profile,
        mentor,
        1_u32,
    );

    assert!(result.is_ok());

    let score = result.unwrap();
    assert!(score >= 0 && score <= 10000);
}

#[test]
fn test_set_registry() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry1 = Address::random(&env);
    let registry2 = Address::random(&env);
    let discovery = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin.clone(),
        registry1.clone(),
        discovery,
    );

    let result = MentorMatching::set_registry(env.clone(), admin.clone(), registry2.clone());
    assert!(result.is_ok());

    let retrieved = MentorMatching::get_registry(env);
    assert_eq!(retrieved, Some(registry2));
}

#[test]
fn test_set_discovery() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let discovery1 = Address::random(&env);
    let discovery2 = Address::random(&env);

    let _ = MentorMatching::init(
        env.clone(),
        admin.clone(),
        registry,
        discovery1.clone(),
    );

    let result = MentorMatching::set_discovery(env.clone(), admin.clone(), discovery2.clone());
    assert!(result.is_ok());

    let retrieved = MentorMatching::get_discovery(env);
    assert_eq!(retrieved, Some(discovery2));
}
