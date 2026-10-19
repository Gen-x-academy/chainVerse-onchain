#![cfg(test)]
mod utils;

use soroban_sdk::{Address, Bytes, Env};
use mentor_profiles::MentorProfilesContract;

#[test]
fn test_publish_profile_success() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let expertise_hash = Bytes::new(&env, &[1; 32]);
    let attestation_hash = Bytes::new(&env, &[2; 32]);
    let policy_hash = Bytes::new(&env, &[3; 32]);
    
    let result = contract.publish_profile(
        env.clone(),
        expertise_hash.clone(),
        5,                // timezone offset
        0b111,            // delivery modes (all three)
        1000000,          // 0.1 XLM per hour
        attestation_hash,
        policy_hash,
    );
    
    assert!(result.is_ok());
    
    // Verify profile was stored
    let profile = contract.get_profile(env.clone(), mentor.clone()).unwrap();
    assert_eq!(profile.mentor_id, mentor);
    assert_eq!(profile.expertise_hash, expertise_hash);
    assert_eq!(profile.timezone_offset, 5);
    assert_eq!(profile.delivery_modes, 0b111);
    assert_eq!(profile.rate_per_hour, 1000000);
    assert_eq!(profile.rating_count, 0);
    assert_eq!(profile.reputation_score, 0);
}

#[test]
fn test_publish_profile_invalid_hash_length() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let invalid_hash = Bytes::new(&env, &[1; 16]); // Wrong length
    let valid_hash = Bytes::new(&env, &[2; 32]);
    
    let result = contract.publish_profile(
        env.clone(),
        invalid_hash,     // Too short
        5,
        0b111,
        1000000,
        valid_hash.clone(),
        valid_hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_publish_profile_invalid_timezone() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    
    let result = contract.publish_profile(
        env.clone(),
        hash.clone(),
        20,  // Invalid: > 14
        0b111,
        1000000,
        hash.clone(),
        hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_publish_profile_zero_rate() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    
    let result = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        0,  // Invalid: zero rate
        hash.clone(),
        hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_publish_duplicate_profile_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    
    // First publish
    let _ = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        1000000,
        hash.clone(),
        hash.clone(),
    );
    
    // Second publish should fail
    let result = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        1000000,
        hash.clone(),
        hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_update_availability_success() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    let _ = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        1000000,
        hash.clone(),
        hash,
    );
    
    // Update availability
    let result = contract.update_availability(env.clone(), 1, -8); // Booked, PST
    
    assert!(result.is_ok());
    
    let profile = contract.get_profile(env.clone(), mentor).unwrap();
    assert_eq!(profile.availability_status as u32, 1); // Booked
    assert_eq!(profile.timezone_offset, -8);
}

#[test]
fn test_update_availability_invalid_status() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    let _ = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        1000000,
        hash.clone(),
        hash,
    );
    
    // Update with invalid status
    let result = contract.update_availability(env.clone(), 99, 5);
    
    assert!(result.is_err());
}

#[test]
fn test_archive_and_restore_profile() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    mentor.mock_all_auths();
    let hash = Bytes::new(&env, &[1; 32]);
    let _ = contract.publish_profile(
        env.clone(),
        hash.clone(),
        5,
        0b111,
        1000000,
        hash.clone(),
        hash,
    );
    
    // Archive
    let result = contract.archive_profile(env.clone());
    assert!(result.is_ok());
    
    let profile = contract.get_profile(env.clone(), mentor.clone()).unwrap();
    assert_eq!(profile.availability_status as u32, 2); // Archived
    
    // Restore
    let result = contract.restore_profile(env.clone());
    assert!(result.is_ok());
    
    let profile = contract.get_profile(env.clone(), mentor).unwrap();
    assert_eq!(profile.availability_status as u32, 0); // Available
}

#[test]
fn test_get_nonexistent_profile_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let nonexistent = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend);
    
    let result = contract.get_profile(env, nonexistent);
    
    assert!(result.is_err());
}
