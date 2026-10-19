#![no_std]
#![allow(clippy::too_many_arguments)]

//! Privacy-minimized mentorship cohort configuration and enrollment.
//!
//! Participant records are keyed by caller-provided, high-entropy reference
//! commitments. Profiles, calendar details, and cohort setting contents remain
//! off-chain; only opaque commitments, dates, capacity, status, and coordinator
//! addresses are stored here.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const INSTANCE_MIN_TTL: u32 = 3_110_400;
const INSTANCE_MAX_TTL: u32 = 6_220_800;
const MAX_COORDINATORS: u32 = 16;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotCoordinator = 4,
    CohortNotFound = 5,
    CohortExists = 6,
    InvalidCapacity = 7,
    InvalidDates = 8,
    InvalidCoordinators = 9,
    EmptyReference = 10,
    EnrollmentClosed = 11,
    CapacityReached = 12,
    AlreadyEnrolled = 13,
    EnrollmentNotFound = 14,
    AlreadyWithdrawn = 15,
    CapacityBelowEnrollment = 16,
    Overflow = 17,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CohortConfig {
    pub program_id: BytesN<32>,
    pub cohort_id: BytesN<32>,
    pub enrollment_opens_at: u64,
    pub enrollment_closes_at: u64,
    pub starts_at: u64,
    pub ends_at: u64,
    pub capacity: u32,
    pub active_count: u32,
    pub coordinators: Vec<Address>,
    pub calendar_ref_hash: BytesN<32>,
    pub settings_hash: BytesN<32>,
    pub version: u32,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnrollmentStatus {
    Active = 1,
    Withdrawn = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Enrollment {
    pub cohort_id: BytesN<32>,
    pub participant_ref_hash: BytesN<32>,
    pub enrolled_at: u64,
    pub withdrawn_at: u64,
    pub status: EnrollmentStatus,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Cohort(BytesN<32>),
    Enrollment(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct MentorshipCohortsContract;

#[contractimpl]
impl MentorshipCohortsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Create a cohort using commitments to off-chain calendar and settings
    /// references. The admin supplies a nonempty, duplicate-free coordinator
    /// list of at most `MAX_COORDINATORS` addresses.
    pub fn create_cohort(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        cohort_id: BytesN<32>,
        enrollment_opens_at: u64,
        enrollment_closes_at: u64,
        starts_at: u64,
        ends_at: u64,
        capacity: u32,
        coordinators: Vec<Address>,
        calendar_ref_hash: BytesN<32>,
        settings_hash: BytesN<32>,
    ) -> Result<CohortConfig, ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Cohort(cohort_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::CohortExists);
        }
        Self::validate_config(
            &env,
            enrollment_opens_at,
            enrollment_closes_at,
            starts_at,
            ends_at,
            capacity,
            &coordinators,
            &calendar_ref_hash,
            &settings_hash,
        )?;

        let config = CohortConfig {
            program_id: program_id.clone(),
            cohort_id: cohort_id.clone(),
            enrollment_opens_at,
            enrollment_closes_at,
            starts_at,
            ends_at,
            capacity,
            active_count: 0,
            coordinators,
            calendar_ref_hash,
            settings_hash,
            version: 1,
            updated_at: env.ledger().timestamp(),
        };
        Self::save_cohort(&env, &key, &config);
        env.events().publish(
            (symbol_short!("COHNEW"),),
            (program_id, cohort_id, config.version),
        );
        Ok(config)
    }

    /// Replace cohort settings while preserving active enrollments. The
    /// cohort's capacity may not be reduced below its current active count.
    pub fn update_cohort(
        env: Env,
        admin: Address,
        cohort_id: BytesN<32>,
        enrollment_opens_at: u64,
        enrollment_closes_at: u64,
        starts_at: u64,
        ends_at: u64,
        capacity: u32,
        coordinators: Vec<Address>,
        calendar_ref_hash: BytesN<32>,
        settings_hash: BytesN<32>,
    ) -> Result<CohortConfig, ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Cohort(cohort_id.clone());
        let mut config: CohortConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CohortNotFound)?;
        Self::validate_config(
            &env,
            enrollment_opens_at,
            enrollment_closes_at,
            starts_at,
            ends_at,
            capacity,
            &coordinators,
            &calendar_ref_hash,
            &settings_hash,
        )?;
        if capacity < config.active_count {
            return Err(ContractError::CapacityBelowEnrollment);
        }
        let version = config.version.checked_add(1).ok_or(ContractError::Overflow)?;
        config.enrollment_opens_at = enrollment_opens_at;
        config.enrollment_closes_at = enrollment_closes_at;
        config.starts_at = starts_at;
        config.ends_at = ends_at;
        config.capacity = capacity;
        config.coordinators = coordinators;
        config.calendar_ref_hash = calendar_ref_hash;
        config.settings_hash = settings_hash;
        config.version = version;
        config.updated_at = env.ledger().timestamp();
        Self::save_cohort(&env, &key, &config);
        env.events().publish(
            (symbol_short!("COHUPD"),),
            (config.program_id.clone(), cohort_id, config.version),
        );
        Ok(config)
    }

    /// A listed coordinator can register a pseudonymous participant
    /// commitment while the enrollment window is open.
    pub fn enroll(
        env: Env,
        coordinator: Address,
        cohort_id: BytesN<32>,
        participant_ref_hash: BytesN<32>,
    ) -> Result<Enrollment, ContractError> {
        let mut config = Self::cohort(&env, &cohort_id)?;
        Self::require_coordinator(&env, &coordinator, &config)?;
        if participant_ref_hash == Self::zero_hash(&env) {
            return Err(ContractError::EmptyReference);
        }
        let now = env.ledger().timestamp();
        if now < config.enrollment_opens_at || now > config.enrollment_closes_at {
            return Err(ContractError::EnrollmentClosed);
        }
        let key = DataKey::Enrollment(cohort_id.clone(), participant_ref_hash.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyEnrolled);
        }
        if config.active_count >= config.capacity {
            return Err(ContractError::CapacityReached);
        }
        config.active_count = config
            .active_count
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        let enrollment = Enrollment {
            cohort_id: cohort_id.clone(),
            participant_ref_hash,
            enrolled_at: now,
            withdrawn_at: 0,
            status: EnrollmentStatus::Active,
        };
        env.storage().persistent().set(&key, &enrollment);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::save_cohort(&env, &DataKey::Cohort(cohort_id.clone()), &config);
        env.events().publish(
            (symbol_short!("COHENR"),),
            (cohort_id, config.active_count, now),
        );
        Ok(enrollment)
    }

    /// A listed coordinator can withdraw a participant commitment. Enrollment
    /// history remains available and a withdrawn reference cannot be enrolled
    /// again under the same cohort.
    pub fn withdraw(
        env: Env,
        coordinator: Address,
        cohort_id: BytesN<32>,
        participant_ref_hash: BytesN<32>,
    ) -> Result<Enrollment, ContractError> {
        let mut config = Self::cohort(&env, &cohort_id)?;
        Self::require_coordinator(&env, &coordinator, &config)?;
        let key = DataKey::Enrollment(cohort_id.clone(), participant_ref_hash);
        let mut enrollment: Enrollment = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EnrollmentNotFound)?;
        if enrollment.status == EnrollmentStatus::Withdrawn {
            return Err(ContractError::AlreadyWithdrawn);
        }
        enrollment.status = EnrollmentStatus::Withdrawn;
        enrollment.withdrawn_at = env.ledger().timestamp();
        config.active_count = config
            .active_count
            .checked_sub(1)
            .ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&key, &enrollment);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::save_cohort(&env, &DataKey::Cohort(cohort_id.clone()), &config);
        env.events().publish(
            (symbol_short!("COHWDR"),),
            (cohort_id, config.active_count, enrollment.withdrawn_at),
        );
        Ok(enrollment)
    }

    pub fn get_cohort(env: Env, cohort_id: BytesN<32>) -> Result<CohortConfig, ContractError> {
        Self::cohort(&env, &cohort_id)
    }

    pub fn get_enrollment(
        env: Env,
        cohort_id: BytesN<32>,
        participant_ref_hash: BytesN<32>,
    ) -> Result<Enrollment, ContractError> {
        let key = DataKey::Enrollment(cohort_id, participant_ref_hash);
        let enrollment = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EnrollmentNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(enrollment)
    }

    fn validate_config(
        env: &Env,
        enrollment_opens_at: u64,
        enrollment_closes_at: u64,
        starts_at: u64,
        ends_at: u64,
        capacity: u32,
        coordinators: &Vec<Address>,
        calendar_ref_hash: &BytesN<32>,
        settings_hash: &BytesN<32>,
    ) -> Result<(), ContractError> {
        if capacity == 0 {
            return Err(ContractError::InvalidCapacity);
        }
        if enrollment_opens_at >= enrollment_closes_at
            || enrollment_closes_at > starts_at
            || starts_at >= ends_at
        {
            return Err(ContractError::InvalidDates);
        }
        if coordinators.is_empty() || coordinators.len() > MAX_COORDINATORS {
            return Err(ContractError::InvalidCoordinators);
        }
        for index in 0..coordinators.len() {
            for other_index in (index + 1)..coordinators.len() {
                if coordinators.get(index) == coordinators.get(other_index) {
                    return Err(ContractError::InvalidCoordinators);
                }
            }
        }
        if *calendar_ref_hash == Self::zero_hash(env) || *settings_hash == Self::zero_hash(env) {
            return Err(ContractError::EmptyReference);
        }
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Self::extend_instance_ttl(env);
        Ok(())
    }

    fn require_coordinator(
        env: &Env,
        caller: &Address,
        config: &CohortConfig,
    ) -> Result<(), ContractError> {
        if !config.coordinators.contains(caller) {
            return Err(ContractError::NotCoordinator);
        }
        caller.require_auth();
        Ok(())
    }

    fn cohort(env: &Env, cohort_id: &BytesN<32>) -> Result<CohortConfig, ContractError> {
        let key = DataKey::Cohort(cohort_id.clone());
        let config = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CohortNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::extend_instance_ttl(env);
        Ok(config)
    }

    fn save_cohort(env: &Env, key: &DataKey, config: &CohortConfig) {
        env.storage().persistent().set(key, config);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::extend_instance_ttl(env);
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_MIN_TTL, INSTANCE_MAX_TTL);
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
