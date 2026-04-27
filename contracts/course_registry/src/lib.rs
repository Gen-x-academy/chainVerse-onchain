#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, contracterror,
    Address, Env, Symbol, panic_with_error
};

// Errors
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotAdmin = 1,
    CourseNotFound = 2,
    CourseInactive = 3,
    AlreadyInitialized = 4,
    NotInitialized = 5,
}

// Storage Keys
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Course(Symbol),
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

        // Validate: prices must be non-negative
        if price_xlm < 0 || price_chv < 0 {
            panic!("prices must be non-negative");
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

        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ContractError::CourseNotFound);
        }

        let mut course: Course =
            env.storage().persistent().get(&key).unwrap();

        course.is_active = is_active;

        env.storage().persistent().set(&key, &course);
        Ok(())
    }

    // Deactivate Course
    pub fn deactivate_course(env: Env, course_id: Symbol) -> Result<(), ContractError> {
        Self::require_admin(&env)?;

        let key = DataKey::Course(course_id.clone());

        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ContractError::CourseNotFound);
        }

        let mut course: Course = env.storage().persistent().get(&key).unwrap();
        course.is_active = false;

        env.storage().persistent().set(&key, &course);
        Ok(())
    }


    // Get Course
    pub fn get_course(env: Env, course_id: Symbol) -> Course {
        let key = DataKey::Course(course_id);

        if !env.storage().persistent().has(&key) {
            panic_with_error!(&env, ContractError::CourseNotFound);
        }

        env.storage().persistent().get(&key).unwrap()
    }

    // Purchase Check
    // (Used by payment contract later)
    pub fn assert_course_active(env: Env, course_id: Symbol) {
        let course = Self::get_course(env.clone(), course_id);

        if !course.is_active {
            panic_with_error!(&env, ContractError::CourseInactive);
        }
    }
}