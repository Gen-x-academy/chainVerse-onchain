use super::*;
use soroban_sdk::{BytesN, Env};

#[test]
fn rotate_backend_pubkey_before_initialize_returns_not_initialized() {
    let env = Env::default();
    let contract_id = env.register_contract(None, RewardContract);
    let client = RewardContractClient::new(&env, &contract_id);
    let new_pubkey = BytesN::from_array(&env, &[7; 32]);

    let result = client.try_rotate_backend_pubkey(&new_pubkey);

    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}
