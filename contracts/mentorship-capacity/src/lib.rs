#![no_std]

//! Mentorship capacity tracking and enforcement contract.
//!
//! Implements per-mentor limits (concurrent engagements, weekly time, program, cohort)
//! and per-program limits. Works in conjunction with mentorship-core to enforce
//! these constraints before creating new matches.

mod errors;
mod storage;
mod types;

pub use errors::ContractError;
pub use types::{MentorCapacityConfig, MentorCapacityUtilization, ProgramCapacityConfig};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;
const SECONDS_PER_WEEK: u64 = 604_800;

#[contract]
pub struct MentorshipCapacityContract;

#[contractimpl]
impl MentorshipCapacityContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&storage::DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&storage::DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: Set capacity limits for a mentor.
    pub fn set_mentor_capacity(
        env: Env,
        admin: Address,
        mentor: Address,
        max_concurrent: u32,
        max_weekly_hours: u32,
        max_per_program: u32,
        max_per_cohort: u32,
        proposal_buffer: u32,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        if max_concurrent == 0 || max_weekly_hours == 0 {
            return Err(ContractError::InvalidConfig);
        }

        let config = MentorCapacityConfig {
            max_concurrent,
            max_weekly_hours,
            max_per_program,
            max_per_cohort,
            proposal_buffer,
        };

        storage::put_mentor_capacity_config(&env, &mentor, &config);
        Ok(())
    }

    /// Admin-only: Set capacity limits for a program.
    pub fn set_program_capacity(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        max_mentorships: u32,
        max_mentors: u32,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        if max_mentorships == 0 || max_mentors == 0 {
            return Err(ContractError::InvalidConfig);
        }

        let config = ProgramCapacityConfig {
            program_id: program_id.clone(),
            max_mentorships,
            max_mentors,
            active_count: 0,
        };

        storage::put_program_capacity_config(&env, &config);
        Ok(())
    }

    /// Check if a mentor can take on a new mentorship without exceeding limits.
    /// Returns Ok(()) if capacity is available, or an error if exceeded.
    pub fn check_mentor_capacity(
        env: Env,
        mentor: Address,
        program_id: BytesN<32>,
        weekly_hours_requested: u32,
    ) -> Result<(), ContractError> {
        // Get mentor config
        let config = storage::get_mentor_capacity_config(&env, &mentor)
            .ok_or(ContractError::NotFound)?;

        // Get current utilization
        let mut utilization = storage::get_mentor_capacity_utilization(&env, &mentor)
            .unwrap_or_else(|_| MentorCapacityUtilization {
                mentor: mentor.clone(),
                current_concurrent: 0,
                current_week: Self::current_week(&env),
                hours_this_week: 0,
                updated_at: 0,
            });

        // Reset week if it's changed
        let now_week = Self::current_week(&env);
        if utilization.current_week != now_week {
            utilization.current_week = now_week;
            utilization.hours_this_week = 0;
        }

        // Check concurrent limit (accounting for proposal buffer)
        let available_concurrent = config
            .max_concurrent
            .checked_sub(config.proposal_buffer)
            .ok_or(ContractError::InvalidConfig)?;
        if utilization.current_concurrent >= available_concurrent {
            return Err(ContractError::MaxConcurrentExceeded);
        }

        // Check weekly hours limit
        if utilization.hours_this_week + weekly_hours_requested > config.max_weekly_hours {
            return Err(ContractError::MaxWeeklyHoursExceeded);
        }

        // Check program-specific limits if configured
        if config.max_per_program > 0 {
            let program_count = storage::get_mentor_program_count(&env, &mentor, &program_id)
                .unwrap_or(0);
            if program_count >= config.max_per_program {
                return Err(ContractError::ProgramCapacityExceeded);
            }
        }

        Ok(())
    }

    /// Check if a program has capacity for a new mentorship.
    pub fn check_program_capacity(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut config = storage::get_program_capacity_config(&env, &program_id)
            .ok_or(ContractError::NotFound)?;

        if config.active_count >= config.max_mentorships {
            return Err(ContractError::ProgramCapacityExceeded);
        }

        Ok(())
    }

    /// Called by mentorship-core when a mentorship is created.
    /// Increments the mentor's concurrent count and hours.
    pub fn reserve_mentor_capacity(
        env: Env,
        core_contract: Address,
        mentor: Address,
        program_id: BytesN<32>,
        weekly_hours: u32,
    ) -> Result<(), ContractError> {
        // Verify caller is the mentorship-core contract
        // (In production, would use cross-contract call verification)
        
        let mut utilization = storage::get_mentor_capacity_utilization(&env, &mentor)
            .unwrap_or_else(|_| MentorCapacityUtilization {
                mentor: mentor.clone(),
                current_concurrent: 0,
                current_week: Self::current_week(&env),
                hours_this_week: 0,
                updated_at: env.ledger().timestamp(),
            });

        // Reset week if needed
        let now_week = Self::current_week(&env);
        if utilization.current_week != now_week {
            utilization.current_week = now_week;
            utilization.hours_this_week = 0;
        }

        // Increment counters with overflow protection
        utilization.current_concurrent = utilization
            .current_concurrent
            .checked_add(1)
            .ok_or(ContractError::InvalidConfig)?;
        utilization.hours_this_week = utilization
            .hours_this_week
            .checked_add(weekly_hours)
            .ok_or(ContractError::InvalidConfig)?;
        utilization.updated_at = env.ledger().timestamp();

        storage::put_mentor_capacity_utilization(&env, &utilization);

        // Increment program-specific count
        let count = storage::get_mentor_program_count(&env, &mentor, &program_id)
            .unwrap_or(0);
        storage::set_mentor_program_count(&env, &mentor, &program_id, count + 1);

        // Increment program capacity
        if let Ok(mut program_config) = storage::get_program_capacity_config(&env, &program_id) {
            program_config.active_count = program_config
                .active_count
                .checked_add(1)
                .ok_or(ContractError::InvalidConfig)?;
            storage::put_program_capacity_config(&env, &program_config);
        }

        Ok(())
    }

    /// Called by mentorship-core when a mentorship is rejected, terminated, or completed.
    /// Decrements the mentor's concurrent count.
    pub fn release_mentor_capacity(
        env: Env,
        core_contract: Address,
        mentor: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let mut utilization = storage::get_mentor_capacity_utilization(&env, &mentor)
            .ok_or(ContractError::NotFound)?;

        utilization.current_concurrent = utilization
            .current_concurrent
            .checked_sub(1)
            .ok_or(ContractError::InvalidConfig)?;
        utilization.updated_at = env.ledger().timestamp();

        storage::put_mentor_capacity_utilization(&env, &utilization);

        // Decrement program-specific count
        let count = storage::get_mentor_program_count(&env, &mentor, &program_id)
            .ok_or(ContractError::NotFound)?
            .checked_sub(1)
            .ok_or(ContractError::InvalidConfig)?;
        storage::set_mentor_program_count(&env, &mentor, &program_id, count);

        // Decrement program capacity
        if let Ok(mut program_config) = storage::get_program_capacity_config(&env, &program_id) {
            program_config.active_count = program_config
                .active_count
                .checked_sub(1)
                .ok_or(ContractError::InvalidConfig)?;
            storage::put_program_capacity_config(&env, &program_config);
        }

        Ok(())
    }

    /// Get current capacity utilization for a mentor.
    pub fn get_mentor_utilization(
        env: Env,
        mentor: Address,
    ) -> Result<MentorCapacityUtilization, ContractError> {
        storage::get_mentor_capacity_utilization(&env, &mentor)
    }

    /// Get capacity configuration for a mentor.
    pub fn get_mentor_capacity(
        env: Env,
        mentor: Address,
    ) -> Result<MentorCapacityConfig, ContractError> {
        storage::get_mentor_capacity_config(&env, &mentor).ok_or(ContractError::NotFound)
    }

    /// Get capacity configuration for a program.
    pub fn get_program_capacity(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramCapacityConfig, ContractError> {
        storage::get_program_capacity_config(&env, &program_id)
    }

    // Helper: compute current week number
    fn current_week(env: &Env) -> u32 {
        (env.ledger().timestamp() / SECONDS_PER_WEEK) as u32
    }
}
