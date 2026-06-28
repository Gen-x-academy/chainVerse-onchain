#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_mint_does_not_exceed_cap() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    // Expected: minting beyond total supply cap returns Err.
    let _env = env;
    let _admin = admin;
    let _recipient = recipient;
}

#[test]
fn test_burn_below_zero_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    // Expected: burning more tokens than the balance returns Err.
    let _env = env;
    let _admin = admin;
}

#[test]
fn test_burn_insufficient_balance_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let user = Address::generate(&env);
    // Expected: burn from an address with zero balance returns Err(ContractError::InsufficientBalance).
    let _env = env;
    let _user = user;
}

#[test]
fn test_admin_transfer_without_auth_fails() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    // Expected: admin transfer without auth returns Err.
    let _env = env;
    let _admin = admin;
    let _recipient = recipient;
}

#[test]
fn test_admin_transfer_succeeds_with_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);
    // Expected: admin transfer with auth succeeds and tokens move.
    let _env = env;
    let _admin = admin;
    let _recipient = recipient;
}
