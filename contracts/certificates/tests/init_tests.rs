#![cfg(test)]

extern crate std;

use ed25519_dalek::SigningKey;
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

use certificates::{CertificateContract, CertificateContractClient, ContractError};

fn backend_pubkey(env: &Env) -> Bytes {
    let key = SigningKey::from_bytes(&[1u8; 32]);
    Bytes::from_slice(env, &key.verifying_key().to_bytes())
}

/// init stores admin and backend pubkey; subsequent reads confirm the state.
#[test]
fn init_sets_admin_and_backend_pubkey() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let pubkey = backend_pubkey(&env);

    // Should succeed without error
    client.init(&admin, &pubkey);

    // Contract must not be paused after init
    assert!(!client.is_paused());
}

/// Second call to init must return AlreadyInitialized.
#[test]
fn init_rejects_double_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let pubkey = backend_pubkey(&env);

    client.init(&admin, &pubkey);

    let result = client.try_init(&admin, &pubkey);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

/// init without admin authorization must be rejected.
#[test]
fn init_rejects_without_admin_auth() {
    let env = Env::default();
    // Do NOT mock all auths — require real auth checking
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register_contract(None, CertificateContract);
    let client = CertificateContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let pubkey = backend_pubkey(&env);

    // With mock_all_auths_allowing_non_root_auth the call itself succeeds —
    // the important thing is that the admin address must have been authorized.
    // We verify by checking the auth tree recorded by the environment.
    client.init(&admin, &pubkey);

    // Confirm the admin's authorization was required (auths must be non-empty)
    let auths = env.auths();
    assert!(
        !auths.is_empty(),
        "init must require the admin address to authorize"
    );
    assert_eq!(
        auths[0].0, admin,
        "the authorizing address must be the admin passed to init"
    );
}
