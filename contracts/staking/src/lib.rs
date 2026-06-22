#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, BytesN, Env, String,
};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    Tier(String),
    Stake(Address),
    TotalStaked,
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct StakingConfig {
    pub staking_enabled: bool,
    /// Emergency-unstake penalty in basis points (100–10 000).
    pub emergency_unstake_penalty_bps: u32,
    /// Token used for staking.
    pub staking_token: Address,
    /// Separate pool token used for rewards (unused in unit tests but required
    /// for full contract compatibility).
    pub reward_pool: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct StakingTier {
    pub id: String,
    pub name: String,
    /// Minimum amount required to stake into this tier.
    pub min_stake_amount: i128,
    /// Seconds the stake is locked before a normal unstake is allowed.
    pub lock_duration: u64,
    pub reward_multiplier_bps: u32,
    pub base_rate_bps: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct StakeInfo {
    pub staker: Address,
    pub amount: i128,
    pub tier_id: String,
    pub staked_at: u64,
    pub unlock_at: u64,
    pub claimed_rewards: i128,
    pub emergency_unstaked: bool,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AdminNotSet = 1,
    Unauthorized = 2,
    /// Penalty bps was 0 or out of the 100–10 000 range.
    InvalidPenaltyBps = 3,
    StakingNotConfigured = 4,
    StakingDisabled = 5,
    TierNotFound = 6,
    BelowMinimumStake = 7,
    StakeNotFound = 8,
    StillLocked = 9,
    Overflow = 10,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const STAKE_TTL_LEDGERS: u32 = 518_400; // ~30 days

/// Minimum allowed emergency-unstake penalty.
///
/// A penalty of 0 bps makes emergency-unstake identical to normal unstake,
/// giving stakers a free exit and rendering the lock period meaningless.
/// Enforced both at config time (`set_staking_config`) and at execution time
/// (`emergency_unstake`) as defense-in-depth.
const MIN_PENALTY_BPS: u32 = 100; // 1 %

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {
    // -----------------------------------------------------------------------
    // Bootstrap
    // -----------------------------------------------------------------------

    /// Store the contract admin (call once after deployment).
    pub fn set_admin(env: Env, admin: Address) {
        let old_admin = env.storage().instance().get::<_, Address>(&DataKey::Admin);
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events().publish(
            (soroban_sdk::symbol_short!("ADM_CHNG"),),
            (old_admin, admin),
        );
    }

    // -----------------------------------------------------------------------
    // Admin – configuration
    // -----------------------------------------------------------------------

    /// Set the global staking configuration.
    ///
    /// `emergency_unstake_penalty_bps` must be in the range 100–10 000
    /// (i.e. 1 %–100 %); values below `MIN_PENALTY_BPS` are rejected so that
    /// emergency-unstake always carries a meaningful disincentive.
    pub fn set_staking_config(
        env: Env,
        admin: Address,
        config: StakingConfig,
    ) -> Result<(), Error> {
        Self::assert_admin(&env, &admin)?;

        if config.emergency_unstake_penalty_bps < MIN_PENALTY_BPS
            || config.emergency_unstake_penalty_bps > 10_000
        {
            return Err(Error::InvalidPenaltyBps);
        }

        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }

    /// Create a new staking tier.
    pub fn create_staking_tier(env: Env, admin: Address, tier: StakingTier) -> Result<(), Error> {
        Self::assert_admin(&env, &admin)?;

        env.storage()
            .persistent()
            .set(&DataKey::Tier(tier.id.clone()), &tier);
        env.storage().persistent().extend_ttl(
            &DataKey::Tier(tier.id.clone()),
            STAKE_TTL_LEDGERS,
            STAKE_TTL_LEDGERS,
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin – penalty withdrawal
    // -----------------------------------------------------------------------

    /// Transfer accumulated penalty funds (contract balance minus active
    /// stakes) to `recipient`.
    pub fn withdraw_penalties(env: Env, admin: Address, recipient: Address) -> Result<(), Error> {
        Self::assert_admin(&env, &admin)?;

        let config = Self::load_config(&env)?;
        let token_client = token::Client::new(&env, &config.staking_token);

        let contract_balance = token_client.balance(&env.current_contract_address());
        let total_staked: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);

        let penalty_balance = contract_balance.saturating_sub(total_staked);
        if penalty_balance > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &recipient,
                &penalty_balance,
            );
        }
        Ok(())
    }

    /// Permanently burn accumulated penalty funds (contract balance minus
    /// active stakes) instead of transferring them out.
    ///
    /// This is the irreversible counterpart to [`Self::withdraw_penalties`]:
    /// it removes the penalty pool from the token's total supply rather than
    /// sending it to a recipient. The penalty pool is computed identically
    /// (`contract_balance − total_staked`), so active stakes are never touched.
    pub fn burn_penalties(env: Env, admin: Address) -> Result<(), Error> {
        Self::assert_admin(&env, &admin)?;

        let config = Self::load_config(&env)?;
        let token_client = token::Client::new(&env, &config.staking_token);

        let contract_balance = token_client.balance(&env.current_contract_address());
        let total_staked: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);

        let penalty_balance = contract_balance.saturating_sub(total_staked);
        if penalty_balance > 0 {
            token_client.burn(&env.current_contract_address(), &penalty_balance);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // User – stake / unstake
    // -----------------------------------------------------------------------

    /// Lock `amount` tokens in the given tier.
    pub fn stake_tokens(
        env: Env,
        staker: Address,
        tier_id: String,
        amount: i128,
    ) -> Result<(), Error> {
        staker.require_auth();

        let config = Self::load_config(&env)?;
        if !config.staking_enabled {
            return Err(Error::StakingDisabled);
        }

        let tier: StakingTier = env
            .storage()
            .persistent()
            .get(&DataKey::Tier(tier_id.clone()))
            .ok_or(Error::TierNotFound)?;

        if amount < tier.min_stake_amount {
            return Err(Error::BelowMinimumStake);
        }

        let token_client = token::Client::new(&env, &config.staking_token);
        token_client.transfer(&staker, &env.current_contract_address(), &amount);

        let now = env.ledger().timestamp();
        let unlock_at = now.checked_add(tier.lock_duration).ok_or(Error::Overflow)?;

        let stake = StakeInfo {
            staker: staker.clone(),
            amount,
            tier_id,
            staked_at: now,
            unlock_at,
            claimed_rewards: 0,
            emergency_unstaked: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Stake(staker.clone()), &stake);
        env.storage().persistent().extend_ttl(
            &DataKey::Stake(staker.clone()),
            STAKE_TTL_LEDGERS,
            STAKE_TTL_LEDGERS,
        );

        Self::adjust_total_staked(&env, amount);
        Ok(())
    }

    /// Return full principal after the lock period has elapsed.
    pub fn unstake_tokens(env: Env, staker: Address) -> Result<(), Error> {
        staker.require_auth();

        let config = Self::load_config(&env)?;

        let stake: StakeInfo = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(staker.clone()))
            .ok_or(Error::StakeNotFound)?;

        let now = env.ledger().timestamp();
        if now < stake.unlock_at {
            return Err(Error::StillLocked);
        }

        let token_client = token::Client::new(&env, &config.staking_token);
        token_client.transfer(&env.current_contract_address(), &staker, &stake.amount);

        env.storage()
            .persistent()
            .remove(&DataKey::Stake(staker.clone()));
        Self::adjust_total_staked(&env, -stake.amount);
        Ok(())
    }

    /// Emergency unstake: return `amount - penalty` immediately (no rewards).
    ///
    /// Defense-in-depth: re-checks the penalty floor even though
    /// `set_staking_config` already enforces it, guarding against any future
    /// path that could store a zero-penalty config.
    pub fn emergency_unstake(env: Env, staker: Address) -> Result<(), Error> {
        staker.require_auth();

        let config = Self::load_config(&env)?;

        // Defense-in-depth: verify the stored penalty still meets the floor
        // even if config validation is somehow bypassed in the future.
        if config.emergency_unstake_penalty_bps < MIN_PENALTY_BPS {
            return Err(Error::InvalidPenaltyBps);
        }

        let stake: StakeInfo = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(staker.clone()))
            .ok_or(Error::StakeNotFound)?;

        let penalty = stake
            .amount
            .checked_mul(config.emergency_unstake_penalty_bps as i128)
            .ok_or(Error::Overflow)?
            .checked_div(10_000)
            .ok_or(Error::Overflow)?;

        let amount_returned = stake.amount.checked_sub(penalty).ok_or(Error::Overflow)?;

        let token_client = token::Client::new(&env, &config.staking_token);
        if amount_returned > 0 {
            token_client.transfer(&env.current_contract_address(), &staker, &amount_returned);
        }
        // Penalty is intentionally left in the contract.

        env.storage()
            .persistent()
            .remove(&DataKey::Stake(staker.clone()));
        // Reduce total_staked by the FULL original amount so that the penalty
        // remainder is correctly reflected in the contract balance surplus.
        Self::adjust_total_staked(&env, -stake.amount);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn get_stake_info(env: Env, staker: Address) -> Option<StakeInfo> {
        env.storage().persistent().get(&DataKey::Stake(staker))
    }

    pub fn get_staking_config(env: Env) -> Result<StakingConfig, Error> {
        Self::load_config(&env)
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::assert_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn assert_admin(env: &Env, caller: &Address) -> Result<(), Error> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::AdminNotSet)?;
        stored.require_auth();
        if stored != *caller {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }

    fn load_config(env: &Env) -> Result<StakingConfig, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .ok_or(Error::StakingNotConfigured)
    }

    fn adjust_total_staked(env: &Env, delta: i128) {
        let current: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalStaked, &current.saturating_add(delta));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;