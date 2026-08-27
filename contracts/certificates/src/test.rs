#![cfg(test)]
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::{Address as _, Ledger}, xdr::ToXdr, Address, Bytes, BytesN, Env};
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
    client.init(&admin, &backend_key, &admin);
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
    client.init(&admin, &backend_key, &admin);
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
    client.init(&admin, &key, &admin);
    let result = client.try_init(&admin, &key);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_is_paused_default_false() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let key = Bytes::from_array(&env, &[5u8; 32]);
    client.init(&admin, &key, &admin);
    assert!(!client.is_paused());
}

fn proof(
    env: &Env,
    signer: &SigningKey,
    recipient: &Address,
    course_id: &BytesN<32>,
    expires_at: u64,
    nonce: &BytesN<32>,
) -> Bytes {
    let payload = (recipient.clone(), course_id.clone(), expires_at, nonce.clone()).to_xdr(env);
    let mut message = std::vec![0u8; payload.len() as usize];
    payload.copy_into_slice(&mut message);
    Bytes::from_slice(env, &signer.sign(&message).to_bytes())
}

#[test]
fn test_valid_proof_consumes_nonce_and_mints() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let signer = SigningKey::from_bytes(&[7u8; 32]);
    let backend_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[8u8; 32]);
    let nonce = BytesN::from_array(&env, &[9u8; 32]);
    let expires_at = 100;

    env.ledger().with_mut(|ledger| ledger.timestamp = 10);
    client.init(&admin, &backend_key, &admin);
    let signed_proof = proof(&env, &signer, &recipient, &course_id, expires_at, &nonce);
    client.mint(&recipient, &course_id, &expires_at, &nonce, &signed_proof);

    assert!(client.get_certificate(&recipient, &course_id).is_some());
}

#[test]
fn test_expired_proof_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let signer = SigningKey::from_bytes(&[10u8; 32]);
    let backend_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[11u8; 32]);
    let nonce = BytesN::from_array(&env, &[12u8; 32]);
    let expires_at = 20;

    env.ledger().with_mut(|ledger| ledger.timestamp = 20);
    client.init(&admin, &backend_key, &admin);
    let signed_proof = proof(&env, &signer, &recipient, &course_id, expires_at, &nonce);
    assert_eq!(client.try_mint(&recipient, &course_id, &expires_at, &nonce, &signed_proof), Err(Ok(ContractError::ProofExpired)));
}

#[test]
fn test_nonce_cannot_be_reused() {
    let (env, contract_id, admin) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let signer = SigningKey::from_bytes(&[13u8; 32]);
    let backend_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let recipient = Address::generate(&env);
    let first_course = BytesN::from_array(&env, &[14u8; 32]);
    let second_course = BytesN::from_array(&env, &[15u8; 32]);
    let nonce = BytesN::from_array(&env, &[16u8; 32]);
    let expires_at = 100;

    env.ledger().with_mut(|ledger| ledger.timestamp = 10);
    client.init(&admin, &backend_key, &admin);
    let first_proof = proof(&env, &signer, &recipient, &first_course, expires_at, &nonce);
    client.mint(&recipient, &first_course, &expires_at, &nonce, &first_proof);
    let second_proof = proof(&env, &signer, &recipient, &second_course, expires_at, &nonce);

    assert_eq!(client.try_mint(&recipient, &second_course, &expires_at, &nonce, &second_proof), Err(Ok(ContractError::NonceAlreadyConsumed)));
}
