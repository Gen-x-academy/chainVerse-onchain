#![cfg(test)]

use escrow::{EscrowContract, EscrowContractClient, EscrowError, EscrowStatus};
use soroban_sdk::{testutils::{Address as _, Ledger}, token::StellarAssetClient, Address, Env};

fn setup(now: u64) -> (Env, Address, Address, Address, EscrowContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().with_mut(|li| li.timestamp = now);

    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_addr = env.register_stellar_asset_contract(token_admin.clone());
    StellarAssetClient::new(&env, &token_addr).mint(&Address::generate(&env), &0); // ensure token exists

    let admin = Address::generate(&env);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);

    StellarAssetClient::new(&env, &token_addr).mint(&buyer, &1_000);

    client.set_admin(&admin);
    client.whitelist_token(&token_addr);

    (env, buyer, seller, token_addr, client)
}

/// Happy path: escrow is created with correct stored fields.
#[test]
fn create_escrow_happy_path_stores_correct_record() {
    let (_env, buyer, seller, token, client) = setup(1_000);

    let id = client.create_escrow(&buyer, &seller, &token, &500, &9_000);

    let escrow = client.get_escrow(&id);
    assert_eq!(escrow.buyer, buyer);
    assert_eq!(escrow.seller, seller);
    assert_eq!(escrow.token, token);
    assert_eq!(escrow.amount, 500);
    assert_eq!(escrow.status, EscrowStatus::Pending);
    assert_eq!(escrow.expiration, 9_000);
}

/// Amount = 0 must be rejected with InvalidAmount.
#[test]
fn create_escrow_rejects_zero_amount() {
    let (_env, buyer, seller, token, client) = setup(1_000);
    let result = client.try_create_escrow(&buyer, &seller, &token, &0, &9_000);
    assert_eq!(result, Err(Ok(EscrowError::InvalidAmount)));
}

/// Negative amount must be rejected with InvalidAmount.
#[test]
fn create_escrow_rejects_negative_amount() {
    let (_env, buyer, seller, token, client) = setup(1_000);
    let result = client.try_create_escrow(&buyer, &seller, &token, &-1, &9_000);
    assert_eq!(result, Err(Ok(EscrowError::InvalidAmount)));
}

/// Expiry at or before current ledger timestamp must be rejected.
#[test]
fn create_escrow_rejects_expired_expiration() {
    let (_env, buyer, seller, token, client) = setup(1_000);
    // expiration == current timestamp
    let result = client.try_create_escrow(&buyer, &seller, &token, &100, &1_000);
    assert_eq!(result, Err(Ok(EscrowError::InvalidExpiration)));
}

/// Buyer == seller must be rejected with InvalidRecipient.
#[test]
fn create_escrow_rejects_buyer_equals_seller() {
    let (_env, buyer, _seller, token, client) = setup(1_000);
    let result = client.try_create_escrow(&buyer, &buyer, &token, &100, &9_000);
    assert_eq!(result, Err(Ok(EscrowError::InvalidRecipient)));
}

/// Token not whitelisted must be rejected with TokenNotAllowed.
#[test]
fn create_escrow_rejects_non_whitelisted_token() {
    let (env, buyer, seller, _token, client) = setup(1_000);
    let unlisted_token_admin = Address::generate(&env);
    let unlisted_token = env.register_stellar_asset_contract(unlisted_token_admin.clone());
    StellarAssetClient::new(&env, &unlisted_token).mint(&buyer, &1_000);

    let result = client.try_create_escrow(&buyer, &seller, &unlisted_token, &100, &9_000);
    assert_eq!(result, Err(Ok(EscrowError::TokenNotAllowed)));
}
