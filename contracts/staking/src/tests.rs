//! Unit tests for the standalone staking contract.
//!
//! Covered scenarios
//! -----------------
//! 1. `test_stake_with_valid_tier_and_amount_succeeds`
//!    A staker deposits 5 000 tokens into a tier with a 1 000 minimum;
//!    the resulting StakeInfo must record the correct amount.
//!
//! 2. `test_emergency_unstake_within_lock_period_applies_penalty`
//!    Emergency unstake immediately after staking (lock still active) must
//!    deduct the configured 10 % penalty and return only 90 % to the staker.
//!
//! 3. `test_emergency_penalty_at_zero_bps_is_rejected_at_config_time`
//!    `set_staking_config` must reject a config whose
//!    `emergency_unstake_penalty_bps` is 0 (below the 100 bps minimum).
//!
//! 4. `test_normal_unstake_after_lock_period_has_zero_penalty`
//!    After the lock period expires a normal `unstake_tokens` call must
//!    return the full principal with no deduction.
//!
//! 5. `test_penalty_stays_in_contract_until_admin_withdraws`
//!    After an emergency unstake the penalty amount must remain inside the
//!    contract.  Only when the admin calls `withdraw_penalties` is the
//!    surplus transferred to the designated recipient.
//!
//! 6. `test_penalty_floor_boundary_100_bps_is_accepted`
//!    Exactly 100 bps (the minimum) must be accepted by `set_staking_config`.
//!
//! 7. `test_penalty_99_bps_below_floor_is_rejected`
//!    99 bps (one below the minimum) must be rejected by `set_staking_config`.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::{StakingConfig, StakingContract, StakingContractClient, StakingTier};

// ---------------------------------------------------------------------------
// Shared helper
// ---------------------------------------------------------------------------

/// Register the staking contract, set an admin, mint `mint_amount` tokens to
/// `staker`, configure a 10 % emergency penalty, and create a single "bronze"
/// tier (min 1 000 tokens, 1-day lock).
///
/// Returns `(client, admin, staker, staking_token_address,
///           staking_asset_client)` so each test can operate independently.
fn setup<'a>(
    env: &'a Env,
    mint_amount: i128,
) -> (
    StakingContractClient<'a>,
    Address,
    Address,
    Address,
    soroban_sdk::token::StellarAssetClient<'a>,
) {
    let contract_id = env.register(StakingContract, ());
    let client = StakingContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let staker = Address::generate(env);

    client.set_admin(&admin);

    // Register two separate token contracts (staking token + reward pool).
    let staking_token = env.register_stellar_asset_contract_v2(admin.clone());
    let reward_token = env.register_stellar_asset_contract_v2(admin.clone());

    let sac = soroban_sdk::token::StellarAssetClient::new(env, &staking_token.address());
    sac.mint(&staker, &mint_amount);

    let config = StakingConfig {
        staking_enabled: true,
        emergency_unstake_penalty_bps: 1_000, // 10 %
        staking_token: staking_token.address(),
        reward_pool: reward_token.address(),
    };
    client.set_staking_config(&admin, &config).unwrap();

    let tier = StakingTier {
        id: String::from_str(env, "bronze"),
        name: String::from_str(env, "Bronze"),
        min_stake_amount: 1_000,
        lock_duration: 86_400, // 1 day in seconds
        reward_multiplier_bps: 10_000,
        base_rate_bps: 500,
    };
    client.create_staking_tier(&admin, &tier).unwrap();

    (client, admin, staker, staking_token.address(), sac)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Staking 5 000 tokens into the "bronze" tier (min 1 000) must succeed and
/// the resulting StakeInfo must record the correct staker address and amount.
#[test]
fn test_stake_with_valid_tier_and_amount_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, staker, _token, _sac) = setup(&env, 10_000);

    client
        .stake_tokens(&staker, &String::from_str(&env, "bronze"), &5_000)
        .unwrap();

    let info = client
        .get_stake_info(&staker)
        .expect("StakeInfo must exist after staking");

    assert_eq!(info.staker, staker);
    assert_eq!(info.amount, 5_000);
    assert_eq!(info.tier_id, String::from_str(&env, "bronze"));
    assert!(!info.emergency_unstaked);
}

/// Emergency-unstaking while still in the lock window must apply the 10 %
/// penalty: staker receives 9 000 out of a 10 000 stake.
#[test]
fn test_emergency_unstake_within_lock_period_applies_penalty() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, staker, staking_token, _sac) = setup(&env, 10_000);

    client
        .stake_tokens(&staker, &String::from_str(&env, "bronze"), &10_000)
        .unwrap();

    // Lock period has NOT elapsed — emergency_unstake should still succeed.
    client.emergency_unstake(&staker).unwrap();

    // Stake record must be cleared.
    assert!(
        client.get_stake_info(&staker).is_none(),
        "stake record must be removed after emergency unstake"
    );

    // Verify the staker received exactly (10 000 - 10 %) = 9 000 tokens.
    let token_client = soroban_sdk::token::Client::new(&env, &staking_token);
    let staker_balance = token_client.balance(&staker);
    assert_eq!(
        staker_balance, 9_000,
        "staker must receive principal minus 10 % penalty"
    );
}

/// `set_staking_config` must return an error when
/// `emergency_unstake_penalty_bps` is 0 — below the 100 bps floor.
#[test]
fn test_emergency_penalty_at_zero_bps_is_rejected_at_config_time() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(StakingContract, ());
    let client = StakingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.set_admin(&admin);

    let staking_token = env.register_stellar_asset_contract_v2(admin.clone());
    let reward_token = env.register_stellar_asset_contract_v2(admin.clone());

    let bad_config = StakingConfig {
        staking_enabled: true,
        emergency_unstake_penalty_bps: 0, // invalid — must be rejected
        staking_token: staking_token.address(),
        reward_pool: reward_token.address(),
    };

    let result = client.try_set_staking_config(&admin, &bad_config);
    assert!(
        result.is_err(),
        "zero-bps penalty must be rejected at config time"
    );
}

/// After the lock period has elapsed, `unstake_tokens` must return the full
/// principal with no deduction.
#[test]
fn test_normal_unstake_after_lock_period_has_zero_penalty() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, _admin, staker, staking_token, _sac) = setup(&env, 10_000);

    client
        .stake_tokens(&staker, &String::from_str(&env, "bronze"), &5_000)
        .unwrap();

    // Advance the ledger timestamp past the 1-day lock duration.
    env.ledger().with_mut(|li| {
        li.timestamp += 86_400 + 1;
    });

    client.unstake_tokens(&staker).unwrap();

    // Stake record must be cleared.
    assert!(
        client.get_stake_info(&staker).is_none(),
        "stake record must be removed after normal unstake"
    );

    // The staker deposited 5 000 and started with 10 000; they should have
    // the full 10 000 back (5 000 remaining + 5 000 returned).
    let token_client = soroban_sdk::token::Client::new(&env, &staking_token);
    let staker_balance = token_client.balance(&staker);
    assert_eq!(
        staker_balance, 10_000,
        "full principal must be returned on normal unstake"
    );
}

/// The penalty (10 % of 10 000 = 1 000) must remain inside the contract
/// after an emergency unstake.  Only after the admin calls
/// `withdraw_penalties` should the recipient receive those 1 000 tokens.
#[test]
fn test_penalty_stays_in_contract_until_admin_withdraws() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin, staker, staking_token, _sac) = setup(&env, 10_000);

    client
        .stake_tokens(&staker, &String::from_str(&env, "bronze"), &10_000)
        .unwrap();

    client.emergency_unstake(&staker).unwrap();

    let token_client = soroban_sdk::token::Client::new(&env, &staking_token);

    // After emergency unstake the contract balance must equal the retained
    // penalty (1 000 tokens = 10 % of 10 000).
    let contract_balance = token_client.balance(client.address());
    assert_eq!(
        contract_balance, 1_000,
        "penalty must stay in the contract until admin withdraws"
    );

    // Now the admin withdraws the penalty to a fresh recipient address.
    let recipient = Address::generate(&env);
    client.withdraw_penalties(&admin, &recipient).unwrap();

    // Recipient must have received exactly the penalty amount.
    let recipient_balance = token_client.balance(&recipient);
    assert_eq!(
        recipient_balance, 1_000,
        "recipient must receive the full penalty after admin withdrawal"
    );

    // Contract balance must now be zero.
    let contract_balance_after = token_client.balance(client.address());
    assert_eq!(
        contract_balance_after, 0,
        "contract balance must be zero after penalty withdrawal"
    );
}

/// Exactly 100 bps (the minimum floor) must be accepted by `set_staking_config`.
#[test]
fn test_penalty_floor_boundary_100_bps_is_accepted() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(StakingContract, ());
    let client = StakingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.set_admin(&admin);

    let staking_token = env.register_stellar_asset_contract_v2(admin.clone());
    let reward_token = env.register_stellar_asset_contract_v2(admin.clone());

    let config = StakingConfig {
        staking_enabled: true,
        emergency_unstake_penalty_bps: 100, // exactly the minimum floor — must be accepted
        staking_token: staking_token.address(),
        reward_pool: reward_token.address(),
    };

    let result = client.set_staking_config(&admin, &config);
    assert!(
        result.is_ok(),
        "100 bps is the minimum floor and must be accepted"
    );
}

/// 99 bps (one below the minimum floor) must be rejected by `set_staking_config`.
#[test]
fn test_penalty_99_bps_below_floor_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(StakingContract, ());
    let client = StakingContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.set_admin(&admin);

    let staking_token = env.register_stellar_asset_contract_v2(admin.clone());
    let reward_token = env.register_stellar_asset_contract_v2(admin.clone());

    let config = StakingConfig {
        staking_enabled: true,
        emergency_unstake_penalty_bps: 99, // one below minimum — must be rejected
        staking_token: staking_token.address(),
        reward_pool: reward_token.address(),
    };

    let result = client.try_set_staking_config(&admin, &config);
    assert!(
        result.is_err(),
        "99 bps is below the 100 bps floor and must be rejected"
    );
}