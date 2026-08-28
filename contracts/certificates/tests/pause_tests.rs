#![cfg(test)]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, BytesN, Env};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn proof_bytes(env: &Env, signing_key: &SigningKey, wallet: &Address, course_id: &BytesN<32>, expires_at: u64, nonce: &BytesN<32>) -> Bytes {
    let payload = (wallet.clone(), course_id.clone(), expires_at, nonce.clone()).to_xdr(env);
    let mut message = std::vec![0u8; payload.len() as usize];
    payload.copy_into_slice(&mut message);
    let signature = signing_key.sign(&message);
    Bytes::from_slice(env, &signature.to_bytes())
}

#[test]
fn test_pause_blocks_mint() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let public_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let course_id = BytesN::from_array(&env, &[1u8; 32]);
    let nonce = BytesN::from_array(&env, &[2u8; 32]);
    let expires_at = 100;
    let proof = proof_bytes(&env, &signer, &wallet, &course_id, expires_at, &nonce);

    client.init(&admin, &public_key, &admin);
    client.toggle_pause(&admin, &true);

    let result = client.try_mint(&wallet, &course_id, &expires_at, &nonce, &proof);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}

#[test]
fn test_only_admin_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fake_admin = Address::generate(&env);

    client.init(&admin, &Bytes::from_array(&env, &[1u8; 32]), &admin);

    let result = client.try_toggle_pause(&fake_admin, &true);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}
