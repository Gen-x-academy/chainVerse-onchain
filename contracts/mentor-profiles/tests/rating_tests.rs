#![cfg(test)]
mod utils;

use soroban_sdk::{Address, Bytes, Env};
use mentor_profiles::MentorProfilesContract;

#[test]
fn test_record_rating_success() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
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
    
    // Record rating
    backend.mock_all_auths();
    let review_hash = Bytes::new(&env, &[2; 32]);
    let engagement_hash = Bytes::new(&env, &[3; 32]);
    
    let result = contract.record_rating(
        env.clone(),
        mentor.clone(),
        5,
        review_hash,
        engagement_hash,
    );
    
    assert!(result.is_ok());
    
    // Verify rating was recorded
    let profile = contract.get_profile(env.clone(), mentor).unwrap();
    assert_eq!(profile.rating_count, 1);
    assert_eq!(profile.rating_sum, 5);
}

#[test]
fn test_record_multiple_ratings() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
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
    
    // Record multiple ratings
    backend.mock_all_auths();
    for i in 1..=5 {
        let review_hash = Bytes::new(&env, &[(i as u8); 32]);
        let engagement_hash = Bytes::new(&env, &[(i as u8 + 10); 32]);
        
        let result = contract.record_rating(
            env.clone(),
            mentor.clone(),
            i as u32,
            review_hash,
            engagement_hash,
        );
        assert!(result.is_ok());
    }
    
    // Verify all ratings were recorded
    let profile = contract.get_profile(env.clone(), mentor).unwrap();
    assert_eq!(profile.rating_count, 5);
    assert_eq!(profile.rating_sum, 15); // 1+2+3+4+5
}

#[test]
fn test_record_rating_invalid_score() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
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
    
    backend.mock_all_auths();
    let review_hash = Bytes::new(&env, &[2; 32]);
    let engagement_hash = Bytes::new(&env, &[3; 32]);
    
    // Try invalid scores (0, 6)
    let result = contract.record_rating(
        env.clone(),
        mentor.clone(),
        0,
        review_hash.clone(),
        engagement_hash.clone(),
    );
    assert!(result.is_err());
    
    let result = contract.record_rating(
        env.clone(),
        mentor.clone(),
        6,
        review_hash,
        engagement_hash,
    );
    assert!(result.is_err());
}

#[test]
fn test_record_rating_unauthorized_backend() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    
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
    
    // Try to record rating with unauthorized address
    unauthorized.mock_all_auths();
    let review_hash = Bytes::new(&env, &[2; 32]);
    let engagement_hash = Bytes::new(&env, &[3; 32]);
    
    let result = contract.record_rating(
        env.clone(),
        mentor,
        5,
        review_hash,
        engagement_hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_record_rating_invalid_hash_length() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
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
    
    backend.mock_all_auths();
    let invalid_hash = Bytes::new(&env, &[2; 16]); // Wrong length
    let valid_hash = Bytes::new(&env, &[3; 32]);
    
    let result = contract.record_rating(
        env.clone(),
        mentor,
        5,
        invalid_hash,
        valid_hash,
    );
    
    assert!(result.is_err());
}

#[test]
fn test_reputation_score_calculation() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let mentor = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
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
    
    // Record ratings that should give average of 4
    backend.mock_all_auths();
    for i in 0..4 {
        let review_hash = Bytes::new(&env, &[(i as u8); 32]);
        let engagement_hash = Bytes::new(&env, &[(i as u8 + 10); 32]);
        
        let _ = contract.record_rating(
            env.clone(),
            mentor.clone(),
            4,
            review_hash,
            engagement_hash,
        );
    }
    
    let profile = contract.get_profile(env.clone(), mentor).unwrap();
    assert_eq!(profile.rating_count, 4);
    assert_eq!(profile.rating_sum, 16);
    
    // Reputation should be min(avg_rating * multiplier, 100)
    // avg = 4, multiplier = 10 (default), so 40
    assert_eq!(profile.reputation_score, 40);
}

#[test]
fn test_record_rating_nonexistent_profile() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let nonexistent = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin, backend.clone());
    
    backend.mock_all_auths();
    let review_hash = Bytes::new(&env, &[2; 32]);
    let engagement_hash = Bytes::new(&env, &[3; 32]);
    
    let result = contract.record_rating(
        env.clone(),
        nonexistent,
        5,
        review_hash,
        engagement_hash,
    );
    
    assert!(result.is_err());
}
