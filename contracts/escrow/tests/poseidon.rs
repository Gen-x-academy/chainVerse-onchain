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
