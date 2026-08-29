#![cfg(test)]

extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::{Address as _, Ledger}, xdr::ToXdr, Address, Bytes, BytesN, Env};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

/// Domain-separated proof matching verify.rs.
fn proof_bytes(
    env: &Env,
    contract_id: &Address,
    signing_key: &SigningKey,
    wallet: &Address,
    course_id: &BytesN<32>,
    nonce: &BytesN<32>,
    expires_at: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);
    msg.append(&Bytes::from_slice(env, b"CHAINVERSE_CERT:"));
    msg.append(&contract_id.to_xdr(env));
    msg.append(&env.ledger().network_id().into());
    msg.append(&wallet.to_xdr(env));
    msg.append(&course_id.clone().into());
    msg.append(&nonce.clone().into());
    msg.append(&Bytes::from_slice(env, &expires_at.to_be_bytes()));

    let len = msg.len() as usize;
    let mut buf = std::vec![0u8; len];
    msg.copy_into_slice(&mut buf);

    let signature = signing_key.sign(&buf);
    Bytes::from_slice(env, &signature.to_bytes())
}

#[test]
fn test_pause_blocks_mint() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let public_key = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    let course_id = BytesN::from_array(&env, &[1u8; 32]);
    let nonce = BytesN::from_array(&env, &[2u8; 32]);
    let expires_at = 5_000u64;

    env.ledger().set_timestamp(1_000);
    let proof = proof_bytes(&env, &contract_id, &signer, &wallet, &course_id, &nonce, expires_at);

    client.init(&admin, &public_key, &admin);
    client.toggle_pause(&admin, &true);

    let result = client.try_mint(&wallet, &course_id, &nonce, &expires_at, &proof);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}

#[test]
fn test_only_admin_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fake_admin = Address::generate(&env);

    client.init(&admin, &Bytes::from_array(&env, &[1u8; 32]), &admin);

    let result = client.try_toggle_pause(&fake_admin, &true);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}
