#![cfg(test)]

//! Acceptance tests for admin fee withdrawal with strict liability separation
//! (#860): withdrawing accrued protocol fees must never touch principal
//! (locked escrow) funds.

use escrow::{EscrowContract, EscrowContractClient, EscrowError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

struct TestCtx {
    env: Env,
    admin: Address,
    buyer: Address,
    seller: Address,
    token: Address,
    client: EscrowContractClient<'static>,
}

fn setup(now: u64, fee_bps: u32) -> TestCtx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|li| li.timestamp = now);

    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract(token_admin.clone());

    let admin = Address::generate(&env);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&buyer, &10_000);

    client.set_admin(&admin);
    client.whitelist_token(&admin, &token);
    client.set_protocol_fee_bps(&admin, &fee_bps);

    TestCtx {
        env,
        admin,
        buyer,
        seller,
        token,
        client,
    }
}

fn token_balance(env: &Env, token: &Address, account: &Address) -> i128 {
    TokenClient::new(env, token).balance(account)
}

/// #860 — admin can withdraw accrued fees; the fee pool is decremented and the
/// recipient receives the exact amount, while escrow principal is untouched.
#[test]
fn test_withdraw_fees_transfers_only_fee_pool() {
    let ctx = setup(1_000, 1_000); // 10% protocol fee.
    let recipient = Address::generate(&ctx.env);

    // Deposit 500 into escrow and fully release it: 10% = 50 accrues as fees.
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.release_escrow(&ctx.buyer, &id);

    // After release the escrow is Completed: locked amount is 0, seller got 450,
    // and 50 remains in the contract's fee pool.
    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 50);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        50
    );

    ctx.client.withdraw_fees(&ctx.admin, &ctx.token, &recipient, &50);

    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 0);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &recipient),
        50
    );
    // Principal escrow funds were never moved: contract holds nothing now.
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        0
    );
}

/// #860 — withdrawing more than the accrued fee pool is rejected, and the
/// accrued pool (and principal) are left untouched.
#[test]
fn test_withdraw_fees_over_accrued_rejected() {
    let ctx = setup(1_000, 1_000);
    let recipient = Address::generate(&ctx.env);

    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.release_escrow(&ctx.buyer, &id);

    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 50);

    let result = ctx.client.try_withdraw_fees(&ctx.admin, &ctx.token, &recipient, &100);
    assert_eq!(result, Err(Ok(EscrowError::NoFeesAvailable)));

    // Fee pool unchanged; recipient got nothing.
    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 50);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &recipient),
        0
    );
}

/// #860 — withdrawing from a token with no accrued fees is rejected.
#[test]
fn test_withdraw_fees_from_empty_pool_rejected() {
    let ctx = setup(1_000, 1_000);
    let recipient = Address::generate(&ctx.env);

    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 0);
    let result = ctx.client.try_withdraw_fees(&ctx.admin, &ctx.token, &recipient, &10);
    assert_eq!(result, Err(Ok(EscrowError::NoFeesAvailable)));
}

/// #860 — non-admin callers are rejected.
#[test]
fn test_withdraw_fees_non_admin_rejected() {
    let ctx = setup(1_000, 0);
    let recipient = Address::generate(&ctx.env);

    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.release_escrow(&ctx.buyer, &id);

    // Still funds the escrow so an accrued balance exists, then a stranger
    // attempts the withdrawal (fee_bps=0 so pool is 0, but auth check fails first).
    let stranger = Address::generate(&ctx.env);
    let result = ctx.client.try_withdraw_fees(&stranger, &ctx.token, &recipient, &1);
    assert_eq!(result, Err(Ok(EscrowError::Unauthorized)));
}

/// #860 — zero/negative withdraw amounts are rejected.
#[test]
fn test_withdraw_fees_zero_amount_rejected() {
    let ctx = setup(1_000, 1_000);
    let recipient = Address::generate(&ctx.env);

    // Mint fees by fully releasing an escrow.
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.release_escrow(&ctx.buyer, &id);

    let result = ctx.client.try_withdraw_fees(&ctx.admin, &ctx.token, &recipient, &0);
    assert_eq!(result, Err(Ok(EscrowError::InvalidAmount)));
}

/// #860 — principal locked in an active escrow is never touched by a fee
/// withdrawal even when there are accrued fees to withdraw.
#[test]
fn test_withdraw_fees_does_not_touch_active_lock() {
    let ctx = setup(1_000, 1_000);
    let recipient = Address::generate(&ctx.env);

    // One escrow released (accrues 50 in fees), one still actively locked (500).
    let id1 = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id1);
    ctx.client.release_escrow(&ctx.buyer, &id1);

    let id2 = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &300, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id2);

    // Contract holds 50 (fee pool) + 300 (active lock) = 350.
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        350
    );

    // Withdraw only the fee pool.
    ctx.client.withdraw_fees(&ctx.admin, &ctx.token, &recipient, &50);

    // Recipient got exactly the fee pool; the active escrow lock is untouched.
    assert_eq!(token_balance(&ctx.env, &ctx.token, &recipient), 50);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        300
    );
    assert_eq!(ctx.client.get_protocol_fee(&ctx.token), 0);
    assert_eq!(ctx.client.get_escrow(&id2).unwrap().amount, 300);
}
