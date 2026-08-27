#![no_std]

pub mod subscription;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, vec, Address, BytesN,
    Env, String, Vec,
};

const MIN_PENALTY_BPS: u32 = 100; // 1% minimum penalty for emergency unstake

/// TTL bounds (in ledger entries, ~5s per entry) applied to every
/// read/write of persistent Tier and Stake records. MIN_TTL is the lower
/// bound we renew to, MAX_TTL the upper bound we are allowed to set.
/// ~180 days and ~360 days respectively.
const MIN_TTL: u32 = 3_110_400;
const MAX_TTL: u32 = 6_220_800;

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    Tier(String),
    Stake(Address),
    TotalStaked,
    PenaltyPool,
    ActiveTiers,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum StakingError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    TierNotFound = 4,
    InsufficientBalance = 5,
    StillLocked = 6,
    NoStake = 7,
    PenaltyTooLow = 8,
    TierExists = 9,
    PenaltyInsufficient = 10,
}

#[contracttype]
#[derive(Clone)]
pub struct StakingConfig {
    pub token: Address,
    pub admin: Address,
    pub emergency_unstake_penalty_bps: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct TierConfig {
    pub min_amount: i128,
    pub lock_period: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct StakeRecord {
    pub amount: i128,
    pub tier: String,
    pub staked_at: u64,
}

#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        emergency_unstake_penalty_bps: u32,
    ) -> Result<(), StakingError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(StakingError::AlreadyInitialized);
        }
        if emergency_unstake_penalty_bps < MIN_PENALTY_BPS {
            return Err(StakingError::PenaltyTooLow);
        }
        admin.require_auth();
        let config = StakingConfig { token, admin: admin.clone(), emergency_unstake_penalty_bps };
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::PenaltyPool, &0i128);
        env.storage().instance().set(&DataKey::ActiveTiers, &vec![&env]);
        env.events().publish(
            (symbol_short!("init"),),
            (config.admin.clone(), config.token.clone(), config.emergency_unstake_penalty_bps),
        );
        Ok(())
    }

    /// Admin-only: register a new stake tier. Refuses to overwrite an
    /// existing tier (use `update_tier` for that).
    pub fn add_tier(
        env: Env,
        caller: Address,
        name: String,
        tier_config: TierConfig,
    ) -> Result<(), StakingError> {
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin {
            return Err(StakingError::Unauthorized);
        }
        caller.require_auth();
        let dk = DataKey::Tier(name.clone());
        if env.storage().persistent().has(&dk) {
            return Err(StakingError::TierExists);
        }
        env.storage().persistent().set(&dk, &tier_config);
        env.storage().persistent().extend_ttl(&dk, MIN_TTL, MAX_TTL);

        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        let mut active: Vec<String> =
            env.storage().instance().get(&DataKey::ActiveTiers).unwrap_or_else(|| vec![&env]);
        active.push_back(name.clone());
        env.storage().instance().set(&DataKey::ActiveTiers, &active);

        env.events().publish((symbol_short!("tier_add"),), (caller, name, tier_config));
        Ok(())
    }

    /// Admin-only: update an existing tier's configuration.
    pub fn update_tier(
        env: Env,
        caller: Address,
        name: String,
        tier_config: TierConfig,
    ) -> Result<(), StakingError> {
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin {
            return Err(StakingError::Unauthorized);
        }
        caller.require_auth();
        let dk = DataKey::Tier(name.clone());
        if !env.storage().persistent().has(&dk) {
            return Err(StakingError::TierNotFound);
        }
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        env.storage().persistent().set(&dk, &tier_config);
        env.storage().persistent().extend_ttl(&dk, MIN_TTL, MAX_TTL);

        env.events().publish((symbol_short!("tier_upd"),), (caller, name, tier_config));
        Ok(())
    }

    pub fn stake_tokens(env: Env, user: Address, tier: String, amount: i128) -> Result<(), StakingError> {
        user.require_auth();
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let tier_dk = DataKey::Tier(tier.clone());
        let tier_cfg: TierConfig =
            env.storage().persistent().get(&tier_dk).ok_or(StakingError::TierNotFound)?;
        env.storage().persistent().extend_ttl(&tier_dk, MIN_TTL, MAX_TTL);
        if amount < tier_cfg.min_amount {
            return Err(StakingError::InsufficientBalance);
        }
        token::Client::new(&env, &config.token).transfer(&user, &env.current_contract_address(), &amount);
        let staked_at = env.ledger().timestamp();
        let record = StakeRecord { amount, tier, staked_at };
        let stake_dk = DataKey::Stake(user.clone());
        env.storage().persistent().set(&stake_dk, &record);
        env.storage().persistent().extend_ttl(&stake_dk, MIN_TTL, MAX_TTL);
        let total: i128 = env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0);
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        env.storage().instance().set(&DataKey::TotalStaked, &(total + amount));
        env.events().publish(
            (symbol_short!("stake"),),
            (user.clone(), record.tier.clone(), record.amount, staked_at),
        );
        Ok(())
    }

    pub fn emergency_unstake(env: Env, user: Address) -> Result<i128, StakingError> {
        user.require_auth();
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let stake_dk = DataKey::Stake(user.clone());
        let record: StakeRecord =
            env.storage().persistent().get(&stake_dk).ok_or(StakingError::NoStake)?;
        env.storage().persistent().extend_ttl(&stake_dk, MIN_TTL, MAX_TTL);
        let penalty = record.amount * config.emergency_unstake_penalty_bps as i128 / 10_000;
        let payout = record.amount - penalty;
        env.storage().persistent().remove(&stake_dk);

        let total: i128 = env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0);
        let pool: i128 = env.storage().instance().get(&DataKey::PenaltyPool).unwrap_or(0);
        env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
        env.storage().instance().set(&DataKey::TotalStaked, &(total - record.amount));
        env.storage().instance().set(&DataKey::PenaltyPool, &(pool + penalty));

        token::Client::new(&env, &config.token).transfer(&env.current_contract_address(), &user, &payout);
        let now = env.ledger().timestamp();
        env.events().publish(
            (symbol_short!("unstake"),),
            (user.clone(), record.amount, penalty, payout, now),
        );
        env.events().publish((symbol_short!("penalty"),), (user.clone(), penalty, now));
        Ok(payout)
    }

    /// Admin-only: withdraw accrued penalties to `recipient`. Only ever moves
    /// funds from the penalty pool; it can never touch staked principal.
    pub fn withdraw_penalties(
        env: Env,
        caller: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), StakingError> {
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin {
            return Err(StakingError::Unauthorized);
        }
        caller.require_auth();
        let pool: i128 = env.storage().instance().get(&DataKey::PenaltyPool).unwrap_or(0);
        if amount < 0 || amount > pool {
            return Err(StakingError::PenaltyInsufficient);
        }
        if amount > 0 {
            env.storage().instance().extend_ttl(MIN_TTL, MAX_TTL);
            env.storage().instance().set(&DataKey::PenaltyPool, &(pool - amount));
            token::Client::new(&env, &config.token).transfer(
                &env.current_contract_address(),
                &recipient,
                &amount,
            );
        }
        env.events().publish(
            (symbol_short!("wdraw_pen"),),
            (caller, recipient, amount),
        );
        Ok(())
    }

    pub fn get_stake(env: Env, user: Address) -> Option<StakeRecord> {
        let stake_dk = DataKey::Stake(user);
        let record: Option<StakeRecord> = env.storage().persistent().get(&stake_dk);
        if record.is_some() {
            env.storage().persistent().extend_ttl(&stake_dk, MIN_TTL, MAX_TTL);
        }
        record
    }

    pub fn get_unlock_timestamp(env: Env, user: Address) -> Option<u64> {
        let stake_dk = DataKey::Stake(user.clone());
        let record: Option<StakeRecord> = env.storage().persistent().get(&stake_dk);
        let record = match record {
            Some(r) => r,
            None => return None,
        };
        env.storage().persistent().extend_ttl(&stake_dk, MIN_TTL, MAX_TTL);
        let tier_dk = DataKey::Tier(record.tier.clone());
        let tier: TierConfig = env.storage().persistent().get(&tier_dk)?;
        env.storage().persistent().extend_ttl(&tier_dk, MIN_TTL, MAX_TTL);
        Some(record.staked_at + tier.lock_period)
    }

    pub fn get_tier(env: Env, name: String) -> Option<TierConfig> {
        let tier_dk = DataKey::Tier(name);
        let tier: Option<TierConfig> = env.storage().persistent().get(&tier_dk);
        if tier.is_some() {
            env.storage().persistent().extend_ttl(&tier_dk, MIN_TTL, MAX_TTL);
        }
        tier
    }

    pub fn get_active_tiers(env: Env) -> Vec<String> {
        env.storage().instance().get(&DataKey::ActiveTiers).unwrap_or_else(|| vec![&env])
    }

    pub fn get_configuration(env: Env) -> StakingConfig {
        env.storage().instance().get(&DataKey::Config).unwrap()
    }

    pub fn get_total_staked(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0)
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), StakingError> {
        let config: StakingConfig =
            env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin {
            return Err(StakingError::Unauthorized);
        }
        caller.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod subscription_test;
#[cfg(test)]
mod subscription_extended_test;
#[cfg(test)]
mod subscription_payment_test;
#[cfg(test)]
mod subscription_suite_test;
