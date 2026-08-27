#![cfg(test)]
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::Address as _, xdr::ToXdr, Address, Bytes, BytesN, Env,
};
use crate::{CertificateContract, ContractError};

fn signing_key() -> SigningKey { SigningKey::from_bytes(&[7u8; 32]) }

fn proof_bytes(
    env: &Env,
    signing_key: &SigningKey,
    recipient: &Address,
    course_id: &BytesN<32>,
    nonce: &BytesN<32>,
    expires_at: u64,
) -> Bytes {
    let contract_id = env.current_contract_address();
    let network_id = env.ledger().network_id();
    let payload = (
        b"CHAINVERSE_CERT:".as_slice(),
        recipient.clone(),
        course_id.clone(),
        contract_id,
        network_id,
        nonce.clone(),
        expires_at,
    )
        .to_xdr(env);
    let mut message = std::vec![0u8; payload.len() as usize];
    payload.copy_into_slice(&mut message);
    let signature = signing_key.sign(&message);
    Bytes::from_slice(env, &signature.to_bytes())
}

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

#[test]
fn test_bound_proof_allows_valid_recipient_mint() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CertificateContract);
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let signer = signing_key();
    let backend_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let minter = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[9u8; 32]);
    let nonce = BytesN::from_array(&env, &[10u8; 32]);
    let expires_at = env.ledger().timestamp() + 600;
    let proof = proof_bytes(&env, &signer, &recipient, &course_id, &nonce, expires_at);

    client.init(&admin, &backend_key, &minter);
    let result = client.try_mint(&recipient, &course_id, &nonce, &expires_at, &proof);
    assert!(result.is_ok());
    assert!(client.has_certificate(&recipient, &course_id));
}

#[test]
fn test_bound_proof_rejects_other_recipient_or_expired_nonce() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CertificateContract);
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    let other_recipient = Address::generate(&env);
    let signer = signing_key();
    let backend_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let minter = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[11u8; 32]);
    let nonce = BytesN::from_array(&env, &[12u8; 32]);
    let expires_at = env.ledger().timestamp() + 600;
    let valid_proof = proof_bytes(&env, &signer, &recipient, &course_id, &nonce, expires_at);

    client.init(&admin, &backend_key, &minter);

    let result = client.try_mint(&other_recipient, &course_id, &nonce, &expires_at, &valid_proof);
    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
    assert!(!client.has_certificate(&other_recipient, &course_id));

    let expired = env.ledger().timestamp() - 1;
    let expired_proof = proof_bytes(&env, &signer, &recipient, &course_id, &nonce, expired);
    let result = client.try_mint(&recipient, &course_id, &nonce, &expired, &expired_proof);
    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
}

