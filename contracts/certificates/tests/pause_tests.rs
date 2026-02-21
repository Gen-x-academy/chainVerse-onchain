use soroban_sdk::{Env, Address};
use crate::{ChainverseContract, ContractError};

#[test]
fn test_pause_blocks_purchase() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ChainverseContract);
    let client = ChainverseContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Pause contract
    client.toggle_pause(&admin, &true);

    // Try purchase
    let result = client.try_purchase_course(&user, &1);

    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn test_only_admin_can_pause() {
    let env = Env::default();
    let contract_id = env.register_contract(None, ChainverseContract);
    let client = ChainverseContractClient::new(&env, &contract_id);

    let fake_admin = Address::generate(&env);

    let result = client.try_toggle_pause(&fake_admin, &true);

    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}