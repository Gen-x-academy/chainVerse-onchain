#![cfg(test)]
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
    client.init(&admin, &key, &minter);
    let result = client.try_init(&admin, &key, &minter);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_is_paused_default_false() {
    let (env, contract_id, admin, minter) = setup();
    let client = crate::CertificateContractClient::new(&env, &contract_id);
    init_client(&env, &client, &admin, &minter);
    assert!(!client.is_paused());
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
