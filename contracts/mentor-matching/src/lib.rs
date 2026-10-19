#![no_std]

mod errors;
mod storage;
mod types;

pub use errors::MentorError;
pub use types::*;

use mentor_types::{
    MenteeProfile, Recommendation, MatchReason,
};
use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, Env,
};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct MentorMatching;

#[contractimpl]
impl MentorMatching {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the mentor matching contract.
    pub fn init(
        env: Env,
        admin: Address,
        registry: Address,
        discovery: Address,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if storage::get_admin(&env).is_some() {
            return Err(MentorError::AlreadyInitialized);
        }

        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_registry(&env, &registry);
        storage::set_discovery(&env, &discovery);

        env.events()
            .publish((symbol_short!("admin_set"),), admin);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Recommendation Generation
    // -----------------------------------------------------------------------

    /// Generate mentee recommendations (simplified scoring).
    /// In production, this would call registry and discovery contracts
    /// to gather data and apply a weighted scoring algorithm.
    pub fn get_recommendations(
        env: Env,
        mentee_profile: MenteeProfile,
        program_id: u32,
        limit: u32,
    ) -> Result<Vec<Recommendation>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if limit == 0 || limit > 50 {
            return Err(MentorError::InvalidInput);
        }

        // TODO: Implement full matching logic
        // This would:
        // 1. Query registry for all active mentors
        // 2. Get program details from discovery
        // 3. Calculate fit scores based on:
        //    - Expertise overlap (30%)
        //    - Availability intersection (20%)
        //    - Price competitiveness (15%)
        //    - Trust signals (20%)
        //    - Program constraints (15%)
        // 4. Sort by score and return top N

        // For now, return empty recommendations
        let recommendations = soroban_sdk::Vec::new(&env);
        Ok(recommendations)
    }

    /// Calculate fit score between mentee and mentor for a program.
    pub fn calculate_fit_score(
        env: Env,
        _mentee: MenteeProfile,
        _mentor: Address,
        _program_id: u32,
    ) -> Result<u32, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        // TODO: Implement scoring algorithm
        // Returns score from 0-10000 (representing 0-100.00%)
        Ok(5000) // Placeholder: 50.00%
    }

    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Update registry contract reference.
    pub fn set_registry(
        env: Env,
        admin: Address,
        registry: Address,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::set_registry(&env, &registry);
        env.events()
            .publish((symbol_short!("reg_set"),), registry);

        Ok(())
    }

    /// Update discovery contract reference.
    pub fn set_discovery(
        env: Env,
        admin: Address,
        discovery: Address,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::set_discovery(&env, &discovery);
        env.events()
            .publish((symbol_short!("disc_set"),), discovery);

        Ok(())
    }

    /// Get registry contract address.
    pub fn get_registry(env: Env) -> Option<Address> {
        storage::get_registry(&env)
    }

    /// Get discovery contract address.
    pub fn get_discovery(env: Env) -> Option<Address> {
        storage::get_discovery(&env)
    }
}
