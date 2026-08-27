#![cfg(test)]

use escrow::{EscrowContract, EscrowContractClient, EscrowError, EscrowStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env,
};

struct TestCtx {
    env: Env,
    admin: Address,
    buyer: Address,
    seller: Address,
    arbiter: Address,
    token: Address,
    client: EscrowContractClient<'static>,
}

fn setup(now: u64) -> TestCtx {
    setup_inner(now, true)
}

fn setup_without_arbiter(now: u64) -> TestCtx {
    setup_inner(now, false)
}

fn setup_inner(now: u64, with_arbiter: bool) -> TestCtx {
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
    let arbiter = Address::generate(&env);

    StellarAssetClient::new(&env, &token).mint(&buyer, &10_000);

    client.set_admin(&admin);
    if with_arbiter {
        client.set_arbiter(&admin, &arbiter);
    }
    client.whitelist_token(&admin, &token);
    client.set_protocol_fee_bps(&admin, &0);

    TestCtx {
        env,
        admin,
        buyer,
        seller,
        arbiter,
        token,
        client,
    }
}

fn token_balance(env: &Env, token: &Address, account: &Address) -> i128 {
    TokenClient::new(env, token).balance(account)
}

fn reason(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

fn create_and_fund_disputed(ctx: &TestCtx, amount: i128) -> u64 {
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &amount, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.dispute_escrow(&ctx.buyer, &id);
    id
}

// ---------- Issue #864: dispute resolution ----------

#[test]
fn test_resolve_dispute_allocates_funds_between_buyer_and_seller() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    let res = ctx
        .client
        .resolve_dispute(&ctx.arbiter, &id, &600, &400, &0, &reason(&ctx.env));
    assert!(res.is_ok());

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
    assert_eq!(escrow.amount, 0);
    // Buyer minted 10_000, funded 1_000, then recovered 600.
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 9_600);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 400);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.client.address), 0);
}

#[test]
fn test_resolve_dispute_accepts_zero_allocations() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    let res = ctx
        .client
        .resolve_dispute(&ctx.arbiter, &id, &1_000, &0, &0, &reason(&ctx.env));
    assert!(res.is_ok());

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 10_000);
}

#[test]
fn test_resolve_dispute_fails_without_arbiter_configured() {
    let ctx = setup_without_arbiter(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    let res = ctx.client.try_resolve_dispute(
        &ctx.arbiter,
        &id,
        &500,
        &500,
        &0,
        &reason(&ctx.env),
    );
    assert_eq!(res, Err(Ok(EscrowError::NoArbiterConfigured)));
}

#[test]
fn test_resolve_dispute_fails_for_non_arbiter() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    let stranger = Address::generate(&ctx.env);
    let res = ctx.client.try_resolve_dispute(
        &stranger,
        &id,
        &500,
        &500,
        &0,
        &reason(&ctx.env),
    );
    assert_eq!(res, Err(Ok(EscrowError::Unauthorized)));
}

#[test]
fn test_resolve_dispute_fails_when_not_disputed() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &1_000, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);

    let res = ctx.client.try_resolve_dispute(
        &ctx.arbiter,
        &id,
        &500,
        &500,
        &0,
        &reason(&ctx.env),
    );
    assert_eq!(res, Err(Ok(EscrowError::NotDisputed)));
}

#[test]
fn test_resolve_dispute_fails_on_overflow_allocation() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    // Sum + fee exceeds the remaining escrow amount.
    let res = ctx.client.try_resolve_dispute(
        &ctx.arbiter,
        &id,
        &600,
        &500,
        &0,
        &reason(&ctx.env),
    );
    assert_eq!(res, Err(Ok(EscrowError::InvalidAllocation)));
}

#[test]
fn test_resolve_dispute_fails_on_negative_allocation() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    let res = ctx.client.try_resolve_dispute(
        &ctx.arbiter,
        &id,
        &-1,
        &500,
        &0,
        &reason(&ctx.env),
    );
    assert_eq!(res, Err(Ok(EscrowError::InvalidAllocation)));
}

#[test]
fn test_resolve_dispute_accumulates_fee_into_protocol_fees() {
    let ctx = setup(1_000);
    let id = create_and_fund_disputed(&ctx, 1_000);

    // 400 to buyer, 500 to seller, 100 resolution fee = 1_000 total.
    let res = ctx
        .client
        .resolve_dispute(&ctx.arbiter, &id, &400, &500, &100, &reason(&ctx.env));
    assert!(res.is_ok());

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 9_400);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 500);
    // 100 stays locked in the contract as accumulated protocol fees.
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.client.address), 100);
}

// ---------- Issue #862: original_amount preservation ----------

#[test]
fn test_original_amount_preserved_through_partial_and_full_release() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &1_000, &9_000);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.original_amount, 1_000);
    assert_eq!(escrow.amount, 1_000);

    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.partial_release(&ctx.buyer, &id, &300);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.original_amount, 1_000);
    assert_eq!(escrow.amount, 700);

    ctx.client.partial_release(&ctx.buyer, &id, &700);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.original_amount, 1_000);
    assert_eq!(escrow.amount, 0);
}

// ---------- Issue #861: fee arithmetic and bps validation ----------

#[test]
fn test_protocol_fee_bps_cap_is_enforced() {
    let ctx = setup(1_000);
    // 5000 bps (50%) is the max; anything above is rejected.
    let res = ctx.client.try_set_protocol_fee_bps(&ctx.admin, &5_001);
    assert_eq!(res, Err(Ok(EscrowError::Unauthorized)));
    // 5000 is allowed.
    let res = ctx.client.try_set_protocol_fee_bps(&ctx.admin, &5_000);
    assert!(res.is_ok());
}

#[test]
fn test_fee_uses_consistent_truncation() {
    let ctx = setup(1_000);
    ctx.client.set_protocol_fee_bps(&ctx.admin, &1_000); // 10%

    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &105, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.partial_release(&ctx.buyer, &id, &105);

    // fee = 105 * 1000 / 10000 = 10 (truncated), seller gets 95.
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 95);
}

#[test]
fn test_no_negative_seller_amount_at_max_fee() {
    let ctx = setup(1_000);
    ctx.client.set_protocol_fee_bps(&ctx.admin, &5_000); // 50% max

    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &100, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);
    ctx.client.partial_release(&ctx.buyer, &id, &100);

    // fee = 100 * 5000 / 10000 = 50, seller gets 50 (>= 0).
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 50);
    // No negative payout could be produced by the checked arithmetic.
    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.amount, 0);
}

// ---------- Issue #863: events (behavioral assertions) ----------

#[test]
fn test_fund_and_dispute_result_in_expected_state() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);

    ctx.client.fund_escrow(&ctx.buyer, &id);
    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);

    ctx.client.dispute_escrow(&ctx.buyer, &id);
    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Disputed);
}

#[test]
fn test_partial_release_moves_funds_and_reduces_locked_amount() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);
    ctx.client.fund_escrow(&ctx.buyer, &id);

    ctx.client.partial_release(&ctx.buyer, &id, &200);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(escrow.amount, 300);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 200);
}
