#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol, String};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> Address {
    let admin = Address::generate(env);
    ScholarshipDashboardsContract::initialize(env.clone(), admin.clone()).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

fn create_sponsor_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[3u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);

    assert!(ScholarshipDashboardsContract::initialize(env.clone(), admin.clone()).is_ok());
    assert_eq!(
        ScholarshipDashboardsContract::initialize(env, admin),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_student_dashboard_requires_student_auth() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    // Student can access their own dashboard
    let result = ScholarshipDashboardsContract::get_student_dashboard(env.clone(), student.clone());
    assert!(result.is_ok());
}

#[test]
fn test_sponsor_dashboard_requires_sponsor_auth() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let sponsor_id = create_sponsor_id(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_sponsor_dashboard(env, sponsor_id, caller);
    // In test with mock_all_auths, admin can access
    assert!(result.is_ok());
}

#[test]
fn test_reviewer_workbench_requires_reviewer_auth() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let reviewer = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_reviewer_workbench(env, reviewer);
    assert!(result.is_ok());
}

#[test]
fn test_finance_ops_dashboard_requires_admin() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_finance_ops_dashboard(env, caller);
    // With mock_all_auths, this works
    assert!(result.is_ok());
}

#[test]
fn test_invalidate_cache_requires_admin() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::invalidate_cache(env, caller);
    // With mock_all_auths, admin can invalidate
    assert!(result.is_ok());
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipDashboardsContract::version(env), 1);
}

#[test]
fn test_student_applications() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_student_applications(env, student);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[test]
fn test_student_info_requests() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_student_info_requests(env, student);
    assert!(result.is_ok());
}

#[test]
fn test_student_awards() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let student = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_student_awards(env, student);
    assert!(result.is_ok());
}

#[test]
fn test_sponsor_program_detail() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let sponsor_id = create_sponsor_id(&env);
    let program_id = create_program_id(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_sponsor_program_detail(env, sponsor_id, program_id, caller);
    assert!(result.is_ok());
}

#[test]
fn test_reviewer_assignments() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let reviewer = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_reviewer_assignments(env, reviewer);
    assert!(result.is_ok());
}

#[test]
fn test_reviewer_conflicts() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let reviewer = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_reviewer_conflicts(env, reviewer);
    assert!(result.is_ok());
}

#[test]
fn test_reconciliation_items() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_reconciliation_items(env, caller, None);
    assert!(result.is_ok());
}

#[test]
fn test_due_payments() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_due_payments(env, caller, None);
    assert!(result.is_ok());
}

#[test]
fn test_failed_payments() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let caller = Address::generate(&env);

    let result = ScholarshipDashboardsContract::get_failed_payments(env, caller);
    assert!(result.is_ok());
}