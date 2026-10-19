use crate::errors::ContractError;
use crate::types::{EligibilityStatus, EligibilityRecord};
use soroban_sdk::{Address, BytesN, Env};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Mentor eligibility record for a program.
    MentorEligibility(Address, BytesN<32>),
    /// Mentee eligibility record for a program.
    MenteeEligibility(Address, BytesN<32>),
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

pub fn put_mentor_eligibility(
    env: &Env,
    mentor: &Address,
    program_id: &BytesN<32>,
    status: EligibilityStatus,
    reason_hash: BytesN<32>,
    expires_at: u64,
) {
    let now = env.ledger().timestamp();
    let record = EligibilityRecord {
        participant: mentor.clone(),
        program_id: program_id.clone(),
        status,
        reason_hash,
        expires_at,
        updated_at: now,
    };

    let key = DataKey::MentorEligibility(mentor.clone(), program_id.clone());
    env.storage().persistent().set(&key, &record);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentor_eligibility(
    env: &Env,
    mentor: &Address,
    program_id: &BytesN<32>,
) -> Option<EligibilityRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorEligibility(mentor.clone(), program_id.clone()))
}

pub fn is_mentor_eligible(
    env: &Env,
    mentor: &Address,
    program_id: &BytesN<32>,
    now: u64,
) -> bool {
    if let Some(record) = get_mentor_eligibility(env, mentor, program_id) {
        record.status == EligibilityStatus::Eligible && record.expires_at > now
    } else {
        false
    }
}

pub fn put_mentee_eligibility(
    env: &Env,
    mentee: &Address,
    program_id: &BytesN<32>,
    status: EligibilityStatus,
    reason_hash: BytesN<32>,
    expires_at: u64,
) {
    let now = env.ledger().timestamp();
    let record = EligibilityRecord {
        participant: mentee.clone(),
        program_id: program_id.clone(),
        status,
        reason_hash,
        expires_at,
        updated_at: now,
    };

    let key = DataKey::MenteeEligibility(mentee.clone(), program_id.clone());
    env.storage().persistent().set(&key, &record);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentee_eligibility(
    env: &Env,
    mentee: &Address,
    program_id: &BytesN<32>,
) -> Option<EligibilityRecord> {
    env.storage()
        .persistent()
        .get(&DataKey::MenteeEligibility(mentee.clone(), program_id.clone()))
}

pub fn is_mentee_eligible(
    env: &Env,
    mentee: &Address,
    program_id: &BytesN<32>,
    now: u64,
) -> bool {
    if let Some(record) = get_mentee_eligibility(env, mentee, program_id) {
        record.status == EligibilityStatus::Eligible && record.expires_at > now
    } else {
        false
    }
}
