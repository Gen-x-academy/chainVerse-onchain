#![no_std]

mod errors;
mod storage;
mod types;

pub use errors::MentorError;
pub use types::*;

use mentor_types::{
    DeliveryMode, ProgramSummary, QueryFilters, MentorComparison,
};
use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, BytesN, Env, String, Vec,
};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct MentorDiscovery;

#[contractimpl]
impl MentorDiscovery {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the mentor discovery contract.
    pub fn init(env: Env, admin: Address, registry: Address) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if storage::get_admin(&env).is_some() {
            return Err(MentorError::AlreadyInitialized);
        }

        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_registry(&env, &registry);
        storage::set_program_counter(&env, 0);

        env.events()
            .publish((symbol_short!("admin_set"),), admin);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Program Registration & Management
    // -----------------------------------------------------------------------

    /// Register a new mentorship program.
    pub fn register_program(
        env: Env,
        organization: Address,
        topic_hash: BytesN<32>,
        cohort_start: u64,
        duration_hours: u32,
        max_seats: u32,
        mode: u8,
        program_uri: String,
    ) -> Result<u32, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        organization.require_auth();

        if max_seats == 0 || duration_hours == 0 {
            return Err(MentorError::InvalidInput);
        }

        if mode > 2 {
            return Err(MentorError::InvalidDeliveryMode);
        }

        let now = env.ledger().timestamp();
        let program_id = storage::increment_program_counter(&env);

        let program = ProgramSummary {
            program_id,
            organization: organization.clone(),
            topic_hash,
            cohort_start,
            duration_hours,
            max_seats,
            available_seats: max_seats,
            mode: match mode {
                0 => DeliveryMode::Synchronous,
                1 => DeliveryMode::Asynchronous,
                _ => DeliveryMode::Hybrid,
            },
            created_at: now,
            updated_at: now,
        };

        storage::set_program(&env, program_id, program);

        env.events().publish(
            (symbol_short!("prog_reg"),),
            (program_id, organization),
        );

        Ok(program_id)
    }

    /// Get a program by ID.
    pub fn get_program(env: Env, program_id: u32) -> Option<ProgramSummary> {
        storage::get_program(&env, program_id)
    }

    /// Update program available seats.
    pub fn update_program_seats(
        env: Env,
        admin: Address,
        program_id: u32,
        available_seats: u32,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        let mut program = storage::get_program(&env, program_id)
            .ok_or(MentorError::ProgramNotFound)?;

        if available_seats > program.max_seats {
            return Err(MentorError::InvalidInput);
        }

        program.available_seats = available_seats;
        program.updated_at = env.ledger().timestamp();

        storage::set_program(&env, program_id, program);

        env.events()
            .publish((symbol_short!("prog_upd"),), program_id);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Program Discovery & Filtering
    // -----------------------------------------------------------------------

    /// Query programs with filters (pagination).
    pub fn query_programs(
        env: Env,
        filters: QueryFilters,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<ProgramSummary>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        let max_limit = 50u32;
        let actual_limit = if limit > max_limit { max_limit } else { limit };

        // Get all programs and filter
        let all_programs = storage::get_all_programs(&env);

        let mut results = Vec::new(&env);

        for program in all_programs.iter() {
            // Apply filters
            if let Some(topic_hash) = &filters.topic_hash {
                if program.topic_hash != *topic_hash {
                    continue;
                }
            }

            if program.available_seats < filters.min_seats {
                continue;
            }

            if let Some(mode) = filters.mode {
                if program.mode != mode {
                    continue;
                }
            }

            if let Some(org) = &filters.organization {
                if program.organization != *org {
                    continue;
                }
            }

            if let Some((min_time, _max_time)) = (
                filters.cohort_start_min,
                filters.cohort_start_max,
            ) {
                if program.cohort_start < min_time {
                    continue;
                }
            }

            results.push_back(program.clone());
        }

        // Apply pagination
        let start = offset as usize;
        let end = (offset as usize + actual_limit as usize).min(results.len());

        let mut paginated = Vec::new(&env);
        for i in start..end {
            if let Some(prog) = results.get(i as u32) {
                paginated.push_back(prog);
            }
        }

        Ok(paginated)
    }

    /// List all programs (with offset/limit for pagination).
    pub fn list_programs(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<ProgramSummary>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        let max_limit = 100u32;
        let actual_limit = if limit > max_limit { max_limit } else { limit };

        let all_programs = storage::get_all_programs(&env);

        let start = offset as usize;
        let end = (offset as usize + actual_limit as usize).min(all_programs.len());

        let mut result = Vec::new(&env);
        for i in start..end {
            if let Some(prog) = all_programs.get(i as u32) {
                result.push_back(prog);
            }
        }

        Ok(result)
    }

    /// Get total program count.
    pub fn get_program_count(env: Env) -> u32 {
        storage::get_program_counter(&env)
    }

    // -----------------------------------------------------------------------
    // Mentor Filtering for Programs
    // -----------------------------------------------------------------------

    /// Get mentors eligible for a specific program.
    /// This queries the registry contract and filters.
    pub fn get_eligible_mentors_for_program(
        env: Env,
        program_id: u32,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Address>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        let _program = storage::get_program(&env, program_id)
            .ok_or(MentorError::ProgramNotFound)?;

        // TODO: Call registry contract to get mentors
        // For now, return empty (would implement cross-contract call)
        Ok(Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Caching & Performance Hints
    // -----------------------------------------------------------------------

    /// Get recommended cache TTL for a program query result.
    pub fn get_program_cache_ttl(env: Env, program_id: u32) -> Result<u32, MentorError> {
        let program = storage::get_program(&env, program_id)
            .ok_or(MentorError::ProgramNotFound)?;

        let now = env.ledger().timestamp();
        let time_until_cohort = if program.cohort_start > now {
            program.cohort_start - now
        } else {
            0
        };

        // Cache based on proximity to cohort start
        let ttl = if time_until_cohort > 86400 * 30 {
            // > 30 days: cache for 1 day
            86400
        } else if time_until_cohort > 86400 * 7 {
            // > 7 days: cache for 6 hours
            21600
        } else {
            // Close to start: cache for 1 hour
            3600
        };

        Ok(ttl)
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

    /// Get the registry contract address.
    pub fn get_registry(env: Env) -> Option<Address> {
        storage::get_registry(&env)
    }
}
