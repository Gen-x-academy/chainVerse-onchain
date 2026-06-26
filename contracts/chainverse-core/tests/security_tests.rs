#![cfg(test)]

use soroban_sdk::{testutils::Address as _, vec, Address, Env};

use chainverse_core::{ChainverseCore, ChainverseCoreClient};

fn setup(env: &Env) -> (Address, ChainverseCoreClient<'_>) {
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ChainverseCore);
    let client = ChainverseCoreClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin, &100, &vec![env]);
    (admin, client)
}

/// Double-initialization is rejected.
#[test]
fn test_double_initialize_rejected() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    assert!(client.try_initialize(&admin, &100, &vec![&env]).is_err());
}

/// Querying config before initialization is rejected.
#[test]
fn test_query_before_init_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, ChainverseCore);
    let client = ChainverseCoreClient::new(&env, &contract_id);
    assert!(client.try_get_config().is_err());
}

/// A non-admin address cannot pause the contract.
#[test]
fn test_non_admin_pause_rejected() {
    let env = Env::default();
    let (_admin, client) = setup(&env);
    let non_admin = Address::generate(&env);
    assert!(client.try_pause(&non_admin).is_err());
}

/// A non-admin address cannot update the config.
#[test]
fn test_non_admin_update_config_rejected() {
    let env = Env::default();
    let (_admin, client) = setup(&env);
    let non_admin = Address::generate(&env);
    assert!(client
        .try_update_config(&non_admin, &Some(999u32), &None)
        .is_err());
}

/// A non-admin address cannot transfer admin rights.
#[test]
fn test_non_admin_transfer_admin_rejected() {
    let env = Env::default();
    let (_admin, client) = setup(&env);
    let non_admin = Address::generate(&env);
    let victim = Address::generate(&env);
    assert!(client.try_transfer_admin(&non_admin, &victim).is_err());
}
