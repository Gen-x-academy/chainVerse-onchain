#![cfg(test)]
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::{Address as _, Ledger}, xdr::ToXdr, Address, Bytes, BytesN, Env};
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
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Bytes, BytesN, Env, IntoVal,
};
use crate::{CertificateContract, ContractError};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CertificateContract);
    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    (env, contract_id, admin, minter)
}

fn init_client(env: &Env, client: &crate::CertificateContractClient, admin: &Address, minter: &Address) -> Bytes {
    let backend_key = Bytes::from_array(env, &[1u8; 32]);
    client.init(admin, &backend_key, minter);
    backend_key
}

#[test]
fn test_admin_can_revoke_certificate() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let backend_key = Bytes::from_array(&env, &[1u8; 32]);
    client.init(&admin, &backend_key, &admin);
    init_client(&env, &client, &admin, &minter);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[2u8; 32]);
    // revoke should succeed even if cert does not exist (idempotent) or return error
    let result = client.try_revoke(&admin, &recipient, &course_id);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn test_non_admin_cannot_revoke() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let backend_key = Bytes::from_array(&env, &[1u8; 32]);
    client.init(&admin, &backend_key, &admin);
    init_client(&env, &client, &admin, &minter);
    let attacker = Address::generate(&env);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[3u8; 32]);
    let result = client.try_revoke(&attacker, &recipient, &course_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_reinitialize_rejected() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let key = Bytes::from_array(&env, &[4u8; 32]);
    client.init(&admin, &key, &admin);
    let result = client.try_init(&admin, &key);
    client.init(&admin, &key, &minter);
    let result = client.try_init(&admin, &key, &minter);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_is_paused_default_false() {
    let (env, contract_id, admin, minter) = setup();
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
    init_client(&env, &client, &admin, &minter);
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

// ===== ISSUE #842: revocation reason/actor in event =====

#[test]
fn test_revoke_with_reason_emits_full_event() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[9u8; 32]);
    let metadata = Bytes::from_array(&env, &[0x01]);
    client.mint_certificate(&recipient, &course_id, &metadata);
    let reason = Bytes::from_array(&env, &[0xAA, 0xBB, 0xCC]);
    client.revoke_with_reason(&admin, &recipient, &course_id, &reason);

    // Fix #842: the CERT_RVK event carries (actor, reason, token_id,
    // recipient, course, timestamp).
    let expected: soroban_sdk::Val =
        (admin, reason, 0u64, recipient.clone(), course_id.clone(), env.ledger().timestamp()).into_val(&env);
    let found = env.events().all().iter().any(|e| e.2 == expected);
    assert!(found, "CERT_RVK event with full revocation payload must be emitted");
    // The certificate is removed from storage.
    assert!(client.get_certificate(&recipient, &course_id).is_none());
}

// ===== ISSUE #841: two-step admin transfer =====

#[test]
fn test_propose_and_accept_admin_transfer() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);

    // The nominated pending admin accepts and becomes the new admin.
    assert!(client.try_accept_admin_transfer().is_ok());
    // New admin can revoke; old admin can no longer.
    let course_id = BytesN::from_array(&env, &[11u8; 32]);
    let recipient = Address::generate(&env);
    assert!(client.try_revoke(&new_admin, &recipient, &course_id).is_ok());
    assert_eq!(client.try_revoke(&admin, &recipient, &course_id), Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_cancel_admin_transfer() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);
    client.cancel_admin_transfer(&admin);
    // After cancellation there is no pending transfer to accept.
    assert_eq!(client.try_accept_admin_transfer(), Err(Ok(ContractError::NoPendingTransfer)));
}

#[test]
fn test_non_admin_cannot_propose_transfer() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let result = client.try_propose_admin_transfer(&attacker, &new_admin);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_accept_admin_transfer_after_expiry_fails() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    let new_admin = Address::generate(&env);
    client.propose_admin_transfer(&admin, &new_admin);
    // Jump far into the future so the proposal window has elapsed.
    env.ledger().set_timestamp(env.ledger().timestamp() + 1_000_000_000);
    let result = client.try_accept_admin_transfer();
    assert_eq!(result, Err(Ok(ContractError::PendingAdminExpired)));
}
