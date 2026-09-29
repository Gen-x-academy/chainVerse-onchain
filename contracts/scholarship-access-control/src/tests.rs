extern crate std;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    ContractError, Operation, Resource, Role, ScholarshipAccessControlContract,
    ScholarshipAccessControlContractClient,
};

fn setup() -> (Env, Address, ScholarshipAccessControlContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipAccessControlContract, ());
    let client = ScholarshipAccessControlContractClient::new(&env, &id);
    (env, id, client)
}

fn program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn other_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

// ── initialization ────────────────────────────────────────────────────────

#[test]
fn test_initialize_sets_admin_and_default_permissions() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    // Admin gets global Administrator role automatically.
    let global = client.global_scope();
    assert!(client.has_role(&admin, &global, &Role::Administrator));
    // Default permission: Student may Write an Application.
    assert!(client.can(&Resource::Application, &Role::Student, &Operation::Write));
}

#[test]
fn test_double_initialize_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let result = client.try_initialize(&admin);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::AlreadyInitialized);
}

// ── role grant / revoke ───────────────────────────────────────────────────

#[test]
fn test_grant_and_has_role_scoped() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let student = Address::generate(&env);
    let pid = program_id(&env);

    client.grant_role(&admin, &student, &pid, &Role::Student);
    assert!(client.has_role(&student, &pid, &Role::Student));
    // Student does NOT have Sponsor role on same program.
    assert!(!client.has_role(&student, &pid, &Role::Sponsor));
    // Student does NOT have role on a different program (cross-tenant isolation).
    let other = other_program_id(&env);
    assert!(!client.has_role(&student, &other, &Role::Student));
}

#[test]
fn test_cross_tenant_isolation() {
    // Assigning Sponsor on program A must not grant access to program B.
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let sponsor = Address::generate(&env);
    let pid_a = program_id(&env);
    let pid_b = other_program_id(&env);

    client.grant_role(&admin, &sponsor, &pid_a, &Role::Sponsor);
    assert!(client.has_role(&sponsor, &pid_a, &Role::Sponsor));
    // Cross-tenant: no role on B.
    assert!(!client.has_role(&sponsor, &pid_b, &Role::Sponsor));
    // require_role on B returns Unauthorized.
    let err = client.try_require_role(&sponsor, &pid_b, &Role::Sponsor).unwrap_err().unwrap();
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn test_revoke_role_removes_access_immediately() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let reviewer = Address::generate(&env);
    let pid = program_id(&env);

    client.grant_role(&admin, &reviewer, &pid, &Role::Reviewer);
    assert!(client.has_role(&reviewer, &pid, &Role::Reviewer));

    client.revoke_role(&admin, &reviewer, &pid, &Role::Reviewer);
    assert!(!client.has_role(&reviewer, &pid, &Role::Reviewer));
}

#[test]
fn test_non_admin_cannot_grant_role() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let attacker = Address::generate(&env);
    let victim = Address::generate(&env);
    let pid = program_id(&env);

    let err = client
        .try_grant_role(&attacker, &victim, &pid, &Role::Finance)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::NotAdmin);
}

// ── permission table ──────────────────────────────────────────────────────

#[test]
fn test_default_permissions_match_spec() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Finance may Disburse.
    assert!(client.can(&Resource::Disbursement, &Role::Finance, &Operation::Disburse));
    // Student may NOT Disburse.
    assert!(!client.can(&Resource::Disbursement, &Role::Student, &Operation::Disburse));
    // Auditor may Audit Disbursements.
    assert!(client.can(&Resource::Disbursement, &Role::Auditor, &Operation::Audit));
    // Reviewer may Read applications.
    assert!(client.can(&Resource::Application, &Role::Reviewer, &Operation::Read));
    // Reviewer may NOT Disburse.
    assert!(!client.can(&Resource::Disbursement, &Role::Reviewer, &Operation::Disburse));
}

#[test]
fn test_set_permission_overrides_default() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // By default Students cannot Disburse. Override to true then back.
    client.set_permission(&admin, &Resource::Disbursement, &Role::Student, &Operation::Disburse, &true);
    assert!(client.can(&Resource::Disbursement, &Role::Student, &Operation::Disburse));
    client.set_permission(&admin, &Resource::Disbursement, &Role::Student, &Operation::Disburse, &false);
    assert!(!client.can(&Resource::Disbursement, &Role::Student, &Operation::Disburse));
}

// ── authorize (combined role + permission check) ──────────────────────────

#[test]
fn test_authorize_passes_for_valid_role_and_op() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let finance = Address::generate(&env);
    let pid = program_id(&env);

    client.grant_role(&admin, &finance, &pid, &Role::Finance);
    client.authorize(&finance, &pid, &Role::Finance, &Resource::Disbursement, &Operation::Disburse);
}

#[test]
fn test_authorize_fails_wrong_role() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let student = Address::generate(&env);
    let pid = program_id(&env);

    client.grant_role(&admin, &student, &pid, &Role::Student);
    let err = client
        .try_authorize(&student, &pid, &Role::Finance, &Resource::Disbursement, &Operation::Disburse)
        .unwrap_err()
        .unwrap();
    // Student has no Finance role → Unauthorized.
    assert_eq!(err, ContractError::Unauthorized);
}

#[test]
fn test_authorize_fails_operation_not_permitted() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let reviewer = Address::generate(&env);
    let pid = program_id(&env);

    client.grant_role(&admin, &reviewer, &pid, &Role::Reviewer);
    // Reviewer has the role but is not allowed to Disburse.
    let err = client
        .try_authorize(&reviewer, &pid, &Role::Reviewer, &Resource::Disbursement, &Operation::Disburse)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::OperationNotPermitted);
}

// ── global roles ──────────────────────────────────────────────────────────

#[test]
fn test_global_administrator_visible_on_any_program() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    // Admin has global Administrator role; visible on any program_id.
    let random_pid = BytesN::from_array(&env, &[42u8; 32]);
    assert!(client.has_role(&admin, &random_pid, &Role::Administrator));
}

// ── per-program grant ceiling ─────────────────────────────────────────────

#[test]
fn test_grant_idempotent_no_double_count() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = program_id(&env);
    let user = Address::generate(&env);
    client.grant_role(&admin, &user, &pid, &Role::Student);
    // Second grant to same account/role is idempotent (no counter increment).
    client.grant_role(&admin, &user, &pid, &Role::Student);
    assert!(client.has_role(&user, &pid, &Role::Student));
}
