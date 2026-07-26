//! Unit tests for `execute()` — payout-automation contract.
//!
//! Covers the acceptance criteria from issue #729 / #301:
//!   1. A batch containing a negative amount is rejected entirely (no partial execution).
//!   2. A batch containing a zero amount is rejected entirely.
//!   3. A mixed batch (valid entries + one invalid) is rejected entirely — the valid
//!      recipients must NOT receive any tokens.
//!   4. A fully valid batch succeeds.
//!   5. Batch-too-large guard is enforced.
//!   6. Re-initialization is rejected.

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

use crate::{PayoutAutomation, PayoutAutomationClient, PayoutError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    token: Address,
}

/// Deploy the contract and a real Stellar-asset token, then initialize.
fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // Use a real Stellar-asset contract so `TokenClient::transfer` works.
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    let contract = env.register_contract(None, PayoutAutomation);
    PayoutAutomationClient::new(&env, &contract).initialize(&admin, &token);

    Ctx { env, contract, admin, token }
}

/// Mint `amount` tokens directly to `to` via the Stellar asset admin.
fn mint(ctx: &Ctx, to: &Address, amount: i128) {
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(to, &amount);
}

/// Convenience: fund the contract itself so it can pay out.
fn fund_contract(ctx: &Ctx, amount: i128) {
    mint(ctx, &ctx.contract, amount);
}

// ---------------------------------------------------------------------------
// Test 1 — negative amount rejects entire batch (acceptance criterion 1)
// ---------------------------------------------------------------------------

/// A batch that contains a negative amount must be rejected with
/// `PayoutError::NegativeAmount`. No tokens may be transferred.
///
/// Regression: the old single-pass implementation would transfer to all
/// recipients that come *before* the invalid entry. The two-pass fix must
/// prevent any transfer.
#[test]
fn test_execute_negative_amount_rejects_batch() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 1_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 100_i128));
    batch.push_back((r2.clone(), -50_i128)); // invalid

    let result = client.try_execute(&ctx.admin, &batch);

    assert_eq!(
        result,
        Err(Ok(PayoutError::NegativeAmount)),
        "batch with negative amount must be rejected"
    );

    // Crucially: r1 must NOT have received any tokens (no partial execution).
    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(
        token_client.balance(&r1),
        0,
        "first recipient must not receive tokens when batch is rejected"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — zero amount rejects entire batch (acceptance criterion 2)
// ---------------------------------------------------------------------------

/// A batch containing a zero amount must be rejected with
/// `PayoutError::NegativeAmount`. Zero is not a valid payout value.
#[test]
fn test_execute_zero_amount_rejects_batch() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 1_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 200_i128));
    batch.push_back((r2.clone(), 0_i128)); // zero — invalid

    let result = client.try_execute(&ctx.admin, &batch);

    assert_eq!(
        result,
        Err(Ok(PayoutError::NegativeAmount)),
        "batch with zero amount must be rejected"
    );

    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(
        token_client.balance(&r1),
        0,
        "first recipient must not receive tokens when batch is rejected due to zero entry"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — mixed batch (valid + invalid) rejected entirely (acceptance criterion 3)
// ---------------------------------------------------------------------------

/// When a batch contains valid entries followed by an invalid one the entire
/// batch must be rejected — the valid recipients must receive nothing.
///
/// This is the key regression test: the old code transferred to r1 before it
/// hit the negative-amount check for r2.
#[test]
fn test_execute_mixed_batch_all_or_nothing() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 10_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);
    let r3 = Address::generate(&ctx.env); // invalid entry will be here
    let r4 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 100_i128));
    batch.push_back((r2.clone(), 200_i128));
    batch.push_back((r3.clone(), -1_i128)); // invalid — buried in the middle
    batch.push_back((r4.clone(), 300_i128));

    let result = client.try_execute(&ctx.admin, &batch);

    assert_eq!(
        result,
        Err(Ok(PayoutError::NegativeAmount)),
        "mixed batch must be fully rejected"
    );

    let token_client = TokenClient::new(&ctx.env, &ctx.token);

    // None of the valid-looking recipients should have received anything.
    assert_eq!(token_client.balance(&r1), 0, "r1 must receive nothing");
    assert_eq!(token_client.balance(&r2), 0, "r2 must receive nothing");
    assert_eq!(token_client.balance(&r4), 0, "r4 must receive nothing");
}

// ---------------------------------------------------------------------------
// Test 4 — valid batch succeeds
// ---------------------------------------------------------------------------

/// A batch where every amount is positive must succeed and all recipients
/// must receive the correct amounts.
#[test]
fn test_execute_valid_batch_succeeds() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 10_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 400_i128));
    batch.push_back((r2.clone(), 600_i128));

    let result = client.try_execute(&ctx.admin, &batch);
    assert!(result.is_ok(), "valid batch must succeed: {:?}", result);

    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(token_client.balance(&r1), 400, "r1 should receive 400");
    assert_eq!(token_client.balance(&r2), 600, "r2 should receive 600");
}

// ---------------------------------------------------------------------------
// Test 5 — batch too large is rejected
// ---------------------------------------------------------------------------

/// A batch with more than MAX_BATCH_SIZE (100) entries must fail with
/// `PayoutError::BatchTooLarge`.
#[test]
fn test_execute_batch_too_large() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let mut batch = Vec::new(&ctx.env);
    for _ in 0..101 {
        batch.push_back((Address::generate(&ctx.env), 1_i128));
    }

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(
        result,
        Err(Ok(PayoutError::BatchTooLarge)),
        "batch of 101 must be rejected"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — re-initialization rejected
// ---------------------------------------------------------------------------

/// Calling `initialize` a second time must fail with
/// `PayoutError::AlreadyInitialized`.
#[test]
fn test_reinitialize_rejected() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let result = client.try_initialize(&ctx.admin, &ctx.token);
    assert!(
        result.is_err(),
        "second initialize call must be rejected"
    );
}
