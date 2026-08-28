#![cfg(test)]

use escrow::{EscrowContract, EscrowContractClient, EscrowError, EscrowStatus};
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

fn setup(now: u64) -> TestCtx {
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
    client.set_protocol_fee_bps(&admin, &0);

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

fn create_and_fund(ctx: &TestCtx, amount: i128, expiration: u64) -> u64 {
    let id = ctx.client.create_escrow(
        &ctx.buyer,
        &ctx.seller,
        &ctx.token,
        &amount,
        &expiration,
    );
    ctx.client.fund_escrow(&ctx.buyer, &id);
    id
}

#[test]
fn test_create_escrow() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);

    let escrow = ctx.client.get_escrow(&id).expect("escrow exists");
    assert_eq!(escrow.buyer, ctx.buyer);
    assert_eq!(escrow.seller, ctx.seller);
    assert_eq!(escrow.token, ctx.token);
    assert_eq!(escrow.amount, 500);
    assert_eq!(escrow.status, EscrowStatus::Created);
    assert_eq!(escrow.expiration, 9_000);
    // Unfunded: buyer still holds tokens; contract holds none of this deposit.
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 10_000);
}

#[test]
fn test_fund_escrow_by_buyer() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);

    ctx.client.fund_escrow(&ctx.buyer, &id);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 9_500);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        500
    );
}

#[test]
fn test_fund_escrow_by_non_buyer_fails() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);

    let stranger = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&stranger, &1_000);

    let result = ctx.client.try_fund_escrow(&stranger, &id);
    assert_eq!(result, Err(Ok(EscrowError::Unauthorized)));

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Created);
}

#[test]
fn test_release_funded_escrow() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    ctx.client.release_escrow(&ctx.buyer, &id);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
    assert_eq!(escrow.amount, 0);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 500);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        0
    );
}

#[test]
fn test_release_unfunded_escrow_fails() {
    let ctx = setup(1_000);
    let id = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &500, &9_000);

    let result = ctx.client.try_release_escrow(&ctx.buyer, &id);
    assert_eq!(result, Err(Ok(EscrowError::InvalidEscrowState)));
}

#[test]
fn test_release_already_released_fails() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    ctx.client.release_escrow(&ctx.buyer, &id);
    let result = ctx.client.try_release_escrow(&ctx.buyer, &id);
    assert_eq!(result, Err(Ok(EscrowError::AlreadyReleased)));
}

#[test]
fn test_dispute_funded_escrow() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    ctx.client.dispute_escrow(&ctx.buyer, &id);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Disputed);

    // Release must be blocked while disputed.
    let result = ctx.client.try_release_escrow(&ctx.buyer, &id);
    assert_eq!(result, Err(Ok(EscrowError::InvalidEscrowState)));
}

#[test]
fn test_dispute_released_escrow_fails() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    ctx.client.release_escrow(&ctx.buyer, &id);
    let result = ctx.client.try_dispute_escrow(&ctx.buyer, &id);
    // A released escrow is no longer Funded, so dispute is rejected with
    // InvalidEscrowState (dispute is only openable on a Funded escrow).
    assert_eq!(result, Err(Ok(EscrowError::InvalidEscrowState)));
}

#[test]
fn test_refund_expired_after_deadline() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 2_000);

    ctx.env.ledger().with_mut(|li| li.timestamp = 2_000);

    ctx.client.refund_escrow(&ctx.buyer, &id);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Cancelled);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.buyer), 10_000);
}

#[test]
fn test_refund_expired_before_deadline_fails() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    let result = ctx.client.try_refund_escrow(&ctx.buyer, &id);
    assert_eq!(result, Err(Ok(EscrowError::NotExpired)));
}

#[test]
fn test_partial_release() {
    let ctx = setup(1_000);
    let id = create_and_fund(&ctx, 500, 9_000);

    ctx.client.partial_release(&ctx.buyer, &id, &200);

    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(escrow.amount, 300);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 200);
    assert_eq!(
        token_balance(&ctx.env, &ctx.token, &ctx.client.address),
        300
    );

    ctx.client.partial_release(&ctx.buyer, &id, &300);
    let escrow = ctx.client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
    assert_eq!(escrow.amount, 0);
    assert_eq!(token_balance(&ctx.env, &ctx.token, &ctx.seller), 500);
}

#[test]
fn test_get_by_buyer_index() {
    let ctx = setup(1_000);

    let id1 = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &100, &9_000);
    let id2 = ctx
        .client
        .create_escrow(&ctx.buyer, &ctx.seller, &ctx.token, &200, &9_000);

    let other_buyer = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&other_buyer, &1_000);
    let _id3 = ctx.client.create_escrow(
        &other_buyer,
        &ctx.seller,
        &ctx.token,
        &50,
        &9_000,
    );

    let ids = ctx.client.get_by_buyer_index(&ctx.buyer);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);

    let other_ids = ctx.client.get_by_buyer_index(&other_buyer);
    assert_eq!(other_ids.len(), 1);
}

/// #858 — every escrow is indexed by its seller at creation, and the returns
/// paginate over the stored list in insertion order.
#[test]
fn test_get_by_seller_index() {
    let ctx = setup(1_000);

    let seller_a = ctx.seller.clone();
    let id1 = ctx
        .client
        .create_escrow(&ctx.buyer, &seller_a, &ctx.token, &100, &9_000);
    let id2 = ctx
        .client
        .create_escrow(&ctx.buyer, &seller_a, &ctx.token, &200, &9_000);

    // A different seller with a single escrow.
    let other_seller = Address::generate(&ctx.env);
    let id3 = ctx.client.create_escrow(
        &ctx.buyer,
        &other_seller,
        &ctx.token,
        &50,
        &9_000,
    );

    let ids = ctx.client.get_by_seller_index(&seller_a);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);

    let other_ids = ctx.client.get_by_seller_index(&other_seller);
    assert_eq!(other_ids.len(), 1);
    assert_eq!(other_ids.get(0).unwrap(), id3);

    // A seller with no escrows returns an empty list.
    let nobody = Address::generate(&ctx.env);
    assert_eq!(ctx.client.get_by_seller_index(&nobody).len(), 0);
}

/// #858 — each escrow is indexed exactly once per seller, buyer, and token.
#[test]
fn test_each_escrow_indexed_once_per_actor() {
    let ctx = setup(1_000);

    let seller = ctx.seller.clone();
    let buyer = ctx.buyer.clone();
    let token = ctx.token.clone();

    let id = ctx.client.create_escrow(&buyer, &seller, &token, &100, &9_000);

    assert_eq!(ctx.client.get_by_seller_index(&seller).len(), 1);
    assert_eq!(ctx.client.get_by_seller_index(&seller).get(0).unwrap(), id);
    assert_eq!(ctx.client.get_by_buyer_index(&buyer).get(0).unwrap(), id);
}
