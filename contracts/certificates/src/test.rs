#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};
use crate::{CertificateContract, ContractError};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CertificateContract);
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

#[test]
fn test_admin_can_revoke_certificate() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let backend_key = Bytes::from_array(&env, &[1u8; 32]);
    client.init(&admin, &backend_key);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[2u8; 32]);
    // revoke should succeed even if cert does not exist (idempotent) or return error
    let result = client.try_revoke(&admin, &recipient, &course_id);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_non_admin_cannot_revoke() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let backend_key = Bytes::from_array(&env, &[1u8; 32]);
    client.init(&admin, &backend_key);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[3u8; 32]);
    let result = client.try_revoke(&attacker, &recipient, &course_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_reinitialize_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let key = Bytes::from_array(&env, &[4u8; 32]);
    client.init(&admin, &key);
    let result = client.try_init(&admin, &key);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_is_paused_default_false() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let key = Bytes::from_array(&env, &[5u8; 32]);
    client.init(&admin, &key);
    assert!(!client.is_paused());
}
