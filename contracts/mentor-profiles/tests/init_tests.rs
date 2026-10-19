#![cfg(test)]
mod utils;

use soroban_sdk::{Address, Bytes, Env};
use mentor_profiles::MentorProfilesContract;

#[test]
fn test_init_sets_admin_and_backend() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let result = contract.init(env.clone(), admin.clone(), backend.clone());
    
    assert!(result.is_ok());
    
    // Verify admin was set
    assert_eq!(contract.get_admin(env.clone()).unwrap(), admin);
    
    // Verify backend signer was set
    assert_eq!(contract.get_backend_signer(env.clone()).unwrap(), backend);
}

#[test]
fn test_init_requires_admin_auth() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    // Don't authorize admin
    let result = contract.init(env.clone(), admin, backend);
    
    assert!(result.is_err());
}

#[test]
fn test_double_init_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let backend = Address::generate(&env);
    let new_admin = Address::generate(&env);
    
    let contract = MentorProfilesContract;
    
    admin.mock_all_auths();
    let _ = contract.init(env.clone(), admin.clone(), backend.clone());
    
    // Try to init again
    new_admin.mock_all_auths();
    let result = contract.init(env.clone(), new_admin, Address::generate(&env));
    
    assert!(result.is_err());
}
