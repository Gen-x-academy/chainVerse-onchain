use crate::errors::ContractError;
use crate::types::{MentorCapacityConfig, MentorCapacityUtilization, ProgramCapacityConfig};
use soroban_sdk::{Address, BytesN, Env};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Mentor capacity configuration.
    MentorCapacityConfig(Address),
    /// Current capacity utilization for a mentor.
    MentorCapacityUtilization(Address),
    /// Per-mentor per-program active mentorship count.
    MentorProgramCount(Address, BytesN<32>),
    /// Program capacity configuration.
    ProgramCapacityConfig(BytesN<32>),
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn require_admin(env: &Env, admin: &Address) -> Result<Address, ContractError> {
    let current = get_admin(env).ok_or(ContractError::NotInitialized)?;
    if current != *admin {
        return Err(ContractError::NotAdmin);
    }
    admin.require_auth();
    Ok(current)
}

pub fn get_mentor_capacity_config(env: &Env, mentor: &Address) -> Option<MentorCapacityConfig> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorCapacityConfig(mentor.clone()))
}

pub fn put_mentor_capacity_config(
    env: &Env,
    mentor: &Address,
    config: &MentorCapacityConfig,
) {
    let key = DataKey::MentorCapacityConfig(mentor.clone());
    env.storage().persistent().set(&key, config);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentor_capacity_utilization(
    env: &Env,
    mentor: &Address,
) -> Result<MentorCapacityUtilization, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorCapacityUtilization(mentor.clone()))
        .ok_or(ContractError::NotFound)
}

pub fn put_mentor_capacity_utilization(
    env: &Env,
    utilization: &MentorCapacityUtilization,
) {
    let key = DataKey::MentorCapacityUtilization(utilization.mentor.clone());
    env.storage().persistent().set(&key, utilization);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentor_program_count(
    env: &Env,
    mentor: &Address,
    program_id: &BytesN<32>,
) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorProgramCount(mentor.clone(), program_id.clone()))
}

pub fn set_mentor_program_count(
    env: &Env,
    mentor: &Address,
    program_id: &BytesN<32>,
    count: u32,
) {
    let key = DataKey::MentorProgramCount(mentor.clone(), program_id.clone());
    env.storage().persistent().set(&key, &count);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_program_capacity_config(
    env: &Env,
    program_id: &BytesN<32>,
) -> Result<ProgramCapacityConfig, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::ProgramCapacityConfig(program_id.clone()))
        .ok_or(ContractError::NotFound)
}

pub fn put_program_capacity_config(env: &Env, config: &ProgramCapacityConfig) {
    let key = DataKey::ProgramCapacityConfig(config.program_id.clone());
    env.storage().persistent().set(&key, config);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}
