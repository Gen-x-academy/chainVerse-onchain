#![cfg(test)]
use crate::{
    ContractError, IntentStatus, ScholarshipDisbursementsContract,
};
use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipDisbursementsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn network_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[3u8; 32])
}

fn program_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn keypair() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

/// Registers the wallet key and completes a challenge for the wallet that
/// is *already* configured for the recipient, proving ownership without
/// changing the wallet epoch.
fn prove_wallet(
    env: &Env,
    contract_id: &Address,
    recipient: &Address,
    wallet: &Address,
    sk: &SigningKey,
) {
    let client = crate::ScholarshipDisbursementsContractClient::new(env, contract_id);
    let pubkey = BytesN::from_array(env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(wallet, &pubkey);
    let id = client.open_wallet_challenge(recipient, wallet, &3_600u64);
    let payload = client.wallet_challenge_payload(&id);
    let signature = sk.sign(&payload.to_array());
    let sig_bn = BytesN::from_array(env, &signature.to_bytes());
    client.confirm_wallet_challenge(&id, &pubkey, &sig_bn);
}

/// Configures the wallet and then proves ownership of it.
fn verify_wallet(
    env: &Env,
    contract_id: &Address,
    recipient: &Address,
    wallet: &Address,
    sk: &SigningKey,
) {
    let client = crate::ScholarshipDisbursementsContractClient::new(env, contract_id);
    client.set_payout_wallet(recipient, wallet);
    prove_wallet(env, contract_id, recipient, wallet, sk);
}

// ── #1097 — wallet ownership challenge ─────────────────────────────────────

#[test]
fn test_confirming_challenge_verifies_wallet() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    assert!(!client.is_wallet_verified(&recipient));

    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());

    assert!(client.is_wallet_verified(&recipient));
    let binding = client.get_payout_wallet(&recipient);
    assert!(binding.verified);
    assert_eq!(binding.wallet, wallet);
    assert_eq!(binding.epoch, 1);
}

#[test]
fn test_expired_challenge_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    let sk = keypair();
    let pubkey = BytesN::from_array(&env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(&wallet, &pubkey);
    client.set_payout_wallet(&recipient, &wallet);

    let id = client.open_wallet_challenge(&recipient, &wallet, &100u64);
    env.ledger().set_timestamp(101);

    let sig_bn = BytesN::from_array(&env, &[0u8; 64]);
    let result = client.try_confirm_wallet_challenge(&id, &pubkey, &sig_bn);
    assert_eq!(result, Err(Ok(ContractError::ChallengeExpired)));
}

#[test]
fn test_challenge_is_single_use() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    let sk = keypair();
    let pubkey = BytesN::from_array(&env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(&wallet, &pubkey);
    client.set_payout_wallet(&recipient, &wallet);

    let id = client.open_wallet_challenge(&recipient, &wallet, &3_600u64);
    let payload = client.wallet_challenge_payload(&id);
    let sig_bn = BytesN::from_array(&env, &sk.sign(&payload.to_array()).to_bytes());
    client.confirm_wallet_challenge(&id, &pubkey, &sig_bn);

    let result = client.try_confirm_wallet_challenge(&id, &pubkey, &sig_bn);
    assert_eq!(result, Err(Ok(ContractError::ChallengeAlreadyUsed)));
}

#[test]
fn test_bad_signature_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    let sk = keypair();
    let pubkey = BytesN::from_array(&env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(&wallet, &pubkey);
    client.set_payout_wallet(&recipient, &wallet);

    let id = client.open_wallet_challenge(&recipient, &wallet, &3_600u64);
    // Sign a different message than the contract-derived payload.
    let wrong = BytesN::from_array(&env, &[0u8; 32]);
    let sig_bn = BytesN::from_array(&env, &sk.sign(&wrong.to_array()).to_bytes());

    let result = client.try_confirm_wallet_challenge(&id, &pubkey, &sig_bn);
    assert_eq!(result, Err(Ok(ContractError::InvalidSignature)));
}

#[test]
fn test_pubkey_must_match_registered_key() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    let sk = keypair();
    let registered = BytesN::from_array(&env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(&wallet, &registered);
    client.set_payout_wallet(&recipient, &wallet);
    let id = client.open_wallet_challenge(&recipient, &wallet, &3_600u64);

    let other = SigningKey::from_bytes(&[9u8; 32]);
    let other_pub = BytesN::from_array(&env, &other.verifying_key().to_bytes());
    let payload = client.wallet_challenge_payload(&id);
    let sig_bn = BytesN::from_array(&env, &other.sign(&payload.to_array()).to_bytes());

    let result = client.try_confirm_wallet_challenge(&id, &other_pub, &sig_bn);
    assert_eq!(result, Err(Ok(ContractError::WalletKeyMismatch)));
}

#[test]
fn test_challenges_are_domain_separated_by_id() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    let sk = keypair();
    let pubkey = BytesN::from_array(&env, &sk.verifying_key().to_bytes());
    client.register_wallet_key(&wallet, &pubkey);
    client.set_payout_wallet(&recipient, &wallet);

    let first = client.open_wallet_challenge(&recipient, &wallet, &3_600u64);
    let second = client.open_wallet_challenge(&recipient, &wallet, &3_600u64);
    assert_ne!(
        client.wallet_challenge_payload(&first),
        client.wallet_challenge_payload(&second)
    );
}

#[test]
fn test_challenge_requires_configured_wallet() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let configured = Address::generate(&env);
    let other = Address::generate(&env);
    client.set_payout_wallet(&recipient, &configured);

    let result = client.try_open_wallet_challenge(&recipient, &other, &3_600u64);
    assert_eq!(result, Err(Ok(ContractError::WalletMismatch)));
}

#[test]
fn test_invalid_challenge_ttl_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    client.set_payout_wallet(&recipient, &wallet);

    assert_eq!(
        client.try_open_wallet_challenge(&recipient, &wallet, &0u64),
        Err(Ok(ContractError::InvalidChallengeTtl))
    );
    assert_eq!(
        client.try_open_wallet_challenge(&recipient, &wallet, &1_000_000u64),
        Err(Ok(ContractError::InvalidChallengeTtl))
    );
}

// ── #1096 — idempotent intents ─────────────────────────────────────────────

#[test]
fn test_create_intent_is_idempotent() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    let pid = program_id(&env, 1);

    let first = client.create_intent(&admin, &pid, &recipient, &1_000i128, &0u32);
    let second = client.create_intent(&admin, &pid, &recipient, &1_000i128, &0u32);
    assert_eq!(first, second);

    let intent = client.get_intent(&first);
    assert_eq!(intent.amount, 1_000);
    assert_eq!(intent.status, IntentStatus::Pending);
    assert_eq!(intent.wallet, wallet);
}

#[test]
fn test_create_intent_conflicts_on_different_amount() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    let pid = program_id(&env, 1);

    client.create_intent(&admin, &pid, &recipient, &1_000i128, &0u32);
    let result = client.try_create_intent(&admin, &pid, &recipient, &2_000i128, &0u32);
    assert_eq!(result, Err(Ok(ContractError::IntentConflict)));
}

#[test]
fn test_intent_requires_configured_wallet() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let result = client.try_create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
    assert_eq!(result, Err(Ok(ContractError::WalletNotFound)));
}

#[test]
fn test_invalid_amount_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());

    let result =
        client.try_create_intent(&admin, &program_id(&env, 1), &recipient, &0i128, &0u32);
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn test_unauthorized_creator_rejected() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());

    let stranger = Address::generate(&env);
    let result =
        client.try_create_intent(&stranger, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
    assert_eq!(result, Err(Ok(ContractError::NotAuthorizedCreator)));
}

#[test]
fn test_allowlisted_creator_can_create() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let creator = Address::generate(&env);
    client.add_creator(&admin, &creator);
    assert!(client.is_authorized_creator(&creator));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());

    client.create_intent(&creator, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
}

#[test]
fn test_non_admin_cannot_add_creator() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let attacker = Address::generate(&env);
    let target = Address::generate(&env);
    assert_eq!(
        client.try_add_creator(&attacker, &target),
        Err(Ok(ContractError::NotAdmin))
    );
}

// ── execution + safe hold ──────────────────────────────────────────────────

#[test]
fn test_record_execution_only_once() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    let id = client.create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);

    assert!(client.is_intent_executable(&id));
    client.record_execution(&admin, &id);
    assert!(!client.is_intent_executable(&id));

    let result = client.try_record_execution(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::AlreadyExecuted)));
}

#[test]
fn test_unverified_wallet_blocks_execution() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    // Configure the wallet but do not prove ownership.
    client.set_payout_wallet(&recipient, &wallet);

    let id = client.create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
    assert!(!client.is_intent_executable(&id));

    let result = client.try_record_execution(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::IntentOnHold)));
}

#[test]
fn test_changing_wallet_places_intent_on_hold() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    let id = client.create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
    assert!(client.is_intent_executable(&id));

    // Address change: epoch bumps, verification clears, intent goes on hold.
    let new_wallet = Address::generate(&env);
    client.set_payout_wallet(&recipient, &new_wallet);
    assert!(!client.is_wallet_verified(&recipient));
    assert_eq!(client.get_payout_wallet(&recipient).epoch, 2);
    assert!(!client.is_intent_executable(&id));

    let result = client.try_record_execution(&admin, &id);
    assert_eq!(result, Err(Ok(ContractError::IntentOnHold)));

    // The immutable intent still targets the old wallet.
    assert_eq!(client.get_intent(&id).wallet, wallet);
}

#[test]
fn test_reverifying_same_wallet_releases_pending_intent() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    client.set_payout_wallet(&recipient, &wallet);
    let id = client.create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);
    assert!(!client.is_intent_executable(&id));

    // Verify the *already-configured* wallet: the pending intent becomes
    // executable, without bumping the wallet epoch.
    prove_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    assert!(client.is_intent_executable(&id));
    client.record_execution(&admin, &id);
}

#[test]
fn test_cancel_intent_keeps_record() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipDisbursementsContractClient::new(&env, &contract_id);
    client.initialize(&admin, &network_id(&env));

    let recipient = Address::generate(&env);
    let wallet = Address::generate(&env);
    verify_wallet(&env, &contract_id, &recipient, &wallet, &keypair());
    let id = client.create_intent(&admin, &program_id(&env, 1), &recipient, &1_000i128, &0u32);

    client.cancel_intent(&admin, &id);
    let intent = client.get_intent(&id);
    assert_eq!(intent.status, IntentStatus::Cancelled);

    assert_eq!(
        client.try_record_execution(&admin, &id),
        Err(Ok(ContractError::AlreadyCancelled))
    );
    assert_eq!(
        client.try_cancel_intent(&admin, &id),
        Err(Ok(ContractError::AlreadyCancelled))
    );
}
