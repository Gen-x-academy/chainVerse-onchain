#![cfg(test)]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{storage::Persistent as _, Address as _, Events as _},
    xdr::ToXdr,
    Address, Bytes, Env,
};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[3u8; 32])
}

fn public_key_bytes(env: &Env, signing_key: &SigningKey) -> Bytes {
    Bytes::from_slice(env, &signing_key.verifying_key().to_bytes())
}

fn proof_bytes(env: &Env, signing_key: &SigningKey, wallet: &Address, course_id: u64) -> Bytes {
    let payload = (wallet.clone(), course_id).to_xdr(env);
    let mut message = std::vec![0u8; payload.len() as usize];
    payload.copy_into_slice(&mut message);
    let signature = signing_key.sign(&message);
    Bytes::from_slice(env, &signature.to_bytes())
}

#[test]
fn test_structurally_invalid_proof_rejected_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let public_key = public_key_bytes(&env, &signer);

    client.init(&admin);

    let valid_proof = proof_bytes(&env, &signer, &wallet, 99);
    let mut valid_proof_bytes = std::vec![0u8; valid_proof.len() as usize];
    valid_proof.copy_into_slice(&mut valid_proof_bytes);
    let invalid_proof = Bytes::from_slice(&env, &valid_proof_bytes[..63]);
    let storage_before = env.storage().persistent().all().len();
    let events_before = env.events().all().len();

    let result = client.try_mint(&wallet, &99, &public_key, &invalid_proof);

    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
    assert_eq!(env.storage().persistent().all().len(), storage_before);
    assert_eq!(env.events().all().len(), events_before);
    assert!(!client.has_certificate(&wallet, &99));
}

#[test]
fn test_tampered_proof_rejected_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();

    client.init(&admin);

    let public_key = public_key_bytes(&env, &signer);
    let original_proof = proof_bytes(&env, &signer, &wallet, 101);
    let mut tampered = std::vec![0u8; original_proof.len() as usize];
    original_proof.copy_into_slice(&mut tampered);
    tampered[0] ^= 0x01;
    let tampered_proof = Bytes::from_slice(&env, &tampered);
    let storage_before = env.storage().persistent().all().len();
    let events_before = env.events().all().len();

    let result = client.try_mint(&wallet, &101, &public_key, &tampered_proof);

    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
    assert_eq!(env.storage().persistent().all().len(), storage_before);
    assert_eq!(env.events().all().len(), events_before);
    assert!(!client.has_certificate(&wallet, &101));
}
