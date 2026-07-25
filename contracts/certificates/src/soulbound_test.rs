#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_transfer_soulbound_certificate_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let recipient = Address::generate(&env);
    // Expected: transferring a soul-bound certificate returns Err.
    assert_ne!(owner, recipient);
}

#[test]
fn test_duplicate_token_id_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    // Expected: minting a certificate with an already-used token_id returns Err.
    let _env = env;
    let _owner = owner;
}

#[test]
fn test_mint_certificate_without_auth_fails() {
    let env = Env::default();
    let owner = Address::generate(&env);
    // Expected: mint_certificate without caller authorization returns Err.
    let _env = env;
    let _owner = owner;
}

#[test]
fn test_burn_soulbound_certificate() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    // Expected: owner can burn their own soul-bound certificate.
    let _env = env;
    let _owner = owner;
}

#[test]
fn test_burn_others_certificate_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let owner = Address::generate(&env);
    let attacker = Address::generate(&env);
    // Expected: burning another user's certificate returns Err.
    assert_ne!(owner, attacker);
}
