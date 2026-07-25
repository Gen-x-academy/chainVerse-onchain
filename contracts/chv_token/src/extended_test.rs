#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{CHVToken, CHVTokenClient, TokenError};

/// Hard cap constant mirrored from lib.rs for use in tests.
const MAX_SUPPLY: i128 = 1_000_000_000 * 10_i128.pow(7);

/// Treasury receives this as initial supply on initialize().
const INITIAL_SUPPLY: i128 = 100_000_000 * 10_i128.pow(7);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register the contract, mock all auths, initialise, and return
/// (env, contract_id, admin, treasury) — matching the pattern in test.rs.
fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CHVToken);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    (env, contract_id, admin, treasury)
}

// ---------------------------------------------------------------------------
// Mint-cap tests (issue #650 — supply cap enforcement)
// ---------------------------------------------------------------------------

/// Minting exactly the remaining supply up to MAX_SUPPLY should succeed.
#[test]
fn test_mint_up_to_cap_succeeds() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let remaining = MAX_SUPPLY - INITIAL_SUPPLY;
    let recipient = Address::generate(&env);
    let result = client.try_mint(&admin, &recipient, &remaining);
    assert!(
        result.is_ok(),
        "minting exactly the remaining supply should succeed"
    );
    assert_eq!(client.total_minted(), MAX_SUPPLY);
}

/// Minting a single token beyond MAX_SUPPLY must return SupplyCapExceeded.
#[test]
fn test_mint_cap_enforced() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    // Try to push total_minted one token over the cap.
    let over_cap = MAX_SUPPLY - INITIAL_SUPPLY + 1;
    let result = client.try_mint(&admin, &treasury, &over_cap);
    assert_eq!(
        result,
        Err(Ok(TokenError::SupplyCapExceeded)),
        "minting above MAX_SUPPLY must return SupplyCapExceeded"
    );
}

/// Minting zero tokens is always invalid regardless of the cap state.
#[test]
fn test_mint_zero_amount_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let result = client.try_mint(&admin, &treasury, &0_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InvalidAmount)),
        "minting zero must return InvalidAmount"
    );
}

// ---------------------------------------------------------------------------
// Burn underflow tests (issue #650 — underflow protection)
// ---------------------------------------------------------------------------

/// Burning more than an account's balance must return InsufficientBalance.
#[test]
fn test_burn_underflow_returns_error() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    // Give the user a small balance, then try to burn one more than they have.
    client.mint(&admin, &user, &500_i128);
    let result = client.try_burn(&user, &501_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InsufficientBalance)),
        "burning more than balance must return InsufficientBalance"
    );
}

/// Burning from an account with zero balance must return InsufficientBalance.
#[test]
fn test_burn_insufficient_balance_fails() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    // user has never received tokens — any positive burn must fail.
    let result = client.try_burn(&user, &1_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InsufficientBalance)),
        "burning from a zero-balance account must return InsufficientBalance"
    );
}

/// Burning zero tokens is always invalid (InvalidAmount guard runs before balance check).
#[test]
fn test_burn_zero_amount_rejected() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let result = client.try_burn(&user, &0_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InvalidAmount)),
        "burning zero must return InvalidAmount"
    );
}

/// A successful burn reduces the account balance by exactly the burned amount.
#[test]
fn test_burn_reduces_balance() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.mint(&admin, &user, &1_000_i128);
    client.burn(&user, &400_i128);
    assert_eq!(
        client.balance(&user),
        600_i128,
        "balance after burn must equal initial minus burned amount"
    );
}

// ---------------------------------------------------------------------------
// Admin transfer tests (issue #650 — two-step admin transfer safety)
// ---------------------------------------------------------------------------

/// Full happy path: propose then accept hands control to the new admin.
#[test]
fn test_admin_transfer_succeeds_with_auth() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    // Step 1 — current admin proposes.
    client.propose_admin(&admin, &new_admin);
    // Step 2 — new admin accepts.
    client.accept_admin(&new_admin);

    // Verify: new admin can mint (fails with Unauthorized if transfer was wrong).
    let recipient = Address::generate(&env);
    let result = client.try_mint(&new_admin, &recipient, &1_000_i128);
    assert!(
        result.is_ok(),
        "new admin must be able to mint after successful two-step transfer"
    );
}

/// The original admin must lose minting rights after the transfer completes.
#[test]
fn test_old_admin_cannot_mint_after_transfer() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    let recipient = Address::generate(&env);
    let result = client.try_mint(&admin, &recipient, &1_000_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "old admin must be rejected after admin transfer"
    );
}

/// An address that was not proposed cannot accept the admin slot.
#[test]
fn test_wrong_address_cannot_accept_admin() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);

    let result = client.try_accept_admin(&impostor);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "an address that was not proposed must not be able to accept admin"
    );
}

/// Calling accept_admin when no proposal is pending returns NoPendingAdmin.
#[test]
fn test_accept_admin_without_proposal_fails() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let random = Address::generate(&env);

    let result = client.try_accept_admin(&random);
    assert_eq!(
        result,
        Err(Ok(TokenError::NoPendingAdmin)),
        "accepting admin with no pending proposal must return NoPendingAdmin"
    );
}

/// A non-admin cannot propose a new admin.
#[test]
fn test_non_admin_cannot_propose_admin() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let result = client.try_propose_admin(&attacker, &target);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "a non-admin address must not be able to propose a new admin"
    );
}
