#![cfg(test)]
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};
use crate::{DataKey, StakingContract, StakingError, TierConfig};

const GOLD_MIN: i128 = 100;
const GOLD_LOCK: u64 = 3_600;

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(
            &DataKey::Tier(String::from_str(&env, "gold")),
            &TierConfig { min_amount: GOLD_MIN, lock_period: GOLD_LOCK },
        );
    });
    (env, contract_id, admin, token)
}

fn mine(env: &Env, token: &Address, user: &Address, amount: i128) {
    StellarAssetClient::new(env, token).mint(user, &amount);
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

// ISSUE #848: reject zero and negative stake amounts
#[test]
fn test_stake_rejects_zero_amount() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 1_000);
    let result = client.try_stake_tokens(&user, &String::from_str(&env, "gold"), &0_i128);
    assert_eq!(result, Err(Ok(StakingError::InvalidAmount)));
}

#[test]
fn test_stake_rejects_negative_amount() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 1_000);
    let result = client.try_stake_tokens(&user, &String::from_str(&env, "gold"), &-100_i128);
    assert_eq!(result, Err(Ok(StakingError::InvalidAmount)));
}

// ISSUE #847: cap emergency penalty at 100 percent
#[test]
fn test_initialize_accepts_100_percent_penalty() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &10_000u32);
    assert!(result.is_ok());
}

#[test]
fn test_initialize_rejects_penalty_above_100_percent() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let result = client.try_initialize(&admin, &token, &10_001u32);
    assert_eq!(result, Err(Ok(StakingError::PenaltyTooHigh)));
}

#[test]
fn test_emergency_unstake_100_percent_penalty_yields_zero_payout() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &10_000u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 10_000);
    let tier = String::from_str(&env, "gold");
    client.stake_tokens(&user, &tier, &1_000_i128);

    let result = client.try_emergency_unstake(&user);
    assert_eq!(result, Ok(0_i128));
}

// ISSUE #846: decrement TotalStaked on every withdrawal
#[test]
fn test_emergency_unstake_decrements_total_staked() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 10_000);
    let tier = String::from_str(&env, "gold");
    client.stake_tokens(&user, &tier, &1_000_i128);

    let total_before = env.as_contract(&contract_id, || {
        env.storage().instance().get::<DataKey, i128>(&DataKey::TotalStaked).unwrap()
    });
    assert_eq!(total_before, 1_000);

    client.emergency_unstake(&user);

    let total_after = env.as_contract(&contract_id, || {
        env.storage().instance().get::<DataKey, i128>(&DataKey::TotalStaked).unwrap_or(0)
    });
    assert_eq!(total_after, 0);
}

#[test]
fn test_total_staked_never_goes_negative() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 10_000);
    let tier = String::from_str(&env, "gold");
    client.stake_tokens(&user, &tier, &1_000_i128);
    client.emergency_unstake(&user);

    // Second unstake has no stake.
    let result = client.try_emergency_unstake(&user);
    assert_eq!(result, Err(Ok(StakingError::NoStake)));

    let total = env.as_contract(&contract_id, || {
        env.storage().instance().get::<DataKey, i128>(&DataKey::TotalStaked).unwrap_or(0)
    });
    assert_eq!(total, 0);
}

// ISSUE #845: normal unstake after lock period
#[test]
fn test_unstake_before_maturity_fails() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 10_000);
    let tier = String::from_str(&env, "gold");
    client.stake_tokens(&user, &tier, &1_000_i128);

    let result = client.try_unstake(&user);
    assert_eq!(result, Err(Ok(StakingError::StillLocked)));
}

#[test]
fn test_unstake_after_maturity_returns_full_principal() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let user = Address::generate(&env);
    mine(&env, &token, &user, 10_000);
    let tier = String::from_str(&env, "gold");
    client.stake_tokens(&user, &tier, &1_000_i128);

    env.ledger().set_timestamp(env.ledger().timestamp() + GOLD_LOCK + 1);

    let payout = client.unstake(&user);
    assert_eq!(payout, 1_000);

    let total = env.as_contract(&contract_id, || {
        env.storage().instance().get::<DataKey, i128>(&DataKey::TotalStaked).unwrap_or(0)
    });
    assert_eq!(total, 0);
}
