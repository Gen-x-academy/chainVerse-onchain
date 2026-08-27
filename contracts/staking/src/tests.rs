#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, Env, String, token};
use crate::{StakingContract, StakingError, TierConfig};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    (env, contract_id, admin, token)
}

// Set up an initialized contract backed by a real (mintable) token and an
// active tier, returning the token client plus the initialized on-chain state.
fn setup_with_token_tier() -> (Env, token::Client, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(admin.clone()).address();
    let token = token::Client::new(&env, &token_id);
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token_id, &500u32);
    let tier = String::from_str(&env, "gold");
    let config = TierConfig { min_amount: 0, lock_period: 100 };
    client.add_tier(&admin, &tier, &config);
    token.mint(&user, &10_000_i128);
    (env, token, contract_id, admin, user)
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

// ===== ISSUE #843: tier management =====

#[test]
fn test_non_admin_cannot_add_tier() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let attacker = Address::generate(&env);
    let tier = String::from_str(&env, "gold");
    let config = TierConfig { min_amount: 0, lock_period: 100 };
    let result = client.try_add_tier(&attacker, &tier, &config);
    assert_eq!(result, Err(Ok(StakingError::Unauthorized)));
    assert!(!client.is_tier_active(&tier));
}

#[test]
fn test_stake_against_inactive_tier_fails() {
    let (env, token, contract_id, admin, user) = setup_with_token_tier();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let tier = String::from_str(&env, "gold");
    let balance_before = token.balance(&user);
    client.stake_tokens(&user, &tier, &1000_i128);
    assert_eq!(token.balance(&user), balance_before - 1000_i128);
    assert!(client.is_tier_active(&tier));
    // Admin deactivates the tier -> new stakes must fail.
    assert!(client.try_deactivate_tier(&admin, &tier).is_ok());
    assert!(!client.is_tier_active(&tier));
    let balance_mid = token.balance(&user);
    let result = client.try_stake_tokens(&user, &tier, &500_i128);
    assert_eq!(result, Err(Ok(StakingError::TierInactive)));
    // No funds moved and existing records intact.
    assert_eq!(token.balance(&user), balance_mid);
}

#[test]
fn test_update_tier_changes_config() {
    let (env, contract_id, admin, token) = setup();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin, &token, &500u32);
    let tier = String::from_str(&env, "gold");
    let config = TierConfig { min_amount: 0, lock_period: 100 };
    client.add_tier(&admin, &tier, &config);
    let new_config = TierConfig { min_amount: 50, lock_period: 200 };
    client.update_tier(&admin, &tier, &new_config);
    assert!(client.is_tier_active(&tier));
    // Updating a non-existent tier fails.
    let missing = String::from_str(&env, "silver");
    let result = client.try_update_tier(&admin, &missing, &new_config);
    assert_eq!(result, Err(Ok(StakingError::TierNotFound)));
}

// ===== ISSUE #844: repeated stake correctness =====

#[test]
fn test_repeated_stake_merges_into_single_record() {
    let (env, token, contract_id, admin, user) = setup_with_token_tier();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let tier = String::from_str(&env, "gold");
    let balance_before = token.balance(&user);
    client.stake_tokens(&user, &tier, &1000_i128);
    client.stake_tokens(&user, &tier, &500_i128);
    client.stake_tokens(&user, &tier, &250_i128);
    // All three deposits are transferred to the contract (no funds orphaned).
    assert_eq!(token.balance(&user), balance_before - 1750_i128);
    assert_eq!(token.balance(&contract_id), 1750_i128);
    // The single record keeps the merged principal and the original tier.
    let record = env.as_contract(&contract_id, || {
        let dk = crate::DataKey::Stake(user.clone());
        env.storage().persistent().get::<_, crate::StakeRecord>(&dk).unwrap()
    });
    assert_eq!(record.amount, 1750_i128);
    assert_eq!(record.tier, tier);
    // TotalStaked matches the merged principal.
    let total = env.as_contract(&contract_id, || {
        env.storage().instance().get::<_, i128>(&crate::DataKey::TotalStaked).unwrap()
    });
    assert_eq!(total, 1750_i128);
    // Emergency unstake repays the full merged principal (minus penalty).
    let payout = client.emergency_unstake(&user);
    assert_eq!(payout, 1750_i128 - (1750_i128 * 500 / 10_000));
    assert_eq!(token.balance(&user), balance_before - 1750_i128 + payout);
}

#[test]
fn test_repeated_stake_cannot_change_tier() {
    let (env, token, contract_id, admin, user) = setup_with_token_tier();
    let client = crate::StakingContractClient::new(&env, &contract_id);
    let gold = String::from_str(&env, "gold");
    let silver = String::from_str(&env, "silver");
    let config = TierConfig { min_amount: 0, lock_period: 50 };
    client.add_tier(&admin, &silver, &config);
    client.stake_tokens(&user, &gold, &1000_i128);
    let balance_before = token.balance(&user);
    // Second stake with a different tier is rejected (keeps first deposit intact).
    let result = client.try_stake_tokens(&user, &silver, &500_i128);
    assert_eq!(result, Err(Ok(StakingError::TierChangeNotAllowed)));
    assert_eq!(token.balance(&user), balance_before);
}
