use crate::errors::ContractError;
use crate::types::{MentorshipMatch, MenteeAcceptance, MentorAcceptance};
use soroban_sdk::{Address, BytesN, Env, Vec};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Primary mentorship record, keyed by stable match ID.
    Mentorship(BytesN<32>),
    /// Uniqueness guard: (mentor, mentee) pair prevents duplicate active mentorships.
    ActiveMentorship(Address, Address),
    /// Signed acceptance by mentee.
    MenteeAcceptance(BytesN<32>),
    /// Signed acceptance by mentor.
    MentorAcceptance(BytesN<32>),
    /// Index of mentorship IDs for a mentor.
    MentorIndex(Address),
    /// Index of mentorship IDs for a mentee.
    MenteeIndex(Address),
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

pub fn has_mentorship(env: &Env, match_id: &BytesN<32>) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Mentorship(match_id.clone()))
}

pub fn put_mentorship(env: &Env, mentorship: &MentorshipMatch) {
    let key = DataKey::Mentorship(mentorship.id.clone());
    env.storage().persistent().set(&key, mentorship);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

    // Also append to index for each participant
    append_to_mentor_index(env, &mentorship.mentor, &mentorship.id);
    append_to_mentee_index(env, &mentorship.mentee, &mentorship.id);
}

pub fn load_mentorship(env: &Env, match_id: &BytesN<32>) -> Result<MentorshipMatch, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::Mentorship(match_id.clone()))
        .ok_or(ContractError::MentorshipNotFound)
}

pub fn load_and_extend_mentorship(env: &Env, match_id: &BytesN<32>) -> Result<MentorshipMatch, ContractError> {
    let key = DataKey::Mentorship(match_id.clone());
    let mentorship: MentorshipMatch = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::MentorshipNotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    Ok(mentorship)
}

pub fn has_active_mentorship(env: &Env, mentor: &Address, mentee: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::ActiveMentorship(mentor.clone(), mentee.clone()))
}

pub fn set_active_mentorship(env: &Env, mentor: &Address, mentee: &Address, match_id: &BytesN<32>) {
    let key = DataKey::ActiveMentorship(mentor.clone(), mentee.clone());
    env.storage().persistent().set(&key, match_id);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn clear_active_mentorship(env: &Env, mentor: &Address, mentee: &Address) {
    env.storage()
        .persistent()
        .remove(&DataKey::ActiveMentorship(mentor.clone(), mentee.clone()));
}

pub fn put_mentee_acceptance(env: &Env, acceptance: &MenteeAcceptance) {
    let key = DataKey::MenteeAcceptance(acceptance.match_id.clone());
    env.storage().persistent().set(&key, acceptance);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn load_mentee_acceptance(env: &Env, match_id: &BytesN<32>) -> Result<MenteeAcceptance, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::MenteeAcceptance(match_id.clone()))
        .ok_or(ContractError::NotFound)
}

pub fn load_and_extend_mentee_acceptance(
    env: &Env,
    match_id: &BytesN<32>,
) -> Result<MenteeAcceptance, ContractError> {
    let key = DataKey::MenteeAcceptance(match_id.clone());
    let acceptance: MenteeAcceptance = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::NotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    Ok(acceptance)
}

pub fn put_mentor_acceptance(env: &Env, acceptance: &MentorAcceptance) {
    let key = DataKey::MentorAcceptance(acceptance.match_id.clone());
    env.storage().persistent().set(&key, acceptance);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn load_mentor_acceptance(env: &Env, match_id: &BytesN<32>) -> Result<MentorAcceptance, ContractError> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorAcceptance(match_id.clone()))
        .ok_or(ContractError::NotFound)
}

pub fn load_and_extend_mentor_acceptance(
    env: &Env,
    match_id: &BytesN<32>,
) -> Result<MentorAcceptance, ContractError> {
    let key = DataKey::MentorAcceptance(match_id.clone());
    let acceptance: MentorAcceptance = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::NotFound)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    Ok(acceptance)
}

fn append_to_mentor_index(env: &Env, mentor: &Address, match_id: &BytesN<32>) {
    let key = DataKey::MentorIndex(mentor.clone());
    let mut ids: Vec<BytesN<32>> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|_| Vec::new(env));
    ids.push_back(match_id.clone());
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentor_index(env: &Env, mentor: &Address) -> Vec<BytesN<32>> {
    env.storage()
        .persistent()
        .get(&DataKey::MentorIndex(mentor.clone()))
        .unwrap_or_else(|_| Vec::new(env))
}

fn append_to_mentee_index(env: &Env, mentee: &Address, match_id: &BytesN<32>) {
    let key = DataKey::MenteeIndex(mentee.clone());
    let mut ids: Vec<BytesN<32>> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|_| Vec::new(env));
    ids.push_back(match_id.clone());
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_mentee_index(env: &Env, mentee: &Address) -> Vec<BytesN<32>> {
    env.storage()
        .persistent()
        .get(&DataKey::MenteeIndex(mentee.clone()))
        .unwrap_or_else(|_| Vec::new(env))
}
