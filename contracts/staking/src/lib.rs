#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, BytesN, Env, String,
};

const MIN_PENALTY_BPS: u32 = 100; // 1% minimum penalty for emergency unstake

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    Tier(String),
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
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Config, &config);
        Ok(())
    }

    pub fn stake_tokens(env: Env, user: Address, tier: String, amount: i128) -> Result<(), StakingError> {
        user.require_auth();
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let tier_cfg: TierConfig = env.storage().persistent().get(&DataKey::Tier(tier.clone())).ok_or(StakingError::TierNotFound)?;
        if amount < tier_cfg.min_amount { return Err(StakingError::InsufficientBalance); }
        token::Client::new(&env, &config.token).transfer(&user, &env.current_contract_address(), &amount);
        let record = StakeRecord { amount, tier, staked_at: env.ledger().timestamp() };
        env.storage().persistent().set(&DataKey::Stake(user.clone()), &record);
        let total: i128 = env.storage().instance().get(&DataKey::TotalStaked).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalStaked, &(total + amount));
        Ok(())
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
}
