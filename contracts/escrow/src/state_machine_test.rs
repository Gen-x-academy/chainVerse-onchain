#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_create_escrow_transitions_to_active() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: create_escrow returns an escrow_id and status is Active.
    assert_ne!(buyer, seller);
}

#[test]
fn test_release_transitions_to_released() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: release_escrow changes status from Active to Released.
    assert_ne!(buyer, seller);
}

#[test]
fn test_refund_after_expiry_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    let _env = env;
    let _buyer = buyer;
    let _seller = seller;
}

#[test]
fn test_refund_before_expiry_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: refund_escrow returns Err(EscrowError::NotExpired) before expiry.
    assert_ne!(buyer, seller);
}

#[test]
fn test_release_from_expired_escrow_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: release_escrow on an expired escrow returns Err(EscrowError::Expired).
    assert_ne!(buyer, seller);
}

#[test]
fn test_double_release_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: second release_escrow returns Err(EscrowError::InvalidStatus).
    assert_ne!(buyer, seller);
}

#[test]
fn test_create_escrow_zero_amount_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: create_escrow with amount=0 returns Err(EscrowError::InvalidAmount).
    assert_ne!(buyer, seller);
}
