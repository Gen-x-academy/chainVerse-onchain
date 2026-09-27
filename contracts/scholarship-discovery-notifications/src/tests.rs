#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Map, String, Symbol, Vec};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> Address {
    let admin = Address::generate(env);
    ScholarshipDiscoveryNotificationsContract::initialize(env.clone(), admin.clone()).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);

    assert!(ScholarshipDiscoveryNotificationsContract::initialize(env.clone(), admin.clone()).is_ok());
    assert_eq!(
        ScholarshipDiscoveryNotificationsContract::initialize(env, admin),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_search_programs_pagination() {
    let env = create_test_env();
    let admin = setup_contract(&env);

    let params = SearchParams {
        filters: SearchFilters {
            sponsor_id: None,
            eligibility_criteria: None,
            min_award: Some(1000),
            max_award: Some(10000),
            funding_type: Some(FundingType::Scholarship),
            deadline_from: None,
            deadline_to: None,
            term: None,
            status: Some(ProgramStatus::Published),
            currency: Some(Symbol::new(&env, "XLM")),
        },
        sort_by: SortBy::DeadlineAsc,
        page_size: 20,
        cursor: None,
    };

    let result = ScholarshipDiscoveryNotificationsContract::search_programs(env, params).unwrap();
    assert_eq!(result.programs.len(), 0);
    assert_eq!(result.total_count, 0);
}

#[test]
fn test_search_programs_page_size_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);

    // Page size too large
    let params = SearchParams {
        filters: SearchFilters {
            sponsor_id: None,
            eligibility_criteria: None,
            min_award: None,
            max_award: None,
            funding_type: None,
            deadline_from: None,
            deadline_to: None,
            term: None,
            status: None,
            currency: None,
        },
        sort_by: SortBy::DeadlineAsc,
        page_size: MAX_SEARCH_RESULTS + 1,
        cursor: None,
    };

    let result = ScholarshipDiscoveryNotificationsContract::search_programs(env, params);
    assert_eq!(result, Err(ContractError::PageSizeExceeded));

    // Page size zero
    let params = SearchParams {
        filters: SearchFilters {
            sponsor_id: None,
            eligibility_criteria: None,
            min_award: None,
            max_award: None,
            funding_type: None,
            deadline_from: None,
            deadline_to: None,
            term: None,
            status: None,
            currency: None,
        },
        sort_by: SortBy::DeadlineAsc,
        page_size: 0,
        cursor: None,
    };

    let result = ScholarshipDiscoveryNotificationsContract::search_programs(env, params);
    assert_eq!(result, Err(ContractError::PageSizeExceeded));
}

#[test]
fn test_get_filter_options() {
    let env = create_test_env();
    let admin = setup_contract(&env);

    let options = ScholarshipDiscoveryNotificationsContract::get_filter_options(env).unwrap();
    assert_eq!(options.funding_types.len(), 6);
    assert_eq!(options.statuses.len(), 5);
}

#[test]
fn test_update_student_profile() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    let mut demographic = Map::new(&env);
    demographic.set(String::from_str(&env, "country"), String::from_str(&env, "US"));

    let profile = ScholarshipDiscoveryNotificationsContract::update_student_profile(
        env.clone(),
        student.clone(),
        Vec::from_array(&env, [
            String::from_str(&env, "Computer Science"),
            String::from_str(&env, "AI"),
        ]),
        String::from_str(&env, "Graduate"),
        String::from_str(&env, "Computer Science"),
        Some(String::from_str(&env, "3.8")),
        String::from_str(&env, "San Francisco"),
        demographic,
    ).unwrap();

    assert_eq!(profile.student, student);
    assert_eq!(profile.interests.len(), 2);
    assert_eq!(profile.academic_level, String::from_str(&env, "Graduate"));
}

#[test]
fn test_get_recommendations_cold_start() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    let result = ScholarshipDiscoveryNotificationsContract::get_recommendations(env, student.clone()).unwrap();
    assert!(result.cold_start);
    assert_eq!(result.student, student);
    assert_eq!(result.recommendations.len(), 0);
}

#[test]
fn test_dismiss_recommendation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);
    let program_id = BytesN::from_array(&env, &[2u8; 32]);

    let result = ScholarshipDiscoveryNotificationsContract::dismiss_recommendation(env, student, program_id);
    assert!(result.is_ok());
}

#[test]
fn test_emit_notification() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = BytesN::from_array(&env, &[2u8; 32]);
    let recipient = Address::generate(&env);

    let event = ScholarshipDiscoveryNotificationsContract::emit_notification(
        env.clone(),
        admin.clone(),
        Some(program_id.clone()),
        Some(recipient.clone()),
        NotificationEventType::ApplicationSubmitted,
        String::from_str(&env, "Application Received"),
        String::from_str(&env, "Your application has been received."),
        Some(BytesN::from_array(&env, &[3u8; 32])),
        Some(String::from_str(&env, "application")),
        NotificationPriority::Normal,
    ).unwrap();

    assert_eq!(event.program_id, Some(program_id));
    assert_eq!(event.recipient, Some(recipient));
    assert_eq!(event.event_type, NotificationEventType::ApplicationSubmitted);
    assert_eq!(event.priority, NotificationPriority::Normal);
    assert!(!event.deduplication_key.is_empty());
}

#[test]
fn test_emit_broadcast_notification() {
    let env = create_test_env();
    let admin = setup_contract(&env);

    // Broadcast (no specific recipient)
    let event = ScholarshipDiscoveryNotificationsContract::emit_notification(
        env.clone(),
        admin.clone(),
        None,
        None,
        NotificationEventType::ApplicationOpened,
        String::from_str(&env, "New Scholarship Open"),
        String::from_str(&env, "A new scholarship is now accepting applications."),
        None,
        None,
        NotificationPriority::High,
    ).unwrap();

    assert_eq!(event.program_id, None);
    assert_eq!(event.recipient, None);
    assert_eq!(event.priority, NotificationPriority::High);
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipDiscoveryNotificationsContract::version(env), 1);
}