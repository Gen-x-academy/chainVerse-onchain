//! Integration tests for the `pay_for_course` payment flow.
//!
//! Issue #651 — covers four critical paths:
//!   1. Paying for a non-existent course returns `PayoutError::CourseNotFound`.
//!   2. Paying twice for the same course returns `PayoutError::AlreadyEnrolled`.
//!   3. Platform fee in basis-points is calculated and forwarded correctly.
//!   4. The student's token balance is reduced by the exact course price.

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

use crate::{PayoutAutomation, PayoutAutomationClient, PayoutError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Shared test context returned by `setup()`.
struct TestCtx {
    env: Env,
    contract: Address,
    admin: Address,
    token: Address,
}

/// Deploy the payout-automation contract and a real Stellar-asset token.
fn setup() -> TestCtx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // Register a real Stellar asset contract so `token::Client::transfer` works.
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    let contract = env.register_contract(None, PayoutAutomation);
    let client = PayoutAutomationClient::new(&env, &contract);

    // Initialize: stores admin, token and treasury (defaults to admin).
    client.initialize(&admin, &token);

    TestCtx { env, contract, admin, token }
}

/// Mint `amount` tokens to `to` via the Stellar asset admin.
fn mint(env: &Env, token: &Address, to: &Address, amount: i128) {
    let sac = StellarAssetClient::new(env, token);
    sac.mint(to, &amount);
}

// ---------------------------------------------------------------------------
// Test 1: phantom course guard — PayoutError::CourseNotFound
// ---------------------------------------------------------------------------

/// Paying for a course ID that was never registered must return `CourseNotFound`.
/// Prevents fund loss when a student supplies an invalid or removed course ID.
#[test]
fn test_pay_phantom_course_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let student = Address::generate(&ctx.env);
    mint(&ctx.env, &ctx.token, &student, 10_000);

    // Course 999 was never registered.
    let result = client.try_pay_for_course(&student, &999_u64);

    assert_eq!(
        result,
        Err(Ok(PayoutError::CourseNotFound)),
        "expected CourseNotFound for unregistered course"
    );
}

// ---------------------------------------------------------------------------
// Test 2: double-payment prevention — PayoutError::AlreadyEnrolled
// ---------------------------------------------------------------------------

/// Enrolling in the same course twice must return `AlreadyEnrolled`.
/// Without this guard a student could be charged multiple times for the same
/// course with no recourse.
#[test]
fn test_double_pay_fails() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let course_id: u64 = 1;
    let price: i128 = 500;

    // Register course with zero platform fee.
    client.register_course(&ctx.admin, &course_id, &price, &0_u32);

    let student = Address::generate(&ctx.env);
    // Give enough balance for two hypothetical payments.
    mint(&ctx.env, &ctx.token, &student, price * 2);

    // First enrollment must succeed.
    client.pay_for_course(&student, &course_id);

    // Second attempt must be rejected.
    let result = client.try_pay_for_course(&student, &course_id);

    assert_eq!(
        result,
        Err(Ok(PayoutError::AlreadyEnrolled)),
        "expected AlreadyEnrolled on second enrollment attempt"
    );
}

// ---------------------------------------------------------------------------
// Test 3: platform fee calculation — correct bps math
// ---------------------------------------------------------------------------

/// When a course is registered with `fee_bps`, the exact fee amount must be
/// forwarded to the treasury address after enrollment.
///
/// Scenario: price = 10_000, fee_bps = 250 (2.5 %) → treasury receives 250.
#[test]
fn test_fee_calculation_correct() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let course_id: u64 = 2;
    let price: i128 = 10_000;
    let fee_bps: u32 = 250; // 2.5 %
    let expected_fee: i128 = price * (fee_bps as i128) / 10_000; // = 250

    // Point treasury at a dedicated address so its balance starts at 0.
    let treasury = Address::generate(&ctx.env);
    client.set_treasury(&ctx.admin, &treasury);

    // Register course with fee.
    client.register_course(&ctx.admin, &course_id, &price, &fee_bps);

    let student = Address::generate(&ctx.env);
    mint(&ctx.env, &ctx.token, &student, price);

    // Enroll.
    client.pay_for_course(&student, &course_id);

    // Treasury must hold exactly the expected platform fee.
    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    let treasury_balance = token_client.balance(&treasury);

    assert_eq!(
        treasury_balance,
        expected_fee,
        "treasury should hold {} ({}bps of price {}), got {}",
        expected_fee,
        fee_bps,
        price,
        treasury_balance
    );
}

// ---------------------------------------------------------------------------
// Test 4: exact balance deduction
// ---------------------------------------------------------------------------

/// The student's token balance must decrease by exactly `price` after a
/// successful enrollment. No more, no less.
#[test]
fn test_pay_deducts_exact_amount() {
    let ctx = setup();
    let client = PayoutAutomationClient::new(&ctx.env, &ctx.contract);

    let course_id: u64 = 3;
    let price: i128 = 1_500;
    let initial_balance: i128 = 5_000;

    // Zero-fee course so fee forwarding doesn't affect the deduction assertion.
    client.register_course(&ctx.admin, &course_id, &price, &0_u32);

    let student = Address::generate(&ctx.env);
    mint(&ctx.env, &ctx.token, &student, initial_balance);

    let token_client = TokenClient::new(&ctx.env, &ctx.token);

    // Confirm starting balance.
    assert_eq!(token_client.balance(&student), initial_balance);

    // Enroll.
    client.pay_for_course(&student, &course_id);

    // Balance must be reduced by exactly the course price.
    let final_balance = token_client.balance(&student);
    assert_eq!(
        final_balance,
        initial_balance - price,
        "student balance should decrease by exactly the course price ({})",
        price
    );
}
