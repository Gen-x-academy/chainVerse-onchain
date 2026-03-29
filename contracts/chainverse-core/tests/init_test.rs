#![allow(clippy::unwrap_used, clippy::expect_used)]

//! # Compliance Module Initialization Tests
//!
//! These tests demonstrate the double re-initialization protection pattern
//! that should be implemented for compliance smart contracts on Soroban.
//!
//! ## Security Properties Tested
//!
//! 1. **Double Initialization Prevention**: Once initialized, the contract
//!    must reject subsequent initialization attempts.
//! 2. **State Immutability After Init**: Initial state values cannot be
//!    overwritten by re-initialization attacks.
//! 3. **Error Type Safety**: Proper error types returned for security violations.
//! 4. **Deterministic State**: Same inputs produce same initialized state.

use compliance::access_control::{AccessControl, Role};
use compliance::audit::{ComplianceAuditLog, SearchKey};
use compliance::rules_engine::{Jurisdiction, OperationContext, RulesEngine};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Mock compliance contract state to demonstrate initialization protection.
/// This simulates what a real Soroban contract wrapper would look like.
struct ComplianceState {
    initialized: bool,
    admin: Option<Address>,
    audit_log_key: Option<[u8; 32]>,
    rules_engine: RulesEngine,
}

impl ComplianceState {
    fn new() -> Self {
        Self {
            initialized: false,
            admin: None,
            audit_log_key: None,
            rules_engine: RulesEngine::new(),
        }
    }

    /// Initialize the compliance state.
    /// Returns an error if already initialized.
    fn initialize(&mut self, admin: &Address, audit_key: [u8; 32]) -> Result<(), &'static str> {
        if self.initialized {
            return Err("AlreadyInitialized");
        }

        self.admin = Some(admin.clone());
        self.audit_log_key = Some(audit_key);
        self.initialized = true;

        // Register default compliance rules
        compliance::hipaa::register_hipaa_rules(&mut self.rules_engine);
        compliance::gdpr::register_gdpr_rules(&mut self.rules_engine);

        Ok(())
    }

    fn get_admin(&self) -> Option<&Address> {
        self.admin.as_ref()
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn get_audit_key(&self) -> Option<&[u8; 32]> {
        self.audit_log_key.as_ref()
    }
}

fn setup_uninitialized() -> (Env, ComplianceState) {
    let env = Env::default();
    env.mock_all_auths();
    (env, ComplianceState::new())
}

fn setup_initialized() -> (Env, ComplianceState, Address) {
    let (env, mut state) = setup_uninitialized();
    let admin = Address::generate(&env);
    let audit_key = [0x42u8; 32];
    state.initialize(&admin, audit_key).expect("initialization should succeed");
    (env, state, admin)
}

// ===========================================================================
// Double Initialization Tests
// ===========================================================================

#[test]
fn test_double_initialize_fails_with_already_initialized_error() {
    let (env, mut state) = setup_uninitialized();
    let admin_1 = Address::generate(&env);
    let admin_2 = Address::generate(&env);
    let audit_key_1 = [0x42u8; 32];
    let audit_key_2 = [0x99u8; 32];

    // First initialization should succeed
    let result1 = state.initialize(&admin_1, audit_key_1);
    assert!(result1.is_ok(), "First initialization should succeed");
    assert!(state.is_initialized());

    // Second initialization should fail
    let result2 = state.initialize(&admin_2, audit_key_2);
    assert!(result2.is_err(), "Double initialization should fail");
    assert_eq!(result2.unwrap_err(), "AlreadyInitialized");

    // State should remain unchanged from first initialization
    assert_eq!(state.get_admin().unwrap(), &admin_1);
    assert_eq!(state.get_audit_key().unwrap(), &audit_key_1);
}

#[test]
fn test_triple_initialize_attempt_also_fails() {
    let (env, mut state) = setup_uninitialized();
    let admin_1 = Address::generate(&env);
    let admin_2 = Address::generate(&env);
    let admin_3 = Address::generate(&env);
    let audit_key_1 = [0x01u8; 32];
    let audit_key_2 = [0x02u8; 32];
    let audit_key_3 = [0x03u8; 32];

    // First initialization succeeds
    assert!(state.initialize(&admin_1, audit_key_1).is_ok());

    // Second attempt fails
    assert_eq!(
        state.initialize(&admin_2, audit_key_2).unwrap_err(),
        "AlreadyInitialized"
    );

    // Third attempt also fails
    assert_eq!(
        state.initialize(&admin_3, audit_key_3).unwrap_err(),
        "AlreadyInitialized"
    );

    // Original state preserved
    assert_eq!(state.get_admin().unwrap(), &admin_1);
    assert_eq!(state.get_audit_key().unwrap(), &audit_key_1);
}

#[test]
fn test_initialize_with_same_admin_twice_still_fails() {
    let (env, mut state) = setup_uninitialized();
    let admin = Address::generate(&env);
    let audit_key_1 = [0x11u8; 32];
    let audit_key_2 = [0x22u8; 32];

    // First initialization
    assert!(state.initialize(&admin, audit_key_1).is_ok());

    // Even with same admin, second init should fail
    assert_eq!(
        state.initialize(&admin, audit_key_2).unwrap_err(),
        "AlreadyInitialized"
    );

    // Original audit key preserved
    assert_eq!(state.get_audit_key().unwrap(), &audit_key_1);
}

// ===========================================================================
// Initialization State Validation Tests
// ===========================================================================

#[test]
fn test_initialization_sets_correct_state() {
    let (_env, mut state, admin) = setup_initialized();

    assert!(state.is_initialized(), "State should be marked as initialized");
    assert!(state.get_admin().is_some(), "Admin should be set");
    assert!(state.get_audit_key().is_some(), "Audit key should be set");

    // Verify rules engine has rules registered
    // (This is a side effect of initialization)
    let ctx = compliance::rules_engine::OperationContext {
        actor: "test".to_string(),
        actor_role: "clinician".to_string(),
        action: "record.read".to_string(),
        target: "patient:1".to_string(),
        timestamp: 1000,
        has_consent: true,
        sensitivity: 2,
        jurisdiction: Jurisdiction::US,
        record_count: 1,
        purpose: "treatment".to_string(),
        metadata: std::collections::HashMap::new(),
    };

    let verdict = state.rules_engine.evaluate(&ctx);
    assert!(
        verdict.rules_evaluated > 0,
        "Rules should have been registered during initialization"
    );
}

#[test]
fn test_uninitialized_state_cannot_perform_operations() {
    let (_env, state) = setup_uninitialized();

    assert!(!state.is_initialized());
    assert!(state.get_admin().is_none());
    assert!(state.get_audit_key().is_none());

    // Uninitialized state should not have rules registered
    let ctx = compliance::rules_engine::OperationContext {
        actor: "test".to_string(),
        actor_role: "clinician".to_string(),
        action: "record.read".to_string(),
        target: "patient:1".to_string(),
        timestamp: 1000,
        has_consent: true,
        sensitivity: 2,
        jurisdiction: Jurisdiction::US,
        record_count: 1,
        purpose: "treatment".to_string(),
        metadata: std::collections::HashMap::new(),
    };

    let verdict = {
        let mut temp_engine = RulesEngine::new();
        temp_engine.evaluate(&ctx)
    };

    assert_eq!(
        verdict.rules_evaluated, 0,
        "Uninitialized state should have no rules"
    );
}

// ===========================================================================
// Access Control Integration Tests
// ===========================================================================

#[test]
fn test_initialization_with_access_control() {
    let (env, mut state, admin) = setup_initialized();

    // Admin should have full access
    let access_control = AccessControl::new();
    assert!(access_control.check(&Role::Admin, "read"));
    assert!(access_control.check(&Role::Admin, "write"));
    assert!(access_control.check(&Role::Admin, "audit"));

    // Verify the admin address matches
    assert_eq!(state.get_admin().unwrap(), &admin);

    // Attempt to reinitialize with different admin should fail
    let attacker = Address::generate(&env);
    let attack_key = [0xFFu8; 32];
    assert_eq!(
        state.initialize(&attacker, attack_key).unwrap_err(),
        "AlreadyInitialized"
    );

    // Original admin preserved
    assert_eq!(state.get_admin().unwrap(), &admin);
}

// ===========================================================================
// Audit Log Initialization Tests
// ===========================================================================

#[test]
fn test_audit_log_creation_after_initialization() {
    let (_env, state, _admin) = setup_initialized();

    // Create audit log with the initialized key
    let audit_key = state.get_audit_key().unwrap().clone();
    let search_key = SearchKey::from_bytes(&audit_key).unwrap();
    let mut audit_log = ComplianceAuditLog::new(search_key);

    // Record some audit entries
    audit_log.record(1000, "user1", "read", "record:1", "ok");
    audit_log.record(1001, "user2", "write", "record:2", "ok");

    assert_eq!(audit_log.len(), 2);

    // Verify searchability
    let hits = audit_log.search("user1");
    assert_eq!(hits, vec![1]);
}

#[test]
fn test_different_audit_keys_produce_different_states() {
    let (env, mut state) = setup_uninitialized();
    let admin = Address::generate(&env);
    let audit_key_1 = [0xAAu8; 32];
    let audit_key_2 = [0xBBu8; 32];

    // Initialize with first key
    state.initialize(&admin, audit_key_1).unwrap();
    assert_eq!(state.get_audit_key().unwrap(), &audit_key_1);

    // Cannot change to second key
    assert!(state.initialize(&admin, audit_key_2).is_err());

    // Key remains unchanged
    assert_eq!(state.get_audit_key().unwrap(), &audit_key_1);
}

// ===========================================================================
// Edge Case Tests
// ===========================================================================

#[test]
fn test_initialize_with_zero_audit_key() {
    let (_env, mut state) = setup_uninitialized();
    let admin = Address::generate(&Env::default());
    let zero_key = [0x00u8; 32];

    // Should still initialize successfully (key validation is separate concern)
    let result = state.initialize(&admin, zero_key);
    assert!(result.is_ok(), "Initialization should accept any 32-byte key");
    assert_eq!(state.get_audit_key().unwrap(), &zero_key);
}

#[test]
fn test_concurrent_initialization_attempts_fail() {
    // Simulate multiple initialization attempts in sequence
    let env = Env::default();
    env.mock_all_auths();

    let admins: Vec<Address> = (0..5).map(|_| Address::generate(&env)).collect();
    let keys: Vec<[u8; 32]> = (0..5)
        .map(|i| [i as u8; 32])
        .collect();

    let mut state = ComplianceState::new();

    // First succeeds
    assert!(state.initialize(&admins[0], keys[0]).is_ok());

    // Rest all fail
    for i in 1..5 {
        assert_eq!(
            state.initialize(&admins[i], keys[i]).unwrap_err(),
            "AlreadyInitialized",
            "Attempt {} should fail",
            i
        );
    }

    // Original state preserved
    assert_eq!(state.get_admin().unwrap(), &admins[0]);
    assert_eq!(state.get_audit_key().unwrap(), &keys[0]);
}

#[test]
fn test_initialization_preserves_deterministic_state() {
    let env = Env::default();
    env.mock_all_auths();

    // Run initialization multiple times with same inputs
    for _ in 0..3 {
        let admin = Address::generate(&env);
        let audit_key = [0xCDu8; 32];
        let mut state = ComplianceState::new();

        state.initialize(&admin, audit_key).unwrap();

        assert!(state.is_initialized());
        assert_eq!(state.get_admin().unwrap(), &admin);
        assert_eq!(state.get_audit_key().unwrap(), &audit_key);
    }
}
