#![no_std]

pub mod subscription;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, BytesN, Env,
    String,
};

const MIN_PENALTY_BPS: u32 = 100; // 1% minimum penalty for emergency unstake
const PENDING_ADMIN_TTL: u64 = 7 * 24 * 60 * 60; // 7 days

#[contracttype]
pub enum DataKey {
    Admin,
    Config,
    Tier(String),
    Stake(Address),
    TotalStaked,
    Paused,
    PendingAdmin,
    PendingAdminProposedAt,
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
    PendingAdminExists = 9,
    NoPendingAdmin = 10,
    NotPendingAdmin = 11,
    PendingAdminExpired = 12,
    NotAdmin = 13,
    ContractPaused = 14,
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
        if env.storage().instance().get(&DataKey::Paused).unwrap_or(false) {
            return Err(StakingError::ContractPaused);
        }
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

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(env: Env, caller: Address, new_wasm_hash: BytesN<32>) -> Result<(), StakingError> {
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin { return Err(StakingError::Unauthorized); }
        caller.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgraded"),), new_wasm_hash);
        Ok(())
    }

    /// Emergency pause. Admin can stop new deposits (stakes) while still
    /// allowing safe exits (withdrawals via `emergency_unstake`).
    pub fn set_paused(env: Env, caller: Address, paused: bool) -> Result<(), StakingError> {
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin { return Err(StakingError::NotAdmin); }
        caller.require_auth();
        env.storage().instance().set(&DataKey::Paused, &paused);
        env.events().publish((symbol_short!("PAUSED"),), paused);
        Ok(())
    }

    /// Returns whether deposits are currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Two-step admin transfer (step 1): the current admin proposes a new
    /// admin. The proposal is bounded and expires after `PENDING_ADMIN_TTL`.
    pub fn propose_admin_transfer(env: Env, caller: Address, new_admin: Address) -> Result<(), StakingError> {
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin { return Err(StakingError::NotAdmin); }
        caller.require_auth();
        if env.storage().instance().has(&DataKey::PendingAdmin) {
            return Err(StakingError::PendingAdminExists);
        }
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
        env.storage().instance().set(&DataKey::PendingAdminProposedAt, &env.ledger().timestamp());
        env.events().publish((symbol_short!("ADMIN_PROP"),), (caller, new_admin));
        Ok(())
    }

    /// Two-step admin transfer (step 2): only the proposed pending admin may
    /// accept, and only before the proposal expires.
    pub fn accept_admin_transfer(env: Env, caller: Address) -> Result<(), StakingError> {
        let mut config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        let pending: Address = env.storage().instance().get(&DataKey::PendingAdmin).ok_or(StakingError::NoPendingAdmin)?;
        if caller != pending { return Err(StakingError::NotPendingAdmin); }
        caller.require_auth();
        let proposed_at: u64 = env.storage().instance().get(&DataKey::PendingAdminProposedAt).unwrap_or(0);
        if env.ledger().timestamp() > proposed_at.saturating_add(PENDING_ADMIN_TTL) {
            return Err(StakingError::PendingAdminExpired);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage().instance().remove(&DataKey::PendingAdminProposedAt);
        let new_admin = caller.clone();
        config.admin = new_admin.clone();
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish((symbol_short!("ADMIN_ACCEPT"),), new_admin);
        Ok(())
    }

    /// Cancels a pending admin transfer. Callable by the current admin.
    pub fn cancel_admin_transfer(env: Env, caller: Address) -> Result<(), StakingError> {
        let config: StakingConfig = env.storage().instance().get(&DataKey::Config).ok_or(StakingError::NotInitialized)?;
        if caller != config.admin { return Err(StakingError::NotAdmin); }
        caller.require_auth();
        if !env.storage().instance().has(&DataKey::PendingAdmin) {
            return Err(StakingError::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.storage().instance().remove(&DataKey::PendingAdminProposedAt);
        env.events().publish((symbol_short!("ADMIN_CANCEL"),), caller);
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
