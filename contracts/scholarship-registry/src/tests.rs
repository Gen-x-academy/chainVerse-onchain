#![cfg(test)]
use crate::{ContractError, Role, ScholarshipRegistryContract};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipRegistryContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

// ── module registry ──────────────────────────────────────────────────────────

#[test]
fn test_register_and_resolve_module() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let applications_addr = Address::generate(&env);
    let name = Symbol::new(&env, "applications");
    client.register_module(&admin, &name, &applications_addr);

    assert!(client.is_module_registered(&name));
    assert_eq!(client.get_module(&name), applications_addr);
}

#[test]
fn test_unregistered_module_lookup_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let name = Symbol::new(&env, "unknown");
    assert!(!client.is_module_registered(&name));
    let result = client.try_get_module(&name);
    assert_eq!(result, Err(Ok(ContractError::ModuleNotFound)));
}

#[test]
fn test_re_registering_module_updates_address() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let name = Symbol::new(&env, "core");
    let old_addr = Address::generate(&env);
    let new_addr = Address::generate(&env);
    client.register_module(&admin, &name, &old_addr);
    client.register_module(&admin, &name, &new_addr);

    assert_eq!(client.get_module(&name), new_addr);
}

#[test]
fn test_non_admin_cannot_register_module() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let attacker = Address::generate(&env);
    let name = Symbol::new(&env, "core");
    let addr = Address::generate(&env);
    let result = client.try_register_module(&attacker, &name, &addr);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// ── role registry ─────────────────────────────────────────────────────────────

#[test]
fn test_grant_and_check_role() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let reviewer = Address::generate(&env);
    assert!(!client.has_role(&reviewer, &Role::Reviewer));

    client.grant_role(&admin, &reviewer, &Role::Reviewer);
    assert!(client.has_role(&reviewer, &Role::Reviewer));
}

#[test]
fn test_role_grant_is_specific_to_the_role() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let account = Address::generate(&env);
    client.grant_role(&admin, &account, &Role::Finance);

    assert!(client.has_role(&account, &Role::Finance));
    assert!(!client.has_role(&account, &Role::Reviewer));
    assert!(!client.has_role(&account, &Role::Administrator));
}

#[test]
fn test_revoke_role_removes_access_immediately() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let account = Address::generate(&env);
    client.grant_role(&admin, &account, &Role::Sponsor);
    assert!(client.has_role(&account, &Role::Sponsor));

    client.revoke_role(&admin, &account, &Role::Sponsor);
    assert!(!client.has_role(&account, &Role::Sponsor));
}

#[test]
fn test_non_admin_cannot_grant_role() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let attacker = Address::generate(&env);
    let account = Address::generate(&env);
    let result = client.try_grant_role(&attacker, &account, &Role::Administrator);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_require_role_ok_when_granted() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let account = Address::generate(&env);
    client.grant_role(&admin, &account, &Role::Student);

    let result = client.try_require_role(&account, &Role::Student);
    assert!(result.is_ok());
}

#[test]
fn test_require_role_consistent_unauthorized_error() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let account = Address::generate(&env);
    let result = client.try_require_role(&account, &Role::Finance);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_multiple_roles_per_account() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipRegistryContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let account = Address::generate(&env);
    client.grant_role(&admin, &account, &Role::Reviewer);
    client.grant_role(&admin, &account, &Role::Finance);

    assert!(client.has_role(&account, &Role::Reviewer));
    assert!(client.has_role(&account, &Role::Finance));
}
