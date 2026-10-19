#![cfg(test)]

use mentor_discovery::{MentorDiscovery, MentorError};
use mentor_types::{DeliveryMode, QueryFilters};
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
    let registry = Address::random(&env);

    let result = MentorDiscovery::init(env.clone(), admin.clone(), registry.clone());

    assert!(result.is_ok());
}

#[test]
fn test_init_twice_fails() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin.clone(), registry.clone());
    let result = MentorDiscovery::init(env.clone(), admin.clone(), registry);

    assert_eq!(result, Err(MentorError::AlreadyInitialized));
}

#[test]
fn test_register_program_success() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin.clone(), registry);

    let topic_hash = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    let result = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic_hash.clone(),
        1700000000_u64,
        40_u32, // 40 hours
        10_u32, // 10 seats
        0_u8,   // Synchronous
        program_uri,
    );

    assert!(result.is_ok());

    let program_id = result.unwrap();
    assert_eq!(program_id, 1);

    // Verify program was registered
    let retrieved = MentorDiscovery::get_program(env.clone(), program_id);
    assert!(retrieved.is_some());

    let program = retrieved.unwrap();
    assert_eq!(program.program_id, 1);
    assert_eq!(program.max_seats, 10);
    assert_eq!(program.available_seats, 10);
}

#[test]
fn test_register_program_invalid_seats() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin, registry);

    let topic_hash = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    let result = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic_hash,
        1700000000_u64,
        40_u32,
        0_u32, // Invalid: 0 seats
        0_u8,
        program_uri,
    );

    assert_eq!(result, Err(MentorError::InvalidInput));
}

#[test]
fn test_list_programs() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin, registry);

    let topic_hash = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    // Register two programs
    let _ = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic_hash.clone(),
        1700000000_u64,
        40_u32,
        10_u32,
        0_u8,
        program_uri.clone(),
    );

    let _ = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic_hash,
        1701000000_u64,
        50_u32,
        15_u32,
        1_u8,
        program_uri,
    );

    let result = MentorDiscovery::list_programs(env.clone(), 0, 10);
    assert!(result.is_ok());

    let programs = result.unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs.get(0).unwrap().program_id, 1);
    assert_eq!(programs.get(1).unwrap().program_id, 2);
}

#[test]
fn test_query_programs_by_topic() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin, registry);

    let topic1 = BytesN::<32>::random(&env);
    let topic2 = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    // Register programs with different topics
    let _ = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic1.clone(),
        1700000000_u64,
        40_u32,
        10_u32,
        0_u8,
        program_uri.clone(),
    );

    let _ = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic2.clone(),
        1701000000_u64,
        50_u32,
        15_u32,
        1_u8,
        program_uri,
    );

    // Query by topic1
    let filters = QueryFilters {
        topic_hash: Some(topic1.clone()),
        min_seats: 0,
        mode: None,
        organization: None,
        cohort_start_min: None,
        cohort_start_max: None,
    };

    let result = MentorDiscovery::query_programs(env.clone(), filters, 0, 10);
    assert!(result.is_ok());

    let programs = result.unwrap();
    assert_eq!(programs.len(), 1);
    assert_eq!(programs.get(0).unwrap().program_id, 1);
}

#[test]
fn test_update_program_seats() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin.clone(), registry);

    let topic_hash = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    let result = MentorDiscovery::register_program(
        env.clone(),
        organization.clone(),
        topic_hash,
        1700000000_u64,
        40_u32,
        10_u32,
        0_u8,
        program_uri,
    );

    assert!(result.is_ok());
    let program_id = result.unwrap();

    // Update seats
    let update_result = MentorDiscovery::update_program_seats(
        env.clone(),
        admin.clone(),
        program_id,
        5_u32, // Reduce to 5 seats
    );

    assert!(update_result.is_ok());

    // Verify update
    let retrieved = MentorDiscovery::get_program(env.clone(), program_id);
    assert!(retrieved.is_some());

    let program = retrieved.unwrap();
    assert_eq!(program.available_seats, 5);
}

#[test]
fn test_get_program_cache_ttl() {
    let env = setup_env();
    let admin = Address::random(&env);
    let registry = Address::random(&env);
    let organization = Address::random(&env);

    let _ = MentorDiscovery::init(env.clone(), admin, registry);

    let topic_hash = BytesN::<32>::random(&env);
    let program_uri = SorobanString::from_small_str("ipfs://QmProgram");

    let far_future = env.ledger().timestamp() + 86400 * 60; // 60 days from now

    let result = MentorDiscovery::register_program(
        env.clone(),
        organization,
        topic_hash,
        far_future,
        40_u32,
        10_u32,
        0_u8,
        program_uri,
    );

    let program_id = result.unwrap();

    let ttl_result = MentorDiscovery::get_program_cache_ttl(env, program_id);
    assert!(ttl_result.is_ok());

    let ttl = ttl_result.unwrap();
    assert!(ttl > 0);
}
