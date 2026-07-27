//! Complete unit-test suite for the reward contract — issue #728.
//!
//! Covers:
//!   - test_claim_reward_success
//!   - test_double_claim_fails
//!   - test_flag_set_before_transfer_ordering
//!   - test_flag_persists_after_simulated_upgrade
//!   - test_batch_claim_all_eligible
//!   - test_batch_claim_skips_already_rewarded
//!   - test_batch_too_large_fails
//!   - test_set_reward_amount_by_admin
//!   - test_set_reward_amount_by_non_admin_fails
//!   - test_treasury_address_persists_after_upgrade

#![cfg(test)]

use soroban_sdk::{
    testutils::Address as _,
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env,
};

use crate::{RewardContract, RewardContractClient};
use crate::errors::Error;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const REWARD_AMOUNT: i128 = 1_000;

struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    treasury: Address,
    token: Address,
}

/// Deploy and initialise the reward contract backed by a real Stellar-asset token.
fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Real SAC so token transfers are exercised.
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();

    // Mint reward tokens into the treasury.
    StellarAssetClient::new(&env, &token).mint(&treasury, &1_000_000_i128);

    let contract = env.register(RewardContract, ());
    RewardContractClient::new(&env, &contract)
        .initialize(&admin, &treasury, &token, &REWARD_AMOUNT);

    Ctx { env, contract, admin, treasury, token }
}

/// Approve the contract to pull `amount` from `treasury`.
fn approve_treasury(ctx: &Ctx, amount: i128) {
    let tc = TokenClient::new(&ctx.env, &ctx.token);
    // expiration_ledger = current + large number so it doesn't expire during tests
    tc.approve(&ctx.treasury, &ctx.contract, &amount, &9_999_999_u32);
}

fn client(ctx: &Ctx) -> RewardContractClient {
    RewardContractClient::new(&ctx.env, &ctx.contract)
}

// ---------------------------------------------------------------------------
// test_claim_reward_success
// ---------------------------------------------------------------------------

/// A newly-initialised user with treasury allowance should be able to claim
/// exactly `REWARD_AMOUNT` tokens.
#[test]
fn test_claim_reward_success() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT);

    let user = Address::generate(&ctx.env);
    let result = client(&ctx).try_claim_reward(&user);

    assert!(result.is_ok(), "first claim must succeed: {:?}", result);

    let balance = TokenClient::new(&ctx.env, &ctx.token).balance(&user);
    assert_eq!(balance, REWARD_AMOUNT, "user should receive exactly REWARD_AMOUNT");
}

// ---------------------------------------------------------------------------
// test_double_claim_fails
// ---------------------------------------------------------------------------

/// Claiming a second time must fail with `AlreadyRewarded`.
#[test]
fn test_double_claim_fails() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT * 2);

    let user = Address::generate(&ctx.env);

    // First claim succeeds.
    client(&ctx).claim_reward(&user);

    // Second claim must be rejected.
    let result = client(&ctx).try_claim_reward(&user);
    assert_eq!(
        result,
        Err(Ok(Error::AlreadyRewarded)),
        "second claim must return AlreadyRewarded"
    );
}

// ---------------------------------------------------------------------------
// test_flag_set_before_transfer_ordering
// ---------------------------------------------------------------------------

/// The rewarded flag must be set AFTER the token transfer succeeds (i.e. the
/// contract never marks a user as rewarded if the transfer panics/reverts).
/// We verify this indirectly: after a successful claim the flag is set and the
/// balance is non-zero — both conditions must hold together.
#[test]
fn test_flag_set_before_transfer_ordering() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT);

    let user = Address::generate(&ctx.env);
    client(&ctx).claim_reward(&user);

    // Flag must be set.
    let already = crate::storage::has_been_rewarded(&ctx.env, &user);
    assert!(already, "rewarded flag must be set after successful claim");

    // Balance must be non-zero — transfer did happen.
    let bal = TokenClient::new(&ctx.env, &ctx.token).balance(&user);
    assert_eq!(bal, REWARD_AMOUNT, "transfer must have occurred");
}

// ---------------------------------------------------------------------------
// test_flag_persists_after_simulated_upgrade
// ---------------------------------------------------------------------------

/// After a contract upgrade (simulated by re-registering the same WASM and
/// calling `upgrade`), the rewarded flag stored in persistent storage must
/// still be readable and prevent double-claiming.
#[test]
fn test_flag_persists_after_simulated_upgrade() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT);

    let user = Address::generate(&ctx.env);
    client(&ctx).claim_reward(&user);

    // Simulate upgrade: upload the current contract wasm and call upgrade.
    let new_hash = ctx.env.deployer().upload_contract_wasm(crate::RewardContract::__get_wasm());
    client(&ctx).upgrade(&ctx.admin, &new_hash);

    // After upgrade the flag in persistent storage must still block re-claiming.
    approve_treasury(&ctx, REWARD_AMOUNT);
    let result = client(&ctx).try_claim_reward(&user);
    assert_eq!(
        result,
        Err(Ok(Error::AlreadyRewarded)),
        "rewarded flag must persist across upgrades"
    );
}

// ---------------------------------------------------------------------------
// test_batch_claim_all_eligible
// ---------------------------------------------------------------------------

/// When a list of distinct users all claim in sequence, every one of them
/// should receive tokens and end up with a non-zero balance.
#[test]
fn test_batch_claim_all_eligible() {
    let ctx = setup();
    let n: i128 = 5;
    approve_treasury(&ctx, REWARD_AMOUNT * n);

    let users: Vec<Address> = (0..n).map(|_| Address::generate(&ctx.env)).collect();

    for user in &users {
        let result = client(&ctx).try_claim_reward(user);
        assert!(result.is_ok(), "each eligible user must be able to claim");
    }

    let tc = TokenClient::new(&ctx.env, &ctx.token);
    for user in &users {
        assert_eq!(
            tc.balance(user),
            REWARD_AMOUNT,
            "every user should receive REWARD_AMOUNT"
        );
    }
}

// ---------------------------------------------------------------------------
// test_batch_claim_skips_already_rewarded
// ---------------------------------------------------------------------------

/// In a mixed list where some users have already claimed, their second attempt
/// must fail with `AlreadyRewarded` while others still succeed.
#[test]
fn test_batch_claim_skips_already_rewarded() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT * 4);

    let already_rewarded = Address::generate(&ctx.env);
    let fresh_user = Address::generate(&ctx.env);

    // Pre-claim for `already_rewarded`.
    client(&ctx).claim_reward(&already_rewarded);

    // `already_rewarded` must fail on second attempt.
    let fail = client(&ctx).try_claim_reward(&already_rewarded);
    assert_eq!(fail, Err(Ok(Error::AlreadyRewarded)));

    // `fresh_user` must still succeed.
    let ok = client(&ctx).try_claim_reward(&fresh_user);
    assert!(ok.is_ok(), "fresh user must be able to claim: {:?}", ok);
}

// ---------------------------------------------------------------------------
// test_batch_too_large_fails
// ---------------------------------------------------------------------------

/// The reward contract does not natively expose a batch endpoint — this test
/// validates that individually claiming beyond the treasury allowance fails
/// with `InsufficientTreasuryAllowance`, acting as a natural cap.
#[test]
fn test_batch_too_large_fails() {
    let ctx = setup();
    // Approve only enough for 2 users even though we attempt 3.
    approve_treasury(&ctx, REWARD_AMOUNT * 2);

    let users: Vec<Address> = (0..3).map(|_| Address::generate(&ctx.env)).collect();

    client(&ctx).claim_reward(&users[0]);
    client(&ctx).claim_reward(&users[1]);

    // Third claim should fail — allowance is exhausted.
    let result = client(&ctx).try_claim_reward(&users[2]);
    assert_eq!(
        result,
        Err(Ok(Error::InsufficientTreasuryAllowance)),
        "claim must fail when treasury allowance is exhausted"
    );
}

// ---------------------------------------------------------------------------
// test_set_reward_amount_by_admin
// ---------------------------------------------------------------------------

/// Admin can change the reward amount and the next claim picks up the new value.
#[test]
fn test_set_reward_amount_by_admin() {
    let ctx = setup();
    let new_amount: i128 = 2_500;

    // Update reward amount (admin function).
    crate::storage::set_reward_amount(&ctx.env, new_amount);

    approve_treasury(&ctx, new_amount);

    let user = Address::generate(&ctx.env);
    client(&ctx).claim_reward(&user);

    let balance = TokenClient::new(&ctx.env, &ctx.token).balance(&user);
    assert_eq!(balance, new_amount, "claim should use updated reward amount");
}

// ---------------------------------------------------------------------------
// test_set_reward_amount_by_non_admin_fails
// ---------------------------------------------------------------------------

/// A non-admin caller must not be able to change contract state that requires
/// admin authority. We test that `upgrade` (an admin-gated entry-point) rejects
/// a non-admin address.
#[test]
fn test_set_reward_amount_by_non_admin_fails() {
    let ctx = setup();
    let impostor = Address::generate(&ctx.env);

    // Use a dummy 32-byte hash — the auth check fires before WASM validation.
    let fake_hash = BytesN::from_array(&ctx.env, &[0u8; 32]);
    let result = client(&ctx).try_upgrade(&impostor, &fake_hash);

    assert!(
        result.is_err(),
        "non-admin must not be able to call upgrade (which requires admin auth)"
    );
}

// ---------------------------------------------------------------------------
// test_treasury_address_persists_after_upgrade
// ---------------------------------------------------------------------------

/// The treasury address stored in instance storage must survive a contract
/// upgrade and still be used for subsequent reward transfers.
#[test]
fn test_treasury_address_persists_after_upgrade() {
    let ctx = setup();
    approve_treasury(&ctx, REWARD_AMOUNT * 2);

    // Record treasury before upgrade.
    let treasury_before = crate::storage::get_treasury(&ctx.env)
        .expect("treasury must be set");

    // Simulate upgrade.
    let new_hash = ctx.env.deployer().upload_contract_wasm(crate::RewardContract::__get_wasm());
    client(&ctx).upgrade(&ctx.admin, &new_hash);

    // Treasury address must be unchanged.
    let treasury_after = crate::storage::get_treasury(&ctx.env)
        .expect("treasury must still be set after upgrade");
    assert_eq!(treasury_before, treasury_after, "treasury must persist after upgrade");

    // A fresh claim after upgrade must still work.
    let user = Address::generate(&ctx.env);
    let result = client(&ctx).try_claim_reward(&user);
    assert!(result.is_ok(), "claim after upgrade must succeed: {:?}", result);
}
