#![no_std]

//! Per-dependency readiness reporting for the capstone workspace.
//!
//! Readiness is tracked independently of liveness so a live deployment can
//! still report which external capabilities are unavailable. Only public
//! states, timestamps, and commitments to off-chain diagnostics are stored.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Administrator,
    Liveness,
    Report(Dependency),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dependency {
    Persistence,
    Search,
    Files,
    Notifications,
    Collaboration,
    PlagiarismChecks,
    Payments,
    LedgerAdapters,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadinessState {
    Unknown,
    Ready,
    Degraded,
    Unavailable,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadinessReport {
    pub dependency: Dependency,
    pub state: ReadinessState,
    pub checked_at: u64,
    pub note_commitment: BytesN<32>,
}

#[contract]
pub struct CapstoneReadinessContract;

#[contractimpl]
impl CapstoneReadinessContract {
    pub fn initialize(env: Env, administrator: Address) -> Result<(), ContractError> {
        administrator.require_auth();
        if env.storage().instance().has(&DataKey::Administrator) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::Administrator, &administrator);
        env.storage().instance().set(&DataKey::Liveness, &false);
        Ok(())
    }

    pub fn report(
        env: Env,
        administrator: Address,
        dependency: Dependency,
        state: ReadinessState,
        note_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_administrator(&env, &administrator)?;
        let key = DataKey::Report(dependency);
        let report = ReadinessReport {
            dependency,
            state,
            checked_at: env.ledger().timestamp(),
            note_commitment,
        };
        env.storage().persistent().set(&key, &report);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("CAPRDY"),),
            (dependency, state, report.checked_at),
        );
        Ok(())
    }

    /// Liveness is reported separately from dependency readiness.
    pub fn set_liveness(
        env: Env,
        administrator: Address,
        live: bool,
    ) -> Result<(), ContractError> {
        Self::require_administrator(&env, &administrator)?;
        env.storage().instance().set(&DataKey::Liveness, &live);
        env.events()
            .publish((symbol_short!("CAPLIVE"),), (live,));
        Ok(())
    }

    pub fn get_report(env: Env, dependency: Dependency) -> Option<ReadinessReport> {
        env.storage().persistent().get(&DataKey::Report(dependency))
    }

    pub fn liveness(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Liveness)
            .unwrap_or(false)
    }

    pub fn dependencies(env: Env) -> Vec<Dependency> {
        Vec::from_array(
            &env,
            &[
                Dependency::Persistence,
                Dependency::Search,
                Dependency::Files,
                Dependency::Notifications,
                Dependency::Collaboration,
                Dependency::PlagiarismChecks,
                Dependency::Payments,
                Dependency::LedgerAdapters,
            ],
        )
    }

    fn require_administrator(
        env: &Env,
        administrator: &Address,
    ) -> Result<(), ContractError> {
        administrator.require_auth();
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::Administrator)
            .ok_or(ContractError::NotInitialized)?;
        if &configured != administrator {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }
}
