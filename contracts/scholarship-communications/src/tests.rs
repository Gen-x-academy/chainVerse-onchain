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
    ScholarshipCommunicationsContract::initialize(env.clone(), admin.clone()).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);

    assert!(ScholarshipCommunicationsContract::initialize(env.clone(), admin.clone()).is_ok());
    assert_eq!(
        ScholarshipCommunicationsContract::initialize(env, admin),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_set_get_preferences() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let user = Address::generate(&env);
    let program_id = create_program_id(&env);

    let mut channels = Map::new(&env);
    channels.set(EventType::ApplicationSubmitted, Vec::from_array(&env, [Channel::InApp, Channel::Email]));
    channels.set(EventType::DecisionReleased, Vec::from_array(&env, [Channel::InApp, Channel::Push]));

    let prefs = ScholarshipCommunicationsContract::set_preferences(
        env.clone(),
        user.clone(),
        program_id.clone(),
        channels.clone(),
        Some(22), // quiet_hours_start
        Some(8),  // quiet_hours_end
        -300,     // timezone_offset_minutes (EST)
        false,    // mandatory_only
    ).unwrap();

    assert_eq!(prefs.user, user);
    assert_eq!(prefs.program_id, program_id);
    assert_eq!(prefs.timezone_offset_minutes, -300);
    assert_eq!(prefs.mandatory_only, false);

    // Get preferences
    let retrieved = ScholarshipCommunicationsContract::get_preferences(env, user, program_id).unwrap();
    assert_eq!(retrieved.channels.get(EventType::ApplicationSubmitted).unwrap().len(), 2);
}

#[test]
fn test_is_channel_enabled() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let user = Address::generate(&env);
    let program_id = create_program_id(&env);

    let mut channels = Map::new(&env);
    channels.set(EventType::ApplicationSubmitted, Vec::from_array(&env, [Channel::InApp, Channel::Email]));

    ScholarshipCommunicationsContract::set_preferences(
        env.clone(),
        user.clone(),
        program_id.clone(),
        channels,
        None, None, 0, false,
    ).unwrap();

    // Enabled channel
    let enabled = ScholarshipCommunicationsContract::is_channel_enabled(
        env.clone(),
        user.clone(),
        program_id.clone(),
        EventType::ApplicationSubmitted,
        Channel::InApp,
    ).unwrap();
    assert!(enabled);

    // Disabled channel
    let disabled = ScholarshipCommunicationsContract::is_channel_enabled(
        env.clone(),
        user,
        program_id,
        EventType::ApplicationSubmitted,
        Channel::Sms,
    ).unwrap();
    assert!(!disabled);
}

#[test]
fn test_preferences_quiet_hours_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let user = Address::generate(&env);
    let program_id = create_program_id(&env);

    let channels = Map::new(&env);

    // Invalid quiet hours (>= 24)
    let result = ScholarshipCommunicationsContract::set_preferences(
        env.clone(),
        user.clone(),
        program_id.clone(),
        channels.clone(),
        Some(25), // invalid
        Some(8),
        0,
        false,
    );
    assert_eq!(result, Err(ContractError::InvalidTimezone));

    // Invalid timezone offset
    let result = ScholarshipCommunicationsContract::set_preferences(
        env,
        user,
        program_id,
        channels,
        None,
        None,
        1000, // invalid
        false,
    );
    assert_eq!(result, Err(ContractError::InvalidTimezone));
}

#[test]
fn test_create_template() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let variables = Vec::new(&env);

    let template = ScholarshipCommunicationsContract::create_template(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "en-US"),
        EventType::ApplicationSubmitted,
        Channel::Email,
        String::from_str(&env, "Application Received"),
        String::from_str(&env, "Thank you for submitting your application!"),
        variables,
    ).unwrap();

    assert_eq!(template.program_id, program_id);
    assert_eq!(template.version, 1);
    assert!(!template.is_active); // requires approval

    // Approve template
    ScholarshipCommunicationsContract::approve_template(env.clone(), admin.clone(), template.id.clone()).unwrap();

    let approved = ScholarshipCommunicationsContract::get_template(env, template.id).unwrap();
    assert!(approved.is_active);
    assert!(approved.approved_at.is_some());
}

#[test]
fn test_update_template_creates_new_version() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let template = ScholarshipCommunicationsContract::create_template(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "en-US"),
        EventType::ApplicationSubmitted,
        Channel::Email,
        String::from_str(&env, "Subject v1"),
        String::from_str(&env, "Body v1"),
        Vec::new(&env),
    ).unwrap();

    // Update template
    let updated = ScholarshipCommunicationsContract::update_template(
        env.clone(),
        admin.clone(),
        template.id.clone(),
        Some(String::from_str(&env, "Subject v2")),
        Some(String::from_str(&env, "Body v2")),
        None,
    ).unwrap();

    assert_eq!(updated.version, 2);
    assert!(!updated.is_active); // requires re-approval

    // Original version still accessible
    let v1 = ScholarshipCommunicationsContract::get_template_version(env.clone(), template.id.clone(), 1).unwrap();
    assert_eq!(v1.version, 1);
    assert_eq!(v1.subject, String::from_str(&env, "Subject v1"));
}

#[test]
fn test_template_variable_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let mut var = TemplateVariable {
        name: String::from_str(&env, "student_name"),
        description: String::from_str(&env, "Student's full name"),
        required: true,
        variable_type: VariableType::String,
        validation_regex: None,
        max_length: Some(100),
    };
    let variables = Vec::from_array(&env, [var]);

    let template = ScholarshipCommunicationsContract::create_template(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "en-US"),
        EventType::ApplicationSubmitted,
        Channel::Email,
        String::from_str(&env, "Subject"),
        String::from_str(&env, "Body"),
        variables,
    ).unwrap();

    // Validate with correct variable
    let mut vars = Map::new(&env);
    vars.set(String::from_str(&env, "student_name"), String::from_str(&env, "John Doe"));
    let result = ScholarshipCommunicationsContract::validate_template_variables(env.clone(), template.id, vars);
    assert!(result.is_ok());

    // Validate with missing required variable
    let mut vars = Map::new(&env);
    let result = ScholarshipCommunicationsContract::validate_template_variables(env.clone(), template.id, vars);
    assert_eq!(result, Err(ContractError::VariableValidationFailed));

    // Validate with variable exceeding max length
    let mut vars = Map::new(&env);
    vars.set(String::from_str(&env, "student_name"), String::from_str(&env, "A".repeat(150)));
    let result = ScholarshipCommunicationsContract::validate_template_variables(env, template.id, vars);
    assert_eq!(result, Err(ContractError::VariableValidationFailed));
}

#[test]
fn test_schedule_reminder() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let user = Address::generate(&env);

    // Create and approve a template first
    let template = ScholarshipCommunicationsContract::create_template(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "en-US"),
        EventType::ApplicationClosing,
        Channel::Email,
        String::from_str(&env, "Application Closing Soon"),
        String::from_str(&env, "Don't forget to submit!"),
        Vec::new(&env),
    ).unwrap();
    ScholarshipCommunicationsContract::approve_template(env.clone(), admin.clone(), template.id.clone()).unwrap();

    let scheduled_at = env.ledger().timestamp() + 86400; // 1 day from now

    let reminder = ScholarshipCommunicationsContract::schedule_reminder(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        user.clone(),
        EventType::ApplicationClosing,
        Channel::Email,
        template.id,
        scheduled_at,
        -300, // EST
    ).unwrap();

    assert_eq!(reminder.user, user);
    assert_eq!(reminder.event_type, EventType::ApplicationClosing);
    assert_eq!(reminder.status, ReminderStatus::Scheduled);
    assert_eq!(reminder.scheduled_at, scheduled_at);
    assert!(!reminder.deduplication_key.is_empty());
}

#[test]
fn test_cancel_reminder() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let user = Address::generate(&env);

    let template = ScholarshipCommunicationsContract::create_template(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        String::from_str(&env, "en-US"),
        EventType::ApplicationClosing,
        Channel::Email,
        String::from_str(&env, "Subject"),
        String::from_str(&env, "Body"),
        Vec::new(&env),
    ).unwrap();
    ScholarshipCommunicationsContract::approve_template(env.clone(), admin.clone(), template.id.clone()).unwrap();

    let reminder = ScholarshipCommunicationsContract::schedule_reminder(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        user.clone(),
        EventType::ApplicationClosing,
        Channel::Email,
        template.id,
        env.ledger().timestamp() + 86400,
        0,
    ).unwrap();

    // Cancel reminder
    ScholarshipCommunicationsContract::cancel_reminder(env.clone(), user.clone(), reminder.id.clone()).unwrap();

    let cancelled = ScholarshipCommunicationsContract::get_reminder(env, reminder.id).unwrap();
    assert_eq!(cancelled.status, ReminderStatus::Cancelled);
    assert!(cancelled.cancelled_at.is_some());
}

#[test]
fn test_record_accessibility_audit() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let passed = Vec::from_array(&env, [
        String::from_str(&env, "keyboard-navigation"),
        String::from_str(&env, "focus-indicators"),
        String::from_str(&env, "color-contrast"),
    ]);
    let failed = Vec::from_array(&env, [
        String::from_str(&env, "aria-labels"),
    ]);
    let warnings = Vec::from_array(&env, [
        String::from_str(&env, "reduce-motion-support"),
    ]);

    let audit = ScholarshipCommunicationsContract::record_accessibility_audit(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        None,
        JourneyStage::Application,
        WcagLevel::AA,
        passed,
        failed,
        warnings,
    ).unwrap();

    assert_eq!(audit.program_id, program_id);
    assert_eq!(audit.journey_stage, JourneyStage::Application);
    assert_eq!(audit.wcag_level, WcagLevel::AA);
    assert_eq!(audit.passed_checks.len(), 3);
    assert_eq!(audit.failed_checks.len(), 1);
    assert_eq!(audit.warnings.len(), 1);
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipCommunicationsContract::version(env), 1);
}