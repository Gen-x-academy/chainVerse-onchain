#![cfg(test)]

use mentor_ratelimit::{
    MentorRateLimit, MentorError, RATE_LIMIT_DIRECTORY_QUERY, RATE_LIMIT_PROFILE_ENUM,
    RATE_LIMIT_PROGRAM_DISCOVERY, RATE_LIMIT_COMPARISON_QUERY,
};
use soroban_sdk::{
    testutils::Address as _,
    Address, Env,
};

fn setup_env() -> Env {
    Env::default()
}

#[test]
fn test_init_success() {
    let env = setup_env();
    let admin = Address::random(&env);

    let result = MentorRateLimit::init(env.clone(), admin.clone());

    assert!(result.is_ok());
}

#[test]
fn test_init_twice_fails() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());
    let result = MentorRateLimit::init(env.clone(), admin.clone());

    assert_eq!(result, Err(MentorError::AlreadyInitialized));
}

#[test]
fn test_check_rate_limit_success() {
    let env = setup_env();
    let admin = Address::random(&env);
    let account = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    // First request should pass
    let result = MentorRateLimit::check_rate_limit(
        env.clone(),
        account.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
    );

    assert!(result.is_ok());
}

#[test]
fn test_check_rate_limit_exceeded() {
    let env = setup_env();
    let admin = Address::random(&env);
    let account = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    // Set a very low rate limit
    let _ = MentorRateLimit::set_rate_limit(
        env.clone(),
        admin.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
        1_u32, // Max 1 request per window
        60_u32,
    );

    // First request should pass
    let _ = MentorRateLimit::check_rate_limit(
        env.clone(),
        account.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
    );

    // Second request should fail
    let result = MentorRateLimit::check_rate_limit(
        env.clone(),
        account.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
    );

    assert_eq!(result, Err(MentorError::RateLimitExceeded));
}

#[test]
fn test_set_rate_limit_success() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    let result = MentorRateLimit::set_rate_limit(
        env.clone(),
        admin.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
        20_u32,
        120_u32,
    );

    assert!(result.is_ok());

    let (max, window) = MentorRateLimit::get_rate_limit_config(env, RATE_LIMIT_DIRECTORY_QUERY).unwrap();
    assert_eq!(max, 20);
    assert_eq!(window, 120);
}

#[test]
fn test_set_rate_limit_invalid_type() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    let result = MentorRateLimit::set_rate_limit(
        env.clone(),
        admin.clone(),
        10_u8, // Invalid type
        20_u32,
        120_u32,
    );

    assert_eq!(result, Err(MentorError::InvalidInput));
}

#[test]
fn test_get_rate_limit_config() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    let result = MentorRateLimit::get_rate_limit_config(
        env.clone(),
        RATE_LIMIT_PROFILE_ENUM,
    );

    assert!(result.is_ok());

    let (max, window) = result.unwrap();
    assert!(max > 0);
    assert!(window > 0);
}

#[test]
fn test_get_abuse_signals() {
    let env = setup_env();
    let admin = Address::random(&env);
    let account = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin);

    let signals = MentorRateLimit::get_abuse_signals(env, account.clone());
    
    // Should be all false initially
    assert!(!signals.rapid_queries);
    assert!(!signals.enumeration_pattern);
    assert!(!signals.cache_bypass_attempts);
    assert_eq!(signals.risk_score, 0);
}

#[test]
fn test_record_abuse_signal() {
    let env = setup_env();
    let admin = Address::random(&env);
    let account = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin);

    let result = MentorRateLimit::record_abuse_signal(
        env.clone(),
        account.clone(),
        0, // rapid_queries signal
    );

    assert!(result.is_ok());

    let signals = MentorRateLimit::get_abuse_signals(env, account);
    assert!(signals.rapid_queries);
    assert!(signals.risk_score > 0);
}

#[test]
fn test_reset_rate_limits() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    // Change a rate limit
    let _ = MentorRateLimit::set_rate_limit(
        env.clone(),
        admin.clone(),
        RATE_LIMIT_DIRECTORY_QUERY,
        50_u32,
        120_u32,
    );

    // Reset
    let result = MentorRateLimit::reset_rate_limits(env.clone(), admin.clone());
    assert!(result.is_ok());

    // Check it's back to default
    let (max, _window) = MentorRateLimit::get_rate_limit_config(env, RATE_LIMIT_DIRECTORY_QUERY).unwrap();
    assert_eq!(max, 10); // Should be default
}

#[test]
fn test_clear_abuse_signals() {
    let env = setup_env();
    let admin = Address::random(&env);
    let account = Address::random(&env);

    let _ = MentorRateLimit::init(env.clone(), admin.clone());

    // Record signal
    let _ = MentorRateLimit::record_abuse_signal(
        env.clone(),
        account.clone(),
        0,
    );

    // Verify signal recorded
    let signals_before = MentorRateLimit::get_abuse_signals(env.clone(), account.clone());
    assert!(signals_before.rapid_queries);

    // Clear
    let result = MentorRateLimit::clear_abuse_signals(env.clone(), admin.clone(), account.clone());
    assert!(result.is_ok());

    // Verify cleared
    let signals_after = MentorRateLimit::get_abuse_signals(env, account);
    assert!(!signals_after.rapid_queries);
    assert_eq!(signals_after.risk_score, 0);
}
