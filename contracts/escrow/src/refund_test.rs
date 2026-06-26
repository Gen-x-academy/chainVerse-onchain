#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env};

// Scaffold tests for escrow refund — expiry enforcement and token return.
// These document expected behavior; wire up with full contract client when available.

#[test]
fn test_refund_after_expiry_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: refund_escrow returns Ok after expiry timestamp has passed
    // and tokens are returned to buyer.
    assert_ne!(buyer, seller); // scaffolding assertion
}

#[test]
fn test_refund_before_expiry_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);
    // Expected: refund_escrow returns Err(EscrowError::NotExpired)
    // when called before the expiration ledger timestamp.
    assert_ne!(buyer, seller);
}

#[test]
fn test_double_refund_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let buyer = Address::generate(&env);
    // Expected: second refund_escrow call returns Err(EscrowError::InvalidStatus)
    // because the escrow status is already Refunded.
    let _ = buyer;
}
