#![no_std]

pub mod subscription;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, BytesN, Env,
    String,
};

const MIN_PENALTY_BPS: u32 = 100; // 1% minimum penalty for emergency unstake

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    Tier(String),
    // Fix #843: active flag for a tier. Staking against an inactive tier must fail.
    TierActive(String),
    Stake(Address),
    TotalStaked,
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
    // Fix #843: tier already exists when adding.
    TierExists = 9,
    // Fix #843: staking against an inactive tier.
    TierInactive = 10,
    // Fix #844: repeated staking cannot change the tier of an existing stake.
    TierChangeNotAllowed = 11,
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

fn require_admin(env: &Env, caller: &Address) -> Result<(), StakingError> {
    let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
    if *caller != config.admin {
        return Err(StakingError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

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
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }

    pub fn stake_tokens(env: Env, user: Address, tier: String, amount: i128) -> Result<(), StakingError> {
        user.require_auth();
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let tier_cfg: TierConfig = env.storage().persistent().get(&DataKey::Tier(tier.clone())).ok_or(StakingError::TierNotFound)?;
        // Fix #843: staking against a deactivated tier must fail.
        let active: bool = env.storage().persistent().get(&DataKey::TierActive(tier.clone())).unwrap_or(false);
        if !active { return Err(StakingError::TierInactive); }
        if amount < tier_cfg.min_amount { return Err(StakingError::InsufficientBalance); }
        // Fix #844: a repeated stake must not change the tier of an existing
        // record. Check this BEFORE transferring any tokens so a rejected
        // repeat never moves funds.
        let existing: Option<StakeRecord> = env.storage().persistent().get(&DataKey::Stake(user.clone()));
        if let Some(existing) = existing.as_ref() {
            if existing.tier != tier {
                return Err(StakingError::TierChangeNotAllowed);
            }
        }
        token::Client::new(&env, &config.token).transfer(&user, &env.current_contract_address(), &amount);
        // Fix #844: merge repeated stakes into a single record instead of
        // overwriting — never orphan a previous deposit.
        let record = match existing {
            Some(mut existing) => {
                existing.amount += amount;
                existing
            }
            None => StakeRecord { amount, tier, staked_at: env.ledger().timestamp() },
        };
        env.storage().persistent().set(&DataKey::Stake(user.clone()), &record);
        // TotalStaked only increased by the newly deposited amount.
        let total: i128 = env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalStaked, &(total + amount));
        Ok(())
    }

    /// Admin-only: create a new (active) tier.
    pub fn add_tier(env: Env, caller: Address, name: String, config: TierConfig) -> Result<(), StakingError> {
        require_admin(&env, &caller)?;
        if env.storage().persistent().has(&DataKey::Tier(name.clone())) {
            return Err(StakingError::TierExists);
        }
        env.storage().persistent().set(&DataKey::Tier(name.clone()), &config);
        env.storage().persistent().set(&DataKey::TierActive(name.clone()), &true);
        env.events().publish((symbol_short!("tier_added"),), name);
        Ok(())
    }

    /// Admin-only: update an existing tier's configuration (keeps its active state).
    pub fn update_tier(env: Env, caller: Address, name: String, config: TierConfig) -> Result<(), StakingError> {
        require_admin(&env, &caller)?;
        if !env.storage().persistent().has(&DataKey::Tier(name.clone())) {
            return Err(StakingError::TierNotFound);
        }
        env.storage().persistent().set(&DataKey::Tier(name.clone()), &config);
        env.events().publish((symbol_short!("tier_updated"),), name);
        Ok(())
    }

    /// Admin-only: deactivate a tier so that new stakes against it fail.
    pub fn deactivate_tier(env: Env, caller: Address, name: String) -> Result<(), StakingError> {
        require_admin(&env, &caller)?;
        if !env.storage().persistent().has(&DataKey::Tier(name.clone())) {
            return Err(StakingError::TierNotFound);
        }
        env.storage().persistent().set(&DataKey::TierActive(name.clone()), &false);
        env.events().publish((symbol_short!("tier_deactivated"),), name);
        Ok(())
    }

    /// Query whether a tier exists and is active.
    pub fn is_tier_active(env: Env, name: String) -> bool {
        if !env.storage().persistent().has(&DataKey::Tier(name.clone())) {
            return false;
        }
        env.storage().persistent().get(&DataKey::TierActive(name)).unwrap_or(false)
    }

    pub fn emergency_unstake(env: Env, user: Address) -> Result<i128, StakingError> {
        user.require_auth();
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let record: StakeRecord = env.storage().persistent().get(&DataKey::Stake(user.clone())).ok_or(StakingError::NoStake)?;
        let penalty = record.amount * config.emergency_unstake_penalty_bps as i128 / 10_000;
        let payout = record.amount - penalty;
        env.storage().persistent().remove(&DataKey::Stake(user.clone()));
        token::Client::new(&env, &config.token).transfer(&env.current_contract_address(), &user, &payout);
        Ok(payout)
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), StakingError> {
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin { return Err(StakingError::Unauthorized); }
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
