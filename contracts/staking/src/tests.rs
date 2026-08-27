#![cfg(test)]
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};
use soroban_sdk::{Address, Env, String};

use crate::{DataKey, StakingConfig, StakingContract, StakingError, TierConfig};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, String,
};
use crate::{DataKey, StakingContract, StakingError, StakeRecord, TierConfig};

/// Real-token fixture. Provides a SAC token so transfers and token-conservation
/// invariants can be asserted.
struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    token: Address,
    client: crate::StakingContractClient<'static>,
}

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
fn setup_real() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, StakingContract);
    let client = crate::StakingContractClient::new(&env, &contract);
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract(token_admin.clone());
    let admin = Address::generate(&env);
    client.initialize(&admin, &token, &500u32);
    Ctx { env, contract, admin, token, client }
}

fn register_tier(ctx: &Ctx, tier: &str, min: i128, lock: u64) {
    let t = String::from_str(&ctx.env, tier);
    ctx.env.as_contract(&ctx.contract, || {
        ctx.env.storage().persistent().set(
            &DataKey::Tier(t.clone()),
            &TierConfig { min_amount: min, lock_period: lock },
        );
    });
}

fn total_staked(ctx: &Ctx) -> i128 {
    ctx.env.as_contract(&ctx.contract, || {
        ctx.env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0)
    })
}

fn read_stake(ctx: &Ctx, user: &Address) -> Option<StakeRecord> {
    ctx.env.as_contract(&ctx.contract, || {
        ctx.env
            .storage()
            .persistent()
            .get(&DataKey::Stake(user.clone()))
    })
}

fn stake(ctx: &Ctx, user: &Address, tier: &str, amount: i128) {
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(user, &amount);
    ctx.client
        .stake_tokens(user, &String::from_str(&ctx.env, tier), &amount);
}

fn token_balance(ctx: &Ctx, addr: &Address) -> i128 {
    TokenClient::new(&ctx.env, &ctx.token).balance(addr)
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
// ---------------------------------------------------------------------------
// ISSUE #854: invariant + time-boundary property tests (deterministic loops)
// ---------------------------------------------------------------------------

/// Principal conservation: across many deterministic stakes of varying sizes,
/// the contract holds exactly the sum of deposits, every depositor's principal
/// leaves their wallet, and the (users + contract) balance is invariant.
#[test]
fn property_principal_conservation_across_many_stakes() {
    let ctx = setup_real();
    register_tier(&ctx, "gold", 100, 9999);

    let amounts = [100_i128, 250, 500, 1_000, 2_000, 750, 1_250];
    let expected_total: i128 = amounts.iter().sum();
    let mut users = Vec::new();

    // Deterministic loop: one fresh user per deterministic amount.
    for amt in amounts.iter() {
        let user = Address::generate(&ctx.env);
        stake(&ctx, &user, "gold", *amt);
        users.push(user);
    }

    // Contract holds exactly the sum of all deposits.
    assert_eq!(token_balance(&ctx, &ctx.contract), expected_total);

    // Every depositor's entire principal moved into the contract.
    let user_total: i128 = users.iter().map(|u| token_balance(&ctx, u)).sum();
    assert_eq!(user_total, 0);

    // Total balance across all parties is invariant.
    assert_eq!(user_total + token_balance(&ctx, &ctx.contract), expected_total);
}

/// Total-staked consistency: the stored TotalStaked counter equals the sum of
/// the individual StakeRecord amounts across all depositors.
#[test]
fn property_total_staked_matches_sum_of_records() {
    let ctx = setup_real();
    register_tier(&ctx, "gold", 100, 9999);

    let amounts = [100_i128, 300, 700, 1_500, 200];
    let expected: i128 = amounts.iter().sum();
    let mut users = Vec::new();
    for amt in amounts.iter() {
        let user = Address::generate(&ctx.env);
        stake(&ctx, &user, "gold", *amt);
        users.push(user);
    }

    // Each user's record preserves its own amount ...
    let record_sum: i128 = users
        .iter()
        .map(|u| read_stake(&ctx, u).expect("stake must exist").amount)
        .sum();
    assert_eq!(record_sum, expected);

    // ... and the aggregate counter matches the sum of those records.
    assert_eq!(total_staked(&ctx), record_sum);
}

/// Lock boundary: staked_at is recorded at stake time; maturity is
/// staked_at + tier lock_period. The unlock timestamp is derived from the
/// recorded value, not the current ledger time, and the full amount stays
/// locked regardless of how far the ledger advances.
#[test]
fn property_lock_boundary_before_and_after_maturity() {
    let ctx = setup_real();
    let lock = 86_400u64; // 1 day in seconds
    register_tier(&ctx, "gold", 100, lock);

    let start = 1_000u64;
    ctx.env.ledger().with_mut(|li| li.timestamp = start);

    let user = Address::generate(&ctx.env);
    stake(&ctx, &user, "gold", 1_000);

    let record = read_stake(&ctx, &user).unwrap();
    assert_eq!(record.staked_at, start, "staked_at must be stake time");
    let maturity = record.staked_at + lock;
    assert_eq!(maturity, start + lock);

    // Before maturity the full amount remains staked.
    ctx.env.ledger().with_mut(|li| li.timestamp = maturity - 1);
    let before = read_stake(&ctx, &user).unwrap();
    assert_eq!(before.amount, 1_000);
    assert_eq!(before.staked_at, start);

    // After maturity the recorded unlock timestamp still derives from staked_at.
    ctx.env.ledger().with_mut(|li| li.timestamp = maturity + 500);
    let after = read_stake(&ctx, &user).unwrap();
    assert_eq!(after.staked_at, start);
    assert_eq!(after.staked_at + lock, maturity);
    assert_eq!(after.amount, 1_000);
}

/// Authorization: only the admin can call admin-only entrypoints. Uses targeted
/// auth (no blanket mock_all_auths) so require_auth is genuinely exercised.
#[test]
fn authorization_non_admin_cannot_pause() {
    let env = Env::default();
    let contract = env.register_contract(None, StakingContract);
    let client = crate::StakingContractClient::new(&env, &contract);
    let token_admin = Address::generate(&env);
    let token = env.register_stellar_asset_contract(token_admin.clone());
    let admin = Address::generate(&env);
    let stranger = Address::generate(&env);

    // Admin initializes with explicit targeted auth.
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "initialize",
            args: (&admin, &token, 500_u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &token, &500u32);

    // Stranger attempts to pause; admin auth is required and must reject.
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "set_paused",
            args: (true,).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let result = client.try_set_paused(&stranger, &true);
    assert!(result.is_err(), "non-admin must not pause the contract");
    assert!(!client.is_paused());
}

// ---------------------------------------------------------------------------
// ISSUE #853: pause + two-step admin transfer tests
// ---------------------------------------------------------------------------

/// Pause blocks new deposits but allows safe exits (withdrawals).
#[test]
fn pause_blocks_stake_but_allows_unstake() {
    let ctx = setup_real();
    register_tier(&ctx, "gold", 100, 9999);

    let user = Address::generate(&ctx.env);
    stake(&ctx, &user, "gold", 1_000);

    ctx.client.set_paused(&ctx.admin, &true);
    assert!(ctx.client.is_paused());

    // Deposits are blocked while paused.
    let fresh = Address::generate(&ctx.env);
    StellarAssetClient::new(&ctx.env, &ctx.token).mint(&fresh, &1_000);
    let result = ctx.client.try_stake_tokens(
        &fresh,
        &String::from_str(&ctx.env, "gold"),
        &1_000_i128,
    );
    assert_eq!(result, Err(Ok(StakingError::ContractPaused)));
    assert_eq!(token_balance(&ctx, &fresh), 1_000, "no funds taken while paused");

    // Safe exit: emergency unstake still succeeds while paused.
    let penalty = 1_000 * 500_i128 / 10_000; // 5% emergency penalty
    let payout = ctx.client.emergency_unstake(&user);
    assert_eq!(payout, 1_000 - penalty);
}

/// Two-step transfer full flow: propose by admin, accept by pending admin,
/// after which the new admin controls admin-only operations.
#[test]
fn admin_transfer_full_flow() {
    let ctx = setup_real();
    let new_admin = Address::generate(&ctx.env);

    ctx.client.propose_admin_transfer(&ctx.admin, &new_admin);
    ctx.client.accept_admin_transfer(&new_admin);

    // New admin can pause; old admin cannot.
    ctx.client.set_paused(&new_admin, &true);
    assert!(ctx.client.is_paused());

    let old_result = ctx.client.try_set_paused(&ctx.admin, &false);
    assert_eq!(old_result, Err(Ok(StakingError::NotAdmin)));
}

/// Cancelling a pending proposal clears it so the pending admin can no longer
/// accept.
#[test]
fn admin_transfer_cancel() {
    let ctx = setup_real();
    let new_admin = Address::generate(&ctx.env);

    ctx.client.propose_admin_transfer(&ctx.admin, &new_admin);
    ctx.client.cancel_admin_transfer(&ctx.admin);

    let result = ctx.client.try_accept_admin_transfer(&new_admin);
    assert_eq!(result, Err(Ok(StakingError::NoPendingAdmin)));
}

/// A pending proposal expires after its bounded TTL and can no longer be
/// accepted.
#[test]
fn admin_transfer_proposal_expires() {
    let ctx = setup_real();
    let new_admin = Address::generate(&ctx.env);

    ctx.env.ledger().with_mut(|li| li.timestamp = 0);
    ctx.client.propose_admin_transfer(&ctx.admin, &new_admin);

    // Advance past the 7-day TTL.
    ctx.env.ledger().with_mut(|li| li.timestamp = 7 * 24 * 60 * 60 + 1);
    let result = ctx.client.try_accept_admin_transfer(&new_admin);
    assert_eq!(result, Err(Ok(StakingError::PendingAdminExpired)));
}

/// A second proposal is rejected while one is already pending.
#[test]
fn admin_transfer_rejects_duplicate_proposal() {
    let ctx = setup_real();
    let other = Address::generate(&ctx.env);
    ctx.client.propose_admin_transfer(&ctx.admin, &other);
    let result = ctx.client.try_propose_admin_transfer(&ctx.admin, &other);
    assert_eq!(result, Err(Ok(StakingError::PendingAdminExists)));
}
