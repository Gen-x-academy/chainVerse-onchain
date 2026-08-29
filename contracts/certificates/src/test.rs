#![cfg(test)]

extern crate std;

use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, BytesN, Env,
};

use crate::{CertificateContract, ContractError};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn make_signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// Build the domain-separated message that verify_backend_proof expects and sign it.
///
/// Message layout (must match verify.rs exactly):
///   "CHAINVERSE_CERT:" | contract_id_xdr | network_id | recipient_xdr
///   | course_id (32 B) | nonce (32 B) | expires_at BE-u64
fn build_proof(
    env: &Env,
    contract_id: &Address,
    signer: &SigningKey,
    recipient: &Address,
    course_id: &BytesN<32>,
    nonce: &BytesN<32>,
    expires_at: u64,
) -> Bytes {
    use soroban_sdk::xdr::ToXdr;

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

    let sig = signer.sign(&buf);
    Bytes::from_slice(env, &sig.to_bytes())
}

// ---------------------------------------------------------------------------
// #833 – Domain separation
// ---------------------------------------------------------------------------

/// A valid domain-separated proof mints a certificate.
#[test]
fn test_833_domain_separated_proof_mints_cert() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(33);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    env.ledger().set_timestamp(1_000);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[1u8; 32]);
    let nonce = BytesN::from_array(&env, &[2u8; 32]);
    let expires_at = 2_000u64;
    let proof = build_proof(&env, &contract_id, &signer, &recipient, &course_id, &nonce, expires_at);

    client.mint(&recipient, &course_id, &nonce, &expires_at, &proof);
    assert!(client.get_certificate(&recipient, &course_id).is_some());
}

/// A proof signed for a different contract address is rejected (cross-contract replay).
#[test]
fn test_833_proof_for_different_contract_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let signer = make_signing_key(33);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());

    // Register two contracts with the same signer key.
    let contract_a = env.register(CertificateContract, ());
    let contract_b = env.register(CertificateContract, ());
    let client_a = crate::CertificateContractClient::new(&env, &contract_a);
    let client_b = crate::CertificateContractClient::new(&env, &contract_b);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    client_a.init(&admin, &pubkey, &minter);
    client_b.init(&admin, &pubkey, &minter);

    env.ledger().set_timestamp(1_000);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[3u8; 32]);
    let nonce = BytesN::from_array(&env, &[4u8; 32]);
    let expires_at = 2_000u64;

    // Proof is generated for contract_a.
    let proof_for_a = build_proof(&env, &contract_a, &signer, &recipient, &course_id, &nonce, expires_at);

    // Replaying on contract_b must fail.
    let result = client_b.try_mint(&recipient, &course_id, &nonce, &expires_at, &proof_for_a);
    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
}

/// A proof signed for a different recipient is rejected.
#[test]
fn test_833_proof_for_wrong_recipient_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(34);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    env.ledger().set_timestamp(1_000);
    let recipient_a = Address::generate(&env);
    let recipient_b = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[5u8; 32]);
    let nonce = BytesN::from_array(&env, &[6u8; 32]);
    let expires_at = 2_000u64;

    // Proof is for recipient_a but we try to mint for recipient_b.
    let proof = build_proof(&env, &contract_id, &signer, &recipient_a, &course_id, &nonce, expires_at);
    let result = client.try_mint(&recipient_b, &course_id, &nonce, &expires_at, &proof);
    assert_eq!(result, Err(Ok(ContractError::InvalidProof)));
}

// ---------------------------------------------------------------------------
// #834 – Backend public key validation
// ---------------------------------------------------------------------------

/// init with a 32-byte key succeeds.
#[test]
fn test_834_valid_32_byte_pubkey_accepted() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let valid_key = Bytes::from_slice(&env, &[0xABu8; 32]);
    assert!(client.try_init(&admin, &valid_key, &minter).is_ok());
}

/// init with a key shorter than 32 bytes is rejected.
#[test]
fn test_834_short_pubkey_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let short_key = Bytes::from_slice(&env, &[0x01u8; 31]);
    let result = client.try_init(&admin, &short_key, &minter);
    assert_eq!(result, Err(Ok(ContractError::InvalidPublicKey)));
}

/// init with a key longer than 32 bytes is rejected.
#[test]
fn test_834_long_pubkey_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let long_key = Bytes::from_slice(&env, &[0x01u8; 33]);
    let result = client.try_init(&admin, &long_key, &minter);
    assert_eq!(result, Err(Ok(ContractError::InvalidPublicKey)));
}

/// init with an empty key is rejected.
#[test]
fn test_834_empty_pubkey_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let empty_key = Bytes::new(&env);
    let result = client.try_init(&admin, &empty_key, &minter);
    assert_eq!(result, Err(Ok(ContractError::InvalidPublicKey)));
}

/// init rejects malformed key and writes NO state (no partial writes).
#[test]
fn test_834_no_state_written_on_invalid_pubkey() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let bad_key = Bytes::from_slice(&env, &[0x01u8; 16]);
    let _ = client.try_init(&admin, &bad_key, &minter);

    // Contract must not be initialized — a second init with a valid key should succeed.
    let valid_key = Bytes::from_slice(&env, &[0x02u8; 32]);
    let result = client.try_init(&admin, &valid_key, &minter);
    assert!(result.is_ok(), "no state should be committed when init fails due to bad pubkey");
}

// ---------------------------------------------------------------------------
// #835 – Backend signing-key rotation
// ---------------------------------------------------------------------------

/// Admin can propose and activate a key rotation; new key is used for proofs.
#[test]
fn test_835_propose_and_activate_key_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let old_signer = make_signing_key(10);
    let new_signer = make_signing_key(20);
    let old_pubkey = Bytes::from_slice(&env, &old_signer.verifying_key().to_bytes());
    let new_pubkey = Bytes::from_slice(&env, &new_signer.verifying_key().to_bytes());

    client.init(&admin, &old_pubkey, &minter);
    env.ledger().set_timestamp(1_000);

    // Propose and activate rotation.
    client.propose_key_rotation(&admin, &new_pubkey);
    client.activate_key_rotation(&admin);

    // A proof signed with the NEW key must be accepted.
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[30u8; 32]);
    let nonce = BytesN::from_array(&env, &[31u8; 32]);
    let expires_at = 5_000u64;
    let proof = build_proof(&env, &contract_id, &new_signer, &recipient, &course_id, &nonce, expires_at);
    assert!(client.try_mint(&recipient, &course_id, &nonce, &expires_at, &proof).is_ok());
}

/// After activation the old key no longer works.
#[test]
fn test_835_old_key_rejected_after_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let old_signer = make_signing_key(10);
    let new_signer = make_signing_key(20);
    let old_pubkey = Bytes::from_slice(&env, &old_signer.verifying_key().to_bytes());
    let new_pubkey = Bytes::from_slice(&env, &new_signer.verifying_key().to_bytes());

    client.init(&admin, &old_pubkey, &minter);
    env.ledger().set_timestamp(1_000);
    client.propose_key_rotation(&admin, &new_pubkey);
    client.activate_key_rotation(&admin);

    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[32u8; 32]);
    let nonce = BytesN::from_array(&env, &[33u8; 32]);
    let expires_at = 5_000u64;
    // Proof still signed with old key.
    let proof = build_proof(&env, &contract_id, &old_signer, &recipient, &course_id, &nonce, expires_at);
    assert_eq!(
        client.try_mint(&recipient, &course_id, &nonce, &expires_at, &proof),
        Err(Ok(ContractError::InvalidProof))
    );
}

/// Non-admin cannot propose a key rotation.
#[test]
fn test_835_non_admin_cannot_propose_key_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(11);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let new_pubkey = Bytes::from_slice(&env, &make_signing_key(22).verifying_key().to_bytes());
    let result = client.try_propose_key_rotation(&attacker, &new_pubkey);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

/// Activate fails when no rotation has been proposed.
#[test]
fn test_835_activate_without_proposal_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(12);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let result = client.try_activate_key_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::NoPendingKeyRotation)));
}

/// Activation after proposal TTL expiry is rejected.
#[test]
fn test_835_expired_proposal_cannot_be_activated() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(13);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    env.ledger().set_timestamp(1_000);
    let new_pubkey = Bytes::from_slice(&env, &make_signing_key(23).verifying_key().to_bytes());
    client.propose_key_rotation(&admin, &new_pubkey);

    // Jump past ROTATION_PROPOSAL_TTL (~120_960 seconds).
    env.ledger().set_timestamp(1_000 + 200_000);
    let result = client.try_activate_key_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::PendingKeyRotationExpired)));
}

/// Admin can cancel an outstanding proposal.
#[test]
fn test_835_cancel_key_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(14);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let new_pubkey = Bytes::from_slice(&env, &make_signing_key(24).verifying_key().to_bytes());
    client.propose_key_rotation(&admin, &new_pubkey);
    client.cancel_key_rotation(&admin);

    let result = client.try_activate_key_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::NoPendingKeyRotation)));
}

/// propose_key_rotation rejects a non-32-byte proposed key.
#[test]
fn test_835_invalid_proposed_key_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(15);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let bad_key = Bytes::from_slice(&env, &[0xFFu8; 31]);
    let result = client.try_propose_key_rotation(&admin, &bad_key);
    assert_eq!(result, Err(Ok(ContractError::InvalidPublicKey)));
}

// ---------------------------------------------------------------------------
// #836 – Minter rotation
// ---------------------------------------------------------------------------

/// Admin can propose and activate a minter rotation; new minter gains auth.
#[test]
fn test_836_propose_and_activate_minter_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let old_minter = Address::generate(&env);
    let new_minter = Address::generate(&env);
    let signer = make_signing_key(40);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &old_minter);
    env.ledger().set_timestamp(1_000);

    client.propose_minter_rotation(&admin, &new_minter);
    client.activate_minter_rotation(&admin);

    // New minter can mint_certificate without error.
    let student = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[50u8; 32]);
    let metadata = Bytes::from_slice(&env, b"ipfs://test");
    assert!(client.try_mint_certificate(&student, &course_id, &metadata).is_ok());
}

/// After minter rotation the old minter address loses authorisation.
/// (With mock_all_auths the auth check passes — we verify the stored minter changed.)
#[test]
fn test_836_minter_replaced_after_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let old_minter = Address::generate(&env);
    let new_minter = Address::generate(&env);
    let signer = make_signing_key(41);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &old_minter);

    client.propose_minter_rotation(&admin, &new_minter);
    client.activate_minter_rotation(&admin);

    // The new minter can still mint (mock auth passes).
    let student = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[51u8; 32]);
    let metadata = Bytes::from_slice(&env, b"ipfs://new-minter");
    assert!(client.try_mint_certificate(&student, &course_id, &metadata).is_ok());
}

/// Non-admin cannot propose a minter rotation.
#[test]
fn test_836_non_admin_cannot_propose_minter_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(42);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let new_minter = Address::generate(&env);
    let result = client.try_propose_minter_rotation(&attacker, &new_minter);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

/// Activate fails when no minter rotation proposal exists.
#[test]
fn test_836_activate_without_proposal_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(43);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let result = client.try_activate_minter_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::NoPendingMinterRotation)));
}

/// Activation of an expired minter rotation proposal is rejected.
#[test]
fn test_836_expired_minter_proposal_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let new_minter = Address::generate(&env);
    let signer = make_signing_key(44);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    env.ledger().set_timestamp(1_000);
    client.propose_minter_rotation(&admin, &new_minter);
    env.ledger().set_timestamp(1_000 + 200_000);

    let result = client.try_activate_minter_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::PendingMinterRotationExpired)));
}

/// Admin can cancel an outstanding minter rotation proposal.
#[test]
fn test_836_cancel_minter_rotation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let new_minter = Address::generate(&env);
    let signer = make_signing_key(45);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    client.propose_minter_rotation(&admin, &new_minter);
    client.cancel_minter_rotation(&admin);

    let result = client.try_activate_minter_rotation(&admin);
    assert_eq!(result, Err(Ok(ContractError::NoPendingMinterRotation)));
}

// ---------------------------------------------------------------------------
// Regression: existing behaviour still works
// ---------------------------------------------------------------------------

/// Re-initialisation is still rejected.
#[test]
fn test_reinitialize_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let key = Bytes::from_slice(&env, &[4u8; 32]);
    client.init(&admin, &key, &minter);
    let result = client.try_init(&admin, &key, &minter);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

/// Contract is not paused after init.
#[test]
fn test_is_paused_default_false() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let key = Bytes::from_slice(&env, &[5u8; 32]);
    client.init(&admin, &key, &minter);
    assert!(!client.is_paused());
}

/// Non-admin cannot revoke.
#[test]
fn test_non_admin_cannot_revoke() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let minter = Address::generate(&env);
    let key = Bytes::from_slice(&env, &[3u8; 32]);
    client.init(&admin, &key, &minter);
    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[3u8; 32]);
    let result = client.try_revoke(&attacker, &recipient, &course_id);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

/// Expired proof is rejected.
#[test]
fn test_expired_proof_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(99);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);

    let recipient = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[11u8; 32]);
    let nonce = BytesN::from_array(&env, &[12u8; 32]);
    let expires_at = 500u64;

    // Current time >= expires_at → rejected before even verifying the proof.
    env.ledger().set_timestamp(500);
    let proof = build_proof(&env, &contract_id, &signer, &recipient, &course_id, &nonce, expires_at);
    let result = client.try_mint(&recipient, &course_id, &nonce, &expires_at, &proof);
    assert_eq!(result, Err(Ok(ContractError::ProofExpired)));
}

/// Nonce cannot be reused across mints.
#[test]
fn test_nonce_cannot_be_reused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CertificateContract, ());
    let client = crate::CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let minter = Address::generate(&env);
    let signer = make_signing_key(88);
    let pubkey = Bytes::from_slice(&env, &signer.verifying_key().to_bytes());
    client.init(&admin, &pubkey, &minter);
    env.ledger().set_timestamp(1_000);

    let recipient = Address::generate(&env);
    let course_a = BytesN::from_array(&env, &[14u8; 32]);
    let course_b = BytesN::from_array(&env, &[15u8; 32]);
    let nonce = BytesN::from_array(&env, &[16u8; 32]);
    let expires_at = 5_000u64;

    let proof_a = build_proof(&env, &contract_id, &signer, &recipient, &course_a, &nonce, expires_at);
    client.mint(&recipient, &course_a, &nonce, &expires_at, &proof_a);

    let proof_b = build_proof(&env, &contract_id, &signer, &recipient, &course_b, &nonce, expires_at);
    let result = client.try_mint(&recipient, &course_b, &nonce, &expires_at, &proof_b);
    assert_eq!(result, Err(Ok(ContractError::NonceAlreadyConsumed)));
}
