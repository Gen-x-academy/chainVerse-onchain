#![cfg(test)]

extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    xdr::ToXdr,
    Address, Bytes, BytesN, Env,
};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[3u8; 32])
}

fn public_key_bytes(env: &Env, signing_key: &SigningKey) -> Bytes {
    Bytes::from_slice(env, &signing_key.verifying_key().to_bytes())
}

/// Domain-separated proof helper that mirrors verify.rs exactly.
fn proof_bytes(
    env: &Env,
    contract_id: &Address,
    signing_key: &SigningKey,
    recipient: &Address,
    course_id: &BytesN<32>,
    nonce: &BytesN<32>,
    expires_at: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);
    msg.append(&Bytes::from_slice(env, b"CHAINVERSE_CERT:"));
    msg.append(&contract_id.to_xdr(env));
    msg.append(&env.ledger().network_id().into());
    msg.append(&recipient.to_xdr(env));
    msg.append(&course_id.clone().into());
    msg.append(&nonce.clone().into());
    msg.append(&Bytes::from_slice(env, &expires_at.to_be_bytes()));

    let len = msg.len() as usize;
    let mut buf = std::vec![0u8; len];
    msg.copy_into_slice(&mut buf);

    let signature = signing_key.sign(&buf);
    Bytes::from_slice(env, &signature.to_bytes())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_init_rejects_reinitialization() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let signer = signing_key();
    let backend_key = public_key_bytes(&env, &signer);

    client.init(&admin, &backend_key, &admin);

    let result = client.try_init(&admin, &backend_key, &admin);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

/// A structurally invalid proof (wrong length) must be rejected without
/// minting or emitting events.
#[test]
fn test_structurally_invalid_proof_rejected_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let public_key = public_key_bytes(&env, &signer);

    client.init(&admin, &public_key, &admin);
    env.ledger().set_timestamp(1_000);

    let course_id = BytesN::from_array(&env, &[99u8; 32]);
    let nonce = BytesN::from_array(&env, &[98u8; 32]);
    let expires_at = 5_000u64;
    let valid_proof = proof_bytes(&env, &contract_id, &signer, &wallet, &course_id, &nonce, expires_at);

    // Truncate the proof to make it invalid.
    let mut valid_proof_bytes = std::vec![0u8; valid_proof.len() as usize];
    valid_proof.copy_into_slice(&mut valid_proof_bytes);
    let invalid_proof = Bytes::from_slice(&env, &valid_proof_bytes[..63]);

    let events_before = env.events().all().len();

    let result = client.try_mint(&wallet, &course_id, &nonce, &expires_at, &invalid_proof);

    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
    // No new events should have been emitted.
    assert_eq!(env.events().all().len(), events_before);
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}

#[test]
fn test_revoke_certificate_emits_event_and_clears_state() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let backend_key = public_key_bytes(&env, &signer);

    client.init(&admin, &backend_key, &admin);
    env.ledger().set_timestamp(1_000);

    let course_id = BytesN::from_array(&env, &[7u8; 32]);
    let nonce = BytesN::from_array(&env, &[8u8; 32]);
    let expires_at = 5_000u64;
    let proof = proof_bytes(&env, &contract_id, &signer, &wallet, &course_id, &nonce, expires_at);
    client.mint(&wallet, &course_id, &nonce, &expires_at, &proof);

    assert!(client.get_certificate(&wallet, &course_id).is_some());

    let before = env.events().all().len();
    client.revoke(&admin, &wallet, &course_id);

    assert!(env.events().all().len() > before, "revoke must emit at least one event");
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}

#[test]
fn test_tampered_proof_rejected_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();

    client.init(&admin, &public_key_bytes(&env, &signer), &admin);
    env.ledger().set_timestamp(1_000);

    let course_id = BytesN::from_array(&env, &[101u8; 32]);
    let nonce = BytesN::from_array(&env, &[102u8; 32]);
    let expires_at = 5_000u64;
    let original_proof = proof_bytes(&env, &contract_id, &signer, &wallet, &course_id, &nonce, expires_at);

    let mut tampered = std::vec![0u8; original_proof.len() as usize];
    original_proof.copy_into_slice(&mut tampered);
    tampered[0] ^= 0x01;
    let tampered_proof = Bytes::from_slice(&env, &tampered);

    let events_before = env.events().all().len();

    let result = client.try_mint(&wallet, &course_id, &nonce, &expires_at, &tampered_proof);

    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
    assert_eq!(env.events().all().len(), events_before);
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}

#[test]
fn test_revoke_certificate_clears_state_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CertificateContract, ());
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let wallet = Address::generate(&env);
    let signer = signing_key();
    let public_key = public_key_bytes(&env, &signer);
    let course_id = BytesN::from_array(&env, &[55u8; 32]);
    let nonce = BytesN::from_array(&env, &[56u8; 32]);

    client.init(&admin, &public_key, &admin);
    env.ledger().set_timestamp(1_000);

    let expires_at = 5_000u64;
    let proof = proof_bytes(&env, &contract_id, &signer, &wallet, &course_id, &nonce, expires_at);
    client.mint(&wallet, &course_id, &nonce, &expires_at, &proof);

    assert!(client.get_certificate(&wallet, &course_id).is_some());

    let before = env.events().all().len();
    client.revoke(&admin, &wallet, &course_id);

    assert!(env.events().all().len() > before, "revoke must emit at least one event");
    assert!(client.get_certificate(&wallet, &course_id).is_none());
}
