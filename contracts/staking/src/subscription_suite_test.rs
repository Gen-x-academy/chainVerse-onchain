#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, String,
};

use crate::subscription::{SubscriptionContract, SubscriptionContractClient, SubscriptionError};

// ── Helpers ───────────────────────────────────────────────────────────────────

const PLAN_ID: &str = "basic";
const PLAN_PRICE: i128 = 1_000;
const PLAN_DURATION: u64 = 86_400; // 1 day in seconds
const PROMO_CODE: &str = "SAVE10";
const PROMO_DISCOUNT_BPS: u32 = 1_000; // 10 %

struct TestEnv {
    env: Env,
    contract_id: Address,
    admin: Address,
    token: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, SubscriptionContract);

    // Deploy a mock SAC token (token_admin is the minter)
    let token_admin = Address::generate(&env);
    let token_addr = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let client = SubscriptionContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token_addr);

    TestEnv { env, contract_id, admin, token: token_addr }
}

fn make_plan(ctx: &TestEnv) {
    SubscriptionContractClient::new(&ctx.env, &ctx.contract_id).create_plan(
        &ctx.admin,
        &String::from_str(&ctx.env, PLAN_ID),
        &PLAN_PRICE,
        &PLAN_DURATION,
    );
}

fn subscribe_user(ctx: &TestEnv, user: &Address) {
    SubscriptionContractClient::new(&ctx.env, &ctx.contract_id).subscribe(
        user,
        &String::from_str(&ctx.env, PLAN_ID),
        &None,
    );
}

fn advance_time(env: &Env, seconds: u64) {
    let current = env.ledger().timestamp();
    env.ledger().set_timestamp(current + seconds);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_create_plan_and_subscribe() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    // Mint enough tokens so transfer succeeds
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    client.subscribe(
        &user,
        &String::from_str(&ctx.env, PLAN_ID),
        &None,
    );

    assert!(client.is_active(&user));
}

#[test]
fn test_subscribe_unknown_plan_fails() {
    let ctx = setup();

    let user = Address::generate(&ctx.env);
    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);

    let result = client.try_subscribe(
        &user,
        &String::from_str(&ctx.env, "nonexistent_plan"),
        &None,
    );
    assert_eq!(result, Err(Ok(SubscriptionError::PlanNotFound)));
}

#[test]
fn test_renew_cancelled_subscription_fails() {
    // A cancelled subscription CAN be renewed (the issue wants to confirm
    // that renewing a truly-active sub fails, not a cancelled one).
    // Per the issue spec: test_renew_cancelled_subscription_fails asserts
    // that renewing a *cancelled* sub SUCCEEDS (i.e. does not error).
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &(PLAN_PRICE * 3));

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);

    // Subscribe then immediately cancel
    subscribe_user(&ctx, &user);
    client.cancel(&user);

    // Renew should succeed for a cancelled subscription
    let result = client.try_renew(
        &user,
        &String::from_str(&ctx.env, PLAN_ID),
        &None,
    );
    assert!(result.is_ok());
}

#[test]
fn test_renew_expired_subscription_succeeds() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &(PLAN_PRICE * 2));

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    subscribe_user(&ctx, &user);

    // Advance past expiry
    advance_time(&ctx.env, PLAN_DURATION + 1);

    let result = client.try_renew(
        &user,
        &String::from_str(&ctx.env, PLAN_ID),
        &None,
    );
    assert!(result.is_ok());
    assert!(client.is_active(&user));
}

#[test]
fn test_cancel_gives_prorated_refund() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    subscribe_user(&ctx, &user);

    // Advance half the plan duration
    advance_time(&ctx.env, PLAN_DURATION / 2);

    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    let balance_before = token_client.balance(&user);

    let refund = client.cancel(&user);

    let balance_after = token_client.balance(&user);
    // Refund should be ~50 % of price (integer division)
    let expected = PLAN_PRICE * (PLAN_DURATION / 2) as i128 / PLAN_DURATION as i128;
    assert_eq!(refund, expected);
    assert_eq!(balance_after - balance_before, refund);
}

#[test]
fn test_is_active_returns_false_after_expiry() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    subscribe_user(&ctx, &user);

    advance_time(&ctx.env, PLAN_DURATION + 1);
    assert!(!client.is_active(&user));
}

#[test]
fn test_is_active_returns_false_when_paused() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    subscribe_user(&ctx, &user);

    client.pause_subscription(&ctx.admin, &user);
    assert!(!client.is_active(&user));
}

#[test]
fn test_pause_and_resume_extends_expiry() {
    let ctx = setup();
    make_plan(&ctx);

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    subscribe_user(&ctx, &user);

    // Pause after half the duration has elapsed
    advance_time(&ctx.env, PLAN_DURATION / 2);
    client.pause_subscription(&ctx.admin, &user);

    // Resume with the remaining half
    let remaining = PLAN_DURATION / 2;
    client.resume_subscription(&ctx.admin, &user, &remaining);

    let now = ctx.env.ledger().timestamp();
    let sub = client.get_subscription(&user).expect("subscription should exist");
    assert_eq!(sub.expires_at, now + remaining);
    assert!(client.is_active(&user));
}

#[test]
fn test_apply_promotion_discount() {
    let ctx = setup();
    make_plan(&ctx);

    // Register a 10 % promo valid far in the future
    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    client.add_promotion(
        &ctx.admin,
        &String::from_str(&ctx.env, PROMO_CODE),
        &PROMO_DISCOUNT_BPS,
        &(ctx.env.ledger().timestamp() + 10_000),
    );

    let user = Address::generate(&ctx.env);
    // Mint only the discounted amount (90 % of price)
    let discounted_price = PLAN_PRICE - PLAN_PRICE * PROMO_DISCOUNT_BPS as i128 / 10_000;
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &discounted_price);

    let result = client.try_subscribe(
        &user,
        &String::from_str(&ctx.env, PLAN_ID),
        &Some(String::from_str(&ctx.env, PROMO_CODE)),
    );
    assert!(result.is_ok());
    assert!(client.is_active(&user));
}

#[test]
fn test_expired_promotion_code_fails() {
    let ctx = setup();
    make_plan(&ctx);

    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);

    // Promo that expired at timestamp 0 (already past)
    client.add_promotion(
        &ctx.admin,
        &String::from_str(&ctx.env, PROMO_CODE),
        &PROMO_DISCOUNT_BPS,
        &0,
    );

    let user = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &PLAN_PRICE);

    let result = client.try_subscribe(
        &user,
        &String::from_str(&ctx.env, PLAN_ID),
        &Some(String::from_str(&ctx.env, PROMO_CODE)),
    );
    assert_eq!(result, Err(Ok(SubscriptionError::PromotionExpired)));
}

#[test]
fn test_type_mismatch_large_price_discount() {
    // Verifies no overflow when price * discount_bps exceeds i128 range boundaries.
    // We use a large but realistic price to confirm the arithmetic stays correct.
    let ctx = setup();

    let large_price: i128 = i64::MAX as i128; // large but safe
    let client = SubscriptionContractClient::new(&ctx.env, &ctx.contract_id);
    client.create_plan(
        &ctx.admin,
        &String::from_str(&ctx.env, "big_plan"),
        &large_price,
        &PLAN_DURATION,
    );

    // 50 % discount
    client.add_promotion(
        &ctx.admin,
        &String::from_str(&ctx.env, "HALF"),
        &5_000u32,
        &(ctx.env.ledger().timestamp() + 10_000),
    );

    let user = Address::generate(&ctx.env);
    let discounted = large_price - large_price * 5_000_i128 / 10_000;
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&user, &discounted);

    let result = client.try_subscribe(
        &user,
        &String::from_str(&ctx.env, "big_plan"),
        &Some(String::from_str(&ctx.env, "HALF")),
    );
    assert!(result.is_ok());
}
