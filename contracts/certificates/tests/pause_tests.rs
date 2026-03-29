#![cfg(test)]

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Bytes, Env};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn proof_bytes(env: &Env, signing_key: &SigningKey, wallet: &Address, course_id: u64) -> Bytes {
    let payload = (wallet.clone(), course_id).to_xdr(env);
    let signature = signing_key.sign(&payload.to_alloc_vec());
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
    let proof = proof_bytes(&env, &signer, &wallet, 1);

    client.init(&admin);
    client.toggle_pause(&admin, &true);

    let result = client.try_mint(&wallet, &1, &public_key, &proof);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
    assert!(!client.has_certificate(&wallet, &1));
}

#[test]
fn test_only_admin_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fake_admin = Address::generate(&env);

    client.init(&admin);

    let result = client.try_toggle_pause(&fake_admin, &true);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}
