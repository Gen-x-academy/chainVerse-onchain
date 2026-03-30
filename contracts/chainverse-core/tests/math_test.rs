#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, symbol_short, testutils::Address as _, Address, Env, Symbol, Vec, Map, String
};
use crate::rules_engine::{RulesEngine, OperationContext, Jurisdiction, Severity};
use crate::breach_detector::{BreachDetector, AccessEvent, BreachDetectorConfig};
use crate::retention::{RetentionManager};

#[contract]
pub struct ComplianceMockContract;

#[contractimpl]
impl ComplianceMockContract {
    /// Wrapper to test RulesEngine::evaluate with boundary values
    pub fn test_rules_eval(
        _env: Env,
        timestamp: u64,
        record_count: u32,
        sensitivity: u32,
        has_consent: bool,
    ) -> bool {
        let mut engine = RulesEngine::new();
        
        // Register a rule that uses the parameters to check for bounds-related issues
        engine.register_rule(crate::rules_engine::ComplianceRule {
            id: "MATH-001".into(),
            name: "Boundary Test".into(),
            jurisdictions: vec![Jurisdiction::Both],
            severity: Severity::Critical,
            remediation: "N/A".into(),
            evaluate: Box::new(move |ctx| {
                // Perform some math that might overflow if not handled correctly
                // ctx.timestamp is u64
                let _seconds_in_hour = 3600;
                let _hour = (ctx.timestamp / _seconds_in_hour) % 24;
                ctx.record_count <= 1_000_000 
            }),
        });

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("encrypted".into(), "true".into());

        let ctx = OperationContext {
            actor: "test_actor".into(),
            actor_role: "clinician".into(),
            action: "record.read".into(),
            target: "patient:1".into(),
            timestamp,
            has_consent,
            sensitivity,
            jurisdiction: Jurisdiction::Both,
            record_count,
            purpose: "treatment".into(),
            metadata,
        };

        engine.evaluate(&ctx).allowed
    }

    /// Wrapper to test BreachDetector with boundary values
    pub fn test_breach_detector(
        _env: Env,
        timestamp: u64,
        record_count: u32,
    ) -> u32 {
        let config = BreachDetectorConfig::default();
        let mut detector = BreachDetector::with_config(config);
        
        let event = AccessEvent {
            actor: "test_actor".into(),
            actor_role: "clinician".into(),
            action: "data.export".into(),
            target: "patient:1".into(),
            timestamp,
            record_count,
            sensitivity: 3,
            success: true,
        };

        let alerts = detector.record_event(event);
        alerts.len() as u32
    }

    /// Explicit check for i128 overflow/underflow handling in compliance calculations
    pub fn test_i128_math_safety(
        _env: Env,
        val1: i128,
        val2: i128,
        op: u32, // 0: add, 1: sub, 2: mul, 3: div
    ) -> i128 {
        match op {
            0 => val1.saturating_add(val2),
            1 => val1.saturating_sub(val2),
            2 => val1.saturating_mul(val2),
            3 => {
                if val2 == 0 {
                    0
                } else {
                    val1.saturating_div(val2)
                }
            },
            _ => 0,
        }
    }

    /// Check retention logic boundary handling
    pub fn test_retention_purge(
        _env: Env,
        created: u64,
        policy_period: u64,
        now: u64,
    ) -> bool {
        let mut manager = RetentionManager::new(now);
        manager.add_policy("TEST", policy_period);
        manager.should_purge(created, "TEST", now)
    }
}

#[test]
fn test_timestamp_math_boundaries() {
    let env = Env::default();
    let contract_id = env.register(ComplianceMockContract, ());
    let client = ComplianceMockContractClient::new(&env, &contract_id);

    // Test with u64::MAX timestamp
    let max_ts = u64::MAX;
    let allowed = client.test_rules_eval(&max_ts, &1, &3, &true);
    assert!(allowed);

    // Test with very large record count
    let max_rc = u32::MAX;
    let allowed_large_rc = client.test_rules_eval(&1000, &max_rc, &3, &true);
    // Our rule allows up to 1M, so this should be false but NOT panic
    assert!(!allowed_large_rc);
}

#[test]
fn test_breach_detector_boundaries() {
    let env = Env::default();
    let contract_id = env.register(ComplianceMockContract, ());
    let client = ComplianceMockContractClient::new(&env, &contract_id);

    // Test breach detector with u32::MAX records
    let alerts_count = client.test_breach_detector(&1000, &u32::MAX);
    assert!(alerts_count > 0);

    // Test with 0 timestamp
    let alerts_zero_ts = client.test_breach_detector(&0, &1);
    assert_eq!(alerts_zero_ts, 0);
}

#[test]
fn test_i128_overflow_protection() {
    let env = Env::default();
    let contract_id = env.register(ComplianceMockContract, ());
    let client = ComplianceMockContractClient::new(&env, &contract_id);

    // Test addition overflow: i128::MAX + 1 should saturate to i128::MAX
    let res = client.test_i128_math_safety(&i128::MAX, &1, &0);
    assert_eq!(res, i128::MAX);

    // Test multiplication overflow: i128::MAX * 2 should saturate to i128::MAX
    let res_mul = client.test_i128_math_safety(&i128::MAX, &2, &2);
    assert_eq!(res_mul, i128::MAX);

    // Test subtraction underflow: i128::MIN - 1 should saturate to i128::MIN
    let res_sub = client.test_i128_math_safety(&i128::MIN, &1, &1);
    assert_eq!(res_sub, i128::MIN);
}

#[test]
fn test_retention_edge_cases() {
    let env = Env::default();
    let contract_id = env.register(ComplianceMockContract, ());
    let client = ComplianceMockContractClient::new(&env, &contract_id);

    // Test retention overflow: created + period > u64::MAX
    // created = u64::MAX, period = 1000
    // should not panic due to saturating_add in RetentionManager
    let should_purge = client.test_retention_purge(&u64::MAX, &1000, &u64::MAX);
    assert!(should_purge); // u64::MAX <= u64::MAX

    // Test wrap-around: if policy period is u64::MAX
    let res = client.test_retention_purge(&1000, &u64::MAX, &2000);
    assert!(!res); // 1000 + u64::MAX (saturated) > 2000
}

#[test]
fn test_floating_point_precision_boundaries() {
    let env = Env::default();
    let mut engine = RulesEngine::new();

    // Mock many operations to ensure aggregate_score calculation is stable near limits
    let ctx = OperationContext {
        actor: "actor".into(),
        actor_role: "role".into(),
        action: "action".into(),
        target: "target".into(),
        timestamp: 1000,
        has_consent: true,
        sensitivity: 1,
        jurisdiction: Jurisdiction::US,
        record_count: 10,
        purpose: "p".into(),
        metadata: std::collections::HashMap::new(),
    };

    // Evaluate 10,000 times
    for _ in 0..10000 {
        engine.evaluate(&ctx);
    }

    let report = engine.generate_report(0, 2000, 2000, Jurisdiction::US);
    assert_eq!(report.total_operations, 10000);
    // Score should be exactly 100.0 since all passed
    assert!((report.aggregate_score - 100.0).abs() < f64::EPSILON);
}

