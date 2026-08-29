#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, String, Symbol,
};

const CONTRACT_VERSION: &str = "1.0.0";

// TTL constants: ~1 year at 6-second ledgers (issue #735)
const COURSE_MIN_TTL: u32 = 3_110_400;
const COURSE_MAX_TTL: u32 = 6_220_800;

// Errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    CourseNotFound = 3,
    CourseInactive = 4,
    NotInitialized = 5,
    ContractPaused = 6,
    InvalidPrice = 7,
    EnrollmentNotFound = 8,
}

// Storage Keys
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Course(BytesN<32>),
    Enrollment(BytesN<32>, Address),
    Paused,
    Enrollment(Address, Symbol),
}

// Course Struct
#[contracttype]
#[derive(Clone)]
pub struct Course {
    pub course_id: BytesN<32>,
    pub price_xlm: i128,
    pub price_chv: i128,
    pub is_active: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnrollmentRecord {
    pub student: Address,
    pub course_id: Symbol,
    pub enrolled: bool,
}

// Contract
#[contract]
pub struct CourseRegistryContract;

#[contractimpl]
impl CourseRegistryContract {
    // Initialize Admin (run once)
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // Internal Admin Check
    fn require_admin(env: &Env) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;

        admin.require_auth();
        Ok(())
    }

    // Add or Update Course
    pub fn upsert_course(
        env: Env,
        course_id: BytesN<32>,
        price_xlm: i128,
        price_chv: i128,
        is_active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env)?;

        if env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(ContractError::ContractPaused);
        }

        // Validate: prices must be non-negative
        if price_xlm < 0 || price_chv < 0 {
            return Err(ContractError::InvalidPrice);
        }

        let course = Course {
            course_id: course_id.clone(),
            price_xlm,
            price_chv,
            is_active,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Course(course_id.clone()), &course);
        env.storage().persistent().extend_ttl(
            &DataKey::Course(course_id.clone()),
            COURSE_MIN_TTL,
            COURSE_MAX_TTL,
        );
        Ok(())
    }

    // Toggle Course Activation
    pub fn toggle_course(
        env: Env,
        course_id: BytesN<32>,
        is_active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env)?;

        let key = DataKey::Course(course_id.clone());

        let mut course: Course = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CourseNotFound)?;

        course.is_active = is_active;

        env.storage().persistent().set(&key, &course);
        env.storage()
            .persistent()
            .extend_ttl(&key, COURSE_MIN_TTL, COURSE_MAX_TTL);
        Ok(())
    }

    // Deactivate Course
    pub fn deactivate_course(env: Env, course_id: BytesN<32>) -> Result<(), ContractError> {
        Self::require_admin(&env)?;

        let key = DataKey::Course(course_id.clone());

        let mut course: Course = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CourseNotFound)?;
        course.is_active = false;

        env.storage().persistent().set(&key, &course);
        env.storage()
            .persistent()
            .extend_ttl(&key, COURSE_MIN_TTL, COURSE_MAX_TTL);
        Ok(())
    }

    // Get Course
    pub fn get_course(env: Env, course_id: BytesN<32>) -> Result<Course, ContractError> {
        let key = DataKey::Course(course_id);

        let course = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CourseNotFound)?;
        // Refresh TTL on read so active courses never silently expire (issue #735)
        env.storage()
            .persistent()
            .extend_ttl(&key, COURSE_MIN_TTL, COURSE_MAX_TTL);
        Ok(course)
    }

    // Purchase Check
    // (Used by payment contract later)
    pub fn assert_course_active(env: Env, course_id: BytesN<32>) -> Result<(), ContractError> {
        let course = Self::get_course(env.clone(), course_id)?;

        if !course.is_active {
            return Err(ContractError::CourseInactive);
        }
        Ok(())
    }

    /// Admin-controlled enrollment attestation consumed by library-rights.
    pub fn set_enrollment(
        env: Env,
        student: Address,
        course_id: Symbol,
        enrolled: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env)?;
        let key = DataKey::Enrollment(student.clone(), course_id.clone());
        env.storage().persistent().set(
            &key,
            &EnrollmentRecord {
                student,
                course_id,
                enrolled,
            },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, COURSE_MIN_TTL, COURSE_MAX_TTL);
        Ok(())
    }

    /// Versioned typed read interface for authoritative enrollment checks.
    pub fn is_enrolled(env: Env, student: Address, course_id: Symbol) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, EnrollmentRecord>(&DataKey::Enrollment(student, course_id))
            .map(|record| record.enrolled)
            .unwrap_or(false)
    // Enroll a user in a course
    pub fn enroll(env: Env, course_id: BytesN<32>, user: Address) -> Result<(), ContractError> {
        user.require_auth();

        let course_key = DataKey::Course(course_id.clone());
        let course: Course = env
            .storage()
            .persistent()
            .get(&course_key)
            .ok_or(ContractError::CourseNotFound)?;

        if !course.is_active {
            return Err(ContractError::CourseInactive);
        }

        let enrollment_key = DataKey::Enrollment(course_id.clone(), user.clone());
        env.storage().persistent().set(&enrollment_key, &());
        env.storage()
            .persistent()
            .extend_ttl(&enrollment_key, COURSE_MIN_TTL, COURSE_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("ENROLL"),), (course_id, user));

        Ok(())
    }

    // Check if a user is enrolled in a course
    pub fn is_enrolled(env: Env, course_id: BytesN<32>, user: Address) -> bool {
        let enrollment_key = DataKey::Enrollment(course_id, user);
        env.storage().persistent().has(&enrollment_key)
    }

    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Admin-only: pause the contract.
    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((soroban_sdk::symbol_short!("PAUSED"),), (caller,));
        Ok(())
    }

    /// Admin-only: unpause the contract.
    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((soroban_sdk::symbol_short!("UNPAUSED"),), (caller,));
        Ok(())
    }

    /// Admin-only: upgrade the current contract to `new_wasm_hash`.
    pub fn upgrade(
        env: Env,
        admin: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;

        if stored_admin != admin {
            return Err(ContractError::NotAdmin);
        }

        env.deployer().update_current_contract_wasm(new_wasm_.hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
}
