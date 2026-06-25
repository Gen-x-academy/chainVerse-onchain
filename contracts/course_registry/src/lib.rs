#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env,
    String, Symbol,
};

const CONTRACT_VERSION: &str = "1.0.0";

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
}

// Storage Keys
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Course(Symbol),
    Paused,
}

// Course Struct
#[contracttype]
#[derive(Clone)]
pub struct Course {
    pub course_id: Symbol,
    pub price_xlm: i128,
    pub price_chv: i128,
    pub is_active: bool,
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
        course_id: Symbol,
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
            .set(&DataKey::Course(course_id), &course);
        Ok(())
    }

    // Toggle Course Activation
    pub fn toggle_course(
        env: Env,
        course_id: Symbol,
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
        Ok(())
    }

    // Deactivate Course
    pub fn deactivate_course(env: Env, course_id: Symbol) -> Result<(), ContractError> {
        Self::require_admin(&env)?;

        let key = DataKey::Course(course_id.clone());

        let mut course: Course = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CourseNotFound)?;
        course.is_active = false;

        env.storage().persistent().set(&key, &course);
        Ok(())
    }

    // Get Course
    pub fn get_course(env: Env, course_id: Symbol) -> Result<Course, ContractError> {
        let key = DataKey::Course(course_id);

        env.storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CourseNotFound)
    }

    // Purchase Check
    // (Used by payment contract later)
    pub fn assert_course_active(env: Env, course_id: Symbol) -> Result<(), ContractError> {
        let course = Self::get_course(env.clone(), course_id)?;

        if !course.is_active {
            return Err(ContractError::CourseInactive);
        }
        Ok(())
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

        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }
}
