#![cfg(test)]
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env, String};

use crate::{DataKey, StakingConfig, StakingContract, StakingError, TierConfig};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    (env, contract_id, admin, token)
}

/// Setup with a real mock SAC token so positive transfer paths work.
struct TestEnv {
    env: Env,
    contract_id: Address,
    admin: Address,
    token: Address,
}

fn setup_with_token() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone()).address();

    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);

    TestEnv { env, contract_id, admin, token }
}

fn make_tier(ctx: &TestEnv, name: &str, min_amount: i128, lock_period: u64) {
    crate::StakingContractClient::new(&ctx.env, &ctx.contract_id).add_tier(
        &ctx.admin,
        &String::from_str(&ctx.env, name),
        &TierConfig { min_amount, lock_period },
    );
}

fn mint(ctx: &TestEnv, user: &Address, amount: i128) {
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(user, &amount);
}

#[test]
fn test_initialize_rejects_zero_penalty() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &0u32);
    assert_eq!(result, Err(Ok(StakingError::PenaltyTooLow)));
}

#[test]
fn test_initialize_accepts_valid_penalty() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &500u32);
    assert!(result.is_ok());
}

#[test]
fn test_reinitialize_rejected() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let result = client.try_initialize(&admin, &token, &500u32);
    assert_eq!(result, Err(Ok(StakingError::AlreadyInitialized)));
}

#[test]
fn test_stake_requires_valid_tier() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    let tier = String::from_str(&env, "gold");
    let result = client.try_stake_tokens(&user, &tier, &1000_i128);
    assert_eq!(result, Err(Ok(StakingError::TierNotFound)));
}

#[test]
fn test_add_tier_then_stake_and_query() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);

    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let tier = client.get_tier(&String::from_str(&ctx.env, "gold")).expect("tier exists");
    assert_eq!(tier.min_amount, 100);
    assert_eq!(tier.lock_period, 3600);

    let active = client.get_active_tiers();
    assert_eq!(active.len(), 1);
    assert_eq!(active.get(0).unwrap(), String::from_str(&ctx.env, "gold"));

    let user = Address::generate(&ctx.env);
    mint(&ctx, &user, 1000);
    let before = client.get_total_staked();
    client.stake_tokens(
        &user,
        &String::from_str(&ctx.env, "gold"),
        &500_i128,
    );

    assert_eq!(client.get_total_staked(), before + 500);
    let stake = client.get_stake(&user).expect("stake exists");
    assert_eq!(stake.amount, 500);
    assert_eq!(stake.tier, String::from_str(&ctx.env, "gold"));
    assert_eq!(stake.staked_at, ctx.env.ledger().timestamp());

    let unlock = client.get_unlock_timestamp(&user).expect("unlock timestamp");
    assert_eq!(unlock, stake.staked_at + 3600);
}

#[test]
fn test_add_existing_tier_rejected() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let res = client.try_add_tier(
        &ctx.admin,
        &String::from_str(&ctx.env, "gold"),
        &TierConfig { min_amount: 200, lock_period: 7200 },
    );
    assert_eq!(res, Err(Ok(StakingError::TierExists)));
}

#[test]
fn test_update_tier_and_queries() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    client.update_tier(
        &ctx.admin,
        &String::from_str(&ctx.env, "gold"),
        &TierConfig { min_amount: 250, lock_period: 7200 },
    );
    let tier = client.get_tier(&String::from_str(&ctx.env, "gold")).expect("tier exists");
    assert_eq!(tier.min_amount, 250);
    assert_eq!(tier.lock_period, 7200);

    let res = client.try_update_tier(
        &ctx.admin,
        &String::from_str(&ctx.env, "missing"),
        &TierConfig { min_amount: 1, lock_period: 1 },
    );
    assert_eq!(res, Err(Ok(StakingError::TierNotFound)));
}

#[test]
fn test_non_admin_tier_ops_rejected() {
    let ctx = setup_with_token();
    let stranger = Address::generate(&ctx.env);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let res = client.try_add_tier(
        &stranger,
        &String::from_str(&ctx.env, "gold"),
        &TierConfig { min_amount: 100, lock_period: 3600 },
    );
    assert_eq!(res, Err(Ok(StakingError::Unauthorized)));
}

#[test]
fn test_stake_below_minimum_rejected() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);
    let user = Address::generate(&ctx.env);
    mint(&ctx, &user, 100);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let res = client.try_stake_tokens(
        &user,
        &String::from_str(&ctx.env, "gold"),
        &99_i128,
    );
    assert_eq!(res, Err(Ok(StakingError::InsufficientBalance)));
}

#[test]
fn test_emergency_unstake_pools_penalty_and_pays_out() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);

    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let user = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);

    mint(&ctx, &user, 1_000);
    client.stake_tokens(&user, &String::from_str(&ctx.env, "gold"), &1_000_i128);

    let token_client = TokenClient::new(&ctx.env, &ctx.token);
    let user_balance_before = token_client.balance(&user);
    let contract_balance_before = token_client.balance(&ctx.contract_id);

    let pool_before = 0i128;
    let penalty = 1_000 * 500 / 10_000; // 50

    let payout = client.emergency_unstake(&user);
    assert_eq!(payout, 950);
    assert_eq!(payout + penalty, 1_000);

    assert!(client.get_stake(&user).is_none());
    assert_eq!(client.get_total_staked(), 0);
    assert_eq!(token_client.balance(&user), user_balance_before + payout);
    assert_eq!(
        token_client.balance(&ctx.contract_id),
        contract_balance_before - payout - pool_before,
    );

    // Penalty must be withdrawn from the pool by admin.
    let pool_before_withdraw = penalty;
    let b = token_client.balance(&recipient);
    client.withdraw_penalties(&ctx.admin, &recipient, &pool_before_withdraw);
    assert_eq!(token_client.balance(&recipient), b + pool_before_withdraw);
}

#[test]
fn test_withdraw_penalties_rejects_overdraft_and_negative() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);

    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let user = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);

    mint(&ctx, &user, 1_000);
    client.stake_tokens(&user, &String::from_str(&ctx.env, "gold"), &1_000_i128);
    client.emergency_unstake(&user);

    let res = client.try_withdraw_penalties(&ctx.admin, &recipient, &(50_i128 + 1));
    assert_eq!(res, Err(Ok(StakingError::PenaltyInsufficient)));

    let res = client.try_withdraw_penalties(&ctx.admin, &recipient, &(-1_i128));
    assert_eq!(res, Err(Ok(StakingError::PenaltyInsufficient)));
}

#[test]
fn test_withdraw_penalties_non_admin_rejected() {
    let ctx = setup_with_token();
    let stranger = Address::generate(&ctx.env);
    let recipient = Address::generate(&ctx.env);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let res = client.try_withdraw_penalties(&stranger, &recipient, &10_i128);
    assert_eq!(res, Err(Ok(StakingError::Unauthorized)));
}

#[test]
fn test_withdraw_zero_amount_is_noop() {
    let ctx = setup_with_token();
    let recipient = Address::generate(&ctx.env);
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let res = client.try_withdraw_penalties(&ctx.admin, &recipient, &0_i128);
    assert!(res.is_ok());
}

#[test]
fn test_persistent_ttl_is_extended_on_stake_and_read() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);

    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let user = Address::generate(&ctx.env);
    mint(&ctx, &user, 1_000);
    client.stake_tokens(&user, &String::from_str(&ctx.env, "gold"), &500_i128);

    let stake_key = DataKey::Stake(user.clone());
    let tier_key = DataKey::Tier(String::from_str(&ctx.env, "gold"));
    let ttl_after_write = ctx.env.storage().persistent().get_ttl(&stake_key);
    assert!(ttl_after_write > 0);

    // A subsequent read re-extends the TTL (bounded at MAX_TTL).
    client.get_stake(&user).expect("stake exists");
    let ttl_after_read = ctx.env.storage().persistent().get_ttl(&stake_key);
    assert!(ttl_after_read >= ttl_after_write);
    assert!(ttl_after_read <= 6_220_800);
    assert!(ctx.env.storage().persistent().get_ttl(&tier_key) > 0);

    // Unlock timestamp read re-extends tier TTL too.
    client.get_unlock_timestamp(&user).expect("unlock timestamp");
    assert!(ctx.env.storage().persistent().get_ttl(&tier_key) > 0);
}

#[test]
fn test_query_methods_return_none_for_unknown() {
    let ctx = setup_with_token();
    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let user = Address::generate(&ctx.env);
    assert!(client.get_stake(&user).is_none());
    assert!(client.get_unlock_timestamp(&user).is_none());
    assert!(client.get_tier(&String::from_str(&ctx.env, "gold")).is_none());
    assert_eq!(client.get_active_tiers().len(), 0);
    assert_eq!(client.get_total_staked(), 0);

    let config: StakingConfig = client.get_configuration();
    assert_eq!(config.admin, ctx.admin);
    assert_eq!(config.token, ctx.token);
}

#[test]
fn test_stake_and_emergency_unstake_emit_events() {
    let ctx = setup_with_token();
    make_tier(&ctx, "gold", 100, 3600);

    let client = crate::StakingContractClient::new(&ctx.env, &ctx.contract_id);
    let user = Address::generate(&ctx.env);
    mint(&ctx, &user, 1_000);
    client.stake_tokens(&user, &String::from_str(&ctx.env, "gold"), &1_000_i128);
    client.emergency_unstake(&user);

    let events = ctx.env.events().all();
    assert!(events.len() >= 3);
}
