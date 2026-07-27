//! Complete unit-test suite for payout-automation — issue #734.
//!
//! Covers:
//!   - execute(): valid batch, negative/zero rejection, batch-size limit,
//!     atomic rejection of mixed batches, insufficient-treasury guard.
//!   - schedule_payout() / execute_scheduled(): future scheduling, past rejection,
//!     too-early execution guard, correct execution at the right time.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
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

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    let contract = env.register_contract(None, PayoutAutomation);
    PayoutAutomationClient::new(&env, &contract).initialize(&admin, &token);

    Ctx { env, contract, admin, token }
}

/// Mint tokens directly to an address via the Stellar asset admin.
fn mint(ctx: &Ctx, to: &Address, amount: i128) {
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(to, &amount);
}

/// Convenience: fund the contract so it can pay out.
fn fund_contract(ctx: &Ctx, amount: i128) {
    mint(ctx, &ctx.contract, amount);
}

/// Set the ledger timestamp to `ts`.
fn set_time(ctx: &Ctx, ts: u64) {
    ctx.env.ledger().set(LedgerInfo {
        timestamp: ts,
        protocol_version: 22,
        sequence_number: ctx.env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 16,
        min_persistent_entry_ttl: 4096,
        max_entry_ttl: 6_220_800,
    });
}

// ---------------------------------------------------------------------------
// execute() — batch payout tests
// ---------------------------------------------------------------------------

/// A fully valid batch must succeed and each recipient must receive the
/// correct amount.
#[test]
fn test_execute_valid_batch() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 10_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);
    let r3 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 1_000_i128));
    batch.push_back((r2.clone(), 2_000_i128));
    batch.push_back((r3.clone(), 500_i128));

    let result = client.try_execute(&ctx.admin, &batch);
    assert!(result.is_ok(), "valid batch must succeed: {:?}", result);

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&r1), 1_000, "r1 should receive 1_000");
    assert_eq!(tc.balance(&r2), 2_000, "r2 should receive 2_000");
    assert_eq!(tc.balance(&r3), 500, "r3 should receive 500");
}

/// A batch containing a negative amount must be rejected with
/// `NegativeAmount`. No tokens may be transferred (atomicity).
#[test]
fn test_execute_negative_amount_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 5_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 100_i128));
    batch.push_back((r2.clone(), -1_i128)); // invalid

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::NegativeAmount)));

    // r1 must not have received anything — atomic rejection.
    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&r1), 0, "no partial execution allowed");
}

/// A batch containing a zero amount must be rejected.
#[test]
fn test_execute_zero_amount_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 5_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 500_i128));
    batch.push_back((r2.clone(), 0_i128)); // zero — invalid

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::NegativeAmount)));

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&r1), 0, "no partial execution on zero-amount entry");
}

/// A batch with more than 100 entries must fail with `BatchTooLarge`.
#[test]
fn test_execute_exceeds_max_batch_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let mut batch = Vec::new(&ctx.env);
    for _ in 0..101 {
        batch.push_back((Address::generate(&ctx.env), 1_i128));
    }

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::BatchTooLarge)));
}

/// When the contract's token balance is less than the sum of the batch,
/// execution must fail with `InsufficientTreasury`.
#[test]
fn test_execute_insufficient_treasury_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    // Fund only 50 but request 200 total.
    fund_contract(&ctx, 50);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 100_i128));
    batch.push_back((r2.clone(), 100_i128));

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::InsufficientTreasury)));

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&r1), 0, "no transfer when treasury is insufficient");
}

/// A mixed batch (valid entries + one invalid) must be rejected entirely.
/// Valid recipients must not receive anything.
#[test]
fn test_batch_validation_atomic_rejection() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    fund_contract(&ctx, 10_000);

    let r1 = Address::generate(&ctx.env);
    let r2 = Address::generate(&ctx.env);
    let bad = Address::generate(&ctx.env);
    let r4 = Address::generate(&ctx.env);

    let mut batch = Vec::new(&ctx.env);
    batch.push_back((r1.clone(), 300_i128));
    batch.push_back((r2.clone(), 300_i128));
    batch.push_back((bad.clone(), -1_i128)); // invalid — buried in middle
    batch.push_back((r4.clone(), 300_i128));

    let result = client.try_execute(&ctx.admin, &batch);
    assert_eq!(result, Err(Ok(PayoutError::NegativeAmount)));

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&r1), 0, "r1 must not receive on rejected batch");
    assert_eq!(tc.balance(&r2), 0, "r2 must not receive on rejected batch");
    assert_eq!(tc.balance(&r4), 0, "r4 must not receive on rejected batch");
}

// ---------------------------------------------------------------------------
// schedule_payout() / execute_scheduled() — scheduling tests
// ---------------------------------------------------------------------------

/// Scheduling a payout in the future must succeed and return a valid ID.
#[test]
fn test_schedule_payout_in_future() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    set_time(&ctx, 1_000);

    let recipient = Address::generate(&ctx.env);
    let result = client.try_schedule_payout(&ctx.admin, &recipient, &500_i128, &2_000_u64);
    assert!(result.is_ok(), "scheduling in the future must succeed: {:?}", result);

    let id = result.unwrap().unwrap();
    assert_eq!(id, 0_u64, "first scheduled payout should have id 0");
}

/// Scheduling a payout with `execute_after` in the past must be rejected
/// with `ScheduleInPast`.
#[test]
fn test_schedule_in_past_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    set_time(&ctx, 5_000);

    let recipient = Address::generate(&ctx.env);
    // execute_after = 4_999 < current timestamp 5_000 → past
    let result = client.try_schedule_payout(&ctx.admin, &recipient, &100_i128, &4_999_u64);
    assert_eq!(result, Err(Ok(PayoutError::ScheduleInPast)));
}

/// Attempting to execute a scheduled payout before its `execute_after`
/// timestamp must fail with `TooEarly`.
#[test]
fn test_execute_scheduled_too_early_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    set_time(&ctx, 1_000);

    let recipient = Address::generate(&ctx.env);
    let id = client.schedule_payout(&ctx.admin, &recipient, &100_i128, &3_000_u64);

    fund_contract(&ctx, 1_000);

    // Try to execute at t=2_000, which is still before t=3_000.
    set_time(&ctx, 2_000);
    let result = client.try_execute_scheduled(&ctx.admin, &id);
    assert_eq!(result, Err(Ok(PayoutError::TooEarly)));

    // Recipient must not have received anything.
    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&recipient), 0);
}

/// Executing a scheduled payout exactly at `execute_after` must succeed
/// and the recipient must receive the correct amount.
#[test]
fn test_execute_scheduled_at_correct_time() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    set_time(&ctx, 1_000);

    let recipient = Address::generate(&ctx.env);
    let amount: i128 = 750;
    let execute_after: u64 = 3_000;

    let id = client.schedule_payout(&ctx.admin, &recipient, &amount, &execute_after);

    fund_contract(&ctx, amount);

    // Advance time to exactly execute_after.
    set_time(&ctx, execute_after);

    let result = client.try_execute_scheduled(&ctx.admin, &id);
    assert!(result.is_ok(), "execution at correct time must succeed: {:?}", result);

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    assert_eq!(tc.balance(&recipient), amount, "recipient should receive {}", amount);
}
