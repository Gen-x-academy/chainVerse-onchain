#![cfg(test)]

use mentor_registry::{MentorRegistry, MentorError};
use mentor_types::DeliveryMode;
use soroban_sdk::{
    testutils::{Address as _, BytesN as _},
    Address, BytesN, Env, String as SorobanString,
};

fn setup_env() -> Env {
    Env::default()
}

#[test]
fn test_init_success() {
    let env = setup_env();
    let admin = Address::random(&env);

    let result = MentorRegistry::init(env.clone(), admin.clone());

    assert!(result.is_ok());
}

#[test]
fn test_init_twice_fails() {
    let env = setup_env();
    let admin = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin.clone());
    let result = MentorRegistry::init(env.clone(), admin.clone());

    assert_eq!(result, Err(MentorError::AlreadyInitialized));
}

#[test]
fn test_register_mentor_success() {
    let env = setup_env();
    let admin = Address::random(&env);
    let mentor = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin.clone());

    let expertise_hash = BytesN::<32>::random(&env);
    let profile_uri = SorobanString::from_small_str("ipfs://QmExample");

    let result = MentorRegistry::register_mentor(
        env.clone(),
        mentor.clone(),
        expertise_hash.clone(),
        0xFF_u32, // All hours available
        0x07_u8,  // All delivery modes
        1000_u64,  // 1000 stroops/hour
        profile_uri,
    );

    assert!(result.is_ok());

    // Verify mentor was registered
    let retrieved = MentorRegistry::get_mentor(env.clone(), mentor.clone());
    assert!(retrieved.is_some());

    let profile = retrieved.unwrap();
    assert_eq!(profile.mentor_id, mentor);
    assert_eq!(profile.base_price, 1000);
    assert!(profile.active);
}

#[test]
fn test_register_mentor_twice_fails() {
    let env = setup_env();
    let admin = Address::random(&env);
    let mentor = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin.clone());

    let expertise_hash = BytesN::<32>::random(&env);
    let profile_uri = SorobanString::from_small_str("ipfs://QmExample");

    let _ = MentorRegistry::register_mentor(
        env.clone(),
        mentor.clone(),
        expertise_hash.clone(),
        0xFF_u32,
        0x07_u8,
        1000_u64,
        profile_uri.clone(),
    );

    let result = MentorRegistry::register_mentor(
        env.clone(),
        mentor.clone(),
        expertise_hash,
        0xFF_u32,
        0x07_u8,
        1000_u64,
        profile_uri,
    );

    assert_eq!(result, Err(MentorError::Unauthorized));
}

#[test]
fn test_update_mentor_status() {
    let env = setup_env();
    let admin = Address::random(&env);
    let mentor = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin.clone());

    let expertise_hash = BytesN::<32>::random(&env);
    let profile_uri = SorobanString::from_small_str("ipfs://QmExample");

    let _ = MentorRegistry::register_mentor(
        env.clone(),
        mentor.clone(),
        expertise_hash,
        0xFF_u32,
        0x07_u8,
        1000_u64,
        profile_uri,
    );

    // Update status
    let result = MentorRegistry::update_mentor_status(
        env.clone(),
        mentor.clone(),
        false, // Deactivate
        0x00_u32,
        2000_u64,
    );

    assert!(result.is_ok());

    // Verify update
    let retrieved = MentorRegistry::get_mentor(env.clone(), mentor.clone());
    assert!(retrieved.is_some());

    let profile = retrieved.unwrap();
    assert!(!profile.active);
    assert_eq!(profile.base_price, 2000);
}

#[test]
fn test_get_nonexistent_mentor() {
    let env = setup_env();
    let admin = Address::random(&env);
    let mentor = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin);

    let result = MentorRegistry::get_mentor(env, mentor);
    assert!(result.is_none());
}

#[test]
fn test_compare_mentors() {
    let env = setup_env();
    let admin = Address::random(&env);
    let mentor1 = Address::random(&env);
    let mentor2 = Address::random(&env);

    let _ = MentorRegistry::init(env.clone(), admin.clone());

    let expertise_hash = BytesN::<32>::random(&env);
    let profile_uri = SorobanString::from_small_str("ipfs://QmExample");

    let _ = MentorRegistry::register_mentor(
        env.clone(),
        mentor1.clone(),
        expertise_hash.clone(),
        0xFF_u32,
        0x07_u8,
        1000_u64,
        profile_uri.clone(),
    );

    let _ = MentorRegistry::register_mentor(
        env.clone(),
        mentor2.clone(),
        expertise_hash,
        0xAA_u32, // Different availability
        0x07_u8,
        2000_u64,
        profile_uri,
    );

    let mut mentor_ids = soroban_sdk::Vec::new(&env);
    mentor_ids.push_back(mentor1.clone());
    mentor_ids.push_back(mentor2.clone());

    let result = MentorRegistry::compare_mentors(env.clone(), mentor_ids, None);
    assert!(result.is_ok());

    let comparisons = result.unwrap();
    assert_eq!(comparisons.len(), 2);
    assert_eq!(comparisons.get(0).unwrap().mentor_id, mentor1);
    assert_eq!(comparisons.get(1).unwrap().mentor_id, mentor2);
    assert_eq!(comparisons.get(0).unwrap().price, 1000);
    assert_eq!(comparisons.get(1).unwrap().price, 2000);
}
