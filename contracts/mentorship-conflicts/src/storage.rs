use crate::errors::ContractError;
use crate::types::{ConflictType, ConflictCheckResult};
use soroban_sdk::{Address, BytesN, Env, Vec};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Organization membership for a participant.
    Organization(Address),
    /// Reporting relationship: Address reports to another Address.
    ReportsTo(Address),
    /// Financial interest between two parties.
    FinancialInterest(Address, Address),
    /// Declared conflict between two parties.
    DeclaredConflict(Address, Address),
    /// Audit trail of conflict checks.
    ConflictCheckHistory(BytesN<32>),
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

pub fn set_organization(env: &Env, participant: &Address, organization: &Address) {
    let key = DataKey::Organization(participant.clone());
    env.storage().persistent().set(&key, organization);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_organization(env: &Env, participant: &Address) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Organization(participant.clone()))
}

pub fn set_reports_to(env: &Env, subordinate: &Address, supervisor: &Address) {
    let key = DataKey::ReportsTo(subordinate.clone());
    env.storage().persistent().set(&key, supervisor);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_reports_to(env: &Env, subordinate: &Address) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::ReportsTo(subordinate.clone()))
}

pub fn set_financial_interest(env: &Env, party_a: &Address, party_b: &Address) {
    let key = DataKey::FinancialInterest(party_a.clone(), party_b.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn has_financial_interest(env: &Env, party_a: &Address, party_b: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::FinancialInterest(party_a.clone(), party_b.clone()))
        || env.storage()
            .persistent()
            .has(&DataKey::FinancialInterest(party_b.clone(), party_a.clone()))
}

pub fn set_declared_conflict(env: &Env, party_a: &Address, party_b: &Address) {
    let key = DataKey::DeclaredConflict(party_a.clone(), party_b.clone());
    env.storage().persistent().set(&key, &true);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn has_declared_conflict(env: &Env, party_a: &Address, party_b: &Address) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::DeclaredConflict(party_a.clone(), party_b.clone()))
        || env.storage()
            .persistent()
            .has(&DataKey::DeclaredConflict(party_b.clone(), party_a.clone()))
}

pub fn record_check(
    env: &Env,
    check_id: &BytesN<32>,
    result: &ConflictCheckResult,
) {
    let key = DataKey::ConflictCheckHistory(check_id.clone());
    env.storage().persistent().set(&key, result);
    env.storage()
        .persistent()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

pub fn get_check_result(
    env: &Env,
    check_id: &BytesN<32>,
) -> Option<ConflictCheckResult> {
    env.storage()
        .persistent()
        .get(&DataKey::ConflictCheckHistory(check_id.clone()))
}
