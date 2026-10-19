#![no_std]

mod errors;
mod storage;
mod types;

pub use errors::MentorError;
pub use types::*;

use mentor_types::{
    CompletionRecord, DeliveryMode, MentorComparison, MentorProfile, MentorError as MentorTypeError,
};
use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, BytesN, Env, String, Vec,
};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct MentorRegistry;

#[contractimpl]
impl MentorRegistry {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the mentor registry contract.
    pub fn init(env: Env, admin: Address) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if storage::get_admin(&env).is_some() {
            return Err(MentorError::AlreadyInitialized);
        }

        admin.require_auth();
        storage::set_admin(&env, &admin);
        storage::set_program_counter(&env, 0);

        env.events()
            .publish((symbol_short!("admin_set"),), admin);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Mentor Registration & Management
    // -----------------------------------------------------------------------

    /// Register a new mentor profile.
    pub fn register_mentor(
        env: Env,
        mentor: Address,
        expertise_tags_hash: BytesN<32>,
        availability_mask: u32,
        delivery_modes: u8,
        base_price: u64,
        profile_uri: String,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        mentor.require_auth();

        // Validate inputs
        if delivery_modes > 0x07 {
            return Err(MentorError::InvalidDeliveryMode);
        }
        if base_price == 0 && delivery_modes != 0 {
            // Allow free mentors only if explicitly no modes set
            // Actually, base_price=0 is valid for free mentors
        }

        // Check if mentor already registered
        if storage::mentor_exists(&env, &mentor) {
            return Err(MentorError::Unauthorized); // Already registered
        }

        let now = env.ledger().timestamp();
        let profile = MentorProfile {
            mentor_id: mentor.clone(),
            expertise_tags_hash,
            availability_mask,
            delivery_modes,
            base_price,
            active: true,
            profile_uri,
            created_at: now,
            updated_at: now,
        };

        storage::set_mentor(&env, &mentor, profile);
        storage::increment_active_count(&env);

        env.events().publish(
            (symbol_short!("mentor_reg"),),
            (mentor.clone(), base_price),
        );

        Ok(())
    }

    /// Update mentor status and basic info.
    pub fn update_mentor_status(
        env: Env,
        mentor: Address,
        active: bool,
        new_availability_mask: u32,
        new_price: u64,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        mentor.require_auth();

        let mut profile = storage::get_mentor(&env, &mentor)
            .ok_or(MentorError::MentorNotFound)?;

        let was_active = profile.active;
        profile.active = active;
        profile.availability_mask = new_availability_mask;
        profile.base_price = new_price;
        profile.updated_at = env.ledger().timestamp();

        // Track active mentor count changes
        if was_active && !active {
            storage::decrement_active_count(&env);
        } else if !was_active && active {
            storage::increment_active_count(&env);
        }

        storage::set_mentor(&env, &mentor, profile);

        env.events()
            .publish((symbol_short!("mentor_upd"),), (mentor.clone(), active));

        Ok(())
    }

    /// Get mentor profile by address.
    pub fn get_mentor(env: Env, mentor: Address) -> Option<MentorProfile> {
        storage::get_mentor(&env, &mentor)
    }

    /// List active mentors (with pagination).
    pub fn list_active_mentors(
        env: Env,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Address>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        let max_limit = 100u32;
        let actual_limit = if limit > max_limit { max_limit } else { limit };

        storage::list_active_mentors(&env, offset, actual_limit)
    }

    /// Get count of active mentors.
    pub fn get_active_mentor_count(env: Env) -> u32 {
        storage::get_active_count(&env)
    }

    // -----------------------------------------------------------------------
    // Trust Signals & Completions
    // -----------------------------------------------------------------------

    /// Record a completed mentorship session (trust signal).
    /// Only callable by an authorized program contract or admin.
    pub fn record_completion(
        env: Env,
        mentor: Address,
        mentee: Address,
        program_id: u32,
        rating: u8,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        // Require admin or program authorization
        let caller = env.current_contract_address();
        storage::require_admin_or_authorized(&env, &caller)?;

        if rating < 1 || rating > 5 {
            return Err(MentorError::InvalidInput);
        }

        // Verify mentor exists
        if !storage::mentor_exists(&env, &mentor) {
            return Err(MentorError::MentorNotFound);
        }

        let completion = CompletionRecord {
            mentee,
            program_id,
            rating,
            completed_at: env.ledger().timestamp(),
        };

        storage::add_completion(&env, &mentor, completion.clone());

        env.events().publish(
            (symbol_short!("complete"),),
            (mentor.clone(), rating),
        );

        Ok(())
    }

    /// Get mentor's average rating.
    pub fn get_mentor_avg_rating(env: Env, mentor: Address) -> Option<u8> {
        let completions = storage::get_completions(&env, &mentor);

        if completions.is_empty() {
            return None;
        }

        let sum: u64 = completions
            .iter()
            .map(|c| c.rating as u64)
            .fold(0u64, |a, b| a + b);

        let avg = (sum / completions.len() as u64) as u8;
        Some(avg)
    }

    /// Get mentor's completion count.
    pub fn get_mentor_completion_count(env: Env, mentor: Address) -> u32 {
        let completions = storage::get_completions(&env, &mentor);
        completions.len() as u32
    }

    /// Get mentor's completion records (for authorized queries).
    pub fn get_mentor_completions(
        env: Env,
        mentor: Address,
        limit: u32,
    ) -> Result<Vec<CompletionRecord>, MentorError> {
        let max_limit = 50u32;
        let actual_limit = if limit > max_limit { max_limit } else { limit };

        let completions = storage::get_completions(&env, &mentor);
        let result: Vec<CompletionRecord> = completions
            .iter()
            .rev() // Most recent first
            .take(actual_limit as usize)
            .cloned()
            .collect();

        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Mentor Comparison
    // -----------------------------------------------------------------------

    /// Compare multiple mentors across key dimensions.
    pub fn compare_mentors(
        env: Env,
        mentor_ids: Vec<Address>,
        program_id: Option<u32>,
    ) -> Result<Vec<MentorComparison>, MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        let mut comparisons = Vec::new();

        for mentor_id in mentor_ids.iter() {
            let profile = match storage::get_mentor(&env, mentor_id) {
                Some(p) => p,
                None => continue, // Skip missing mentors
            };

            let avg_rating = Self::get_mentor_avg_rating(env.clone(), mentor_id.clone())
                .unwrap_or(0);
            let completion_count = Self::get_mentor_completion_count(env.clone(), mentor_id.clone());

            // Calculate fit score (simplified for now)
            let fit_score = if let Some(_pid) = program_id {
                // TODO: Calculate fit based on program constraints
                50u8
            } else {
                0u8
            };

            let comparison = MentorComparison {
                mentor_id: mentor_id.clone(),
                price: profile.base_price,
                avg_rating,
                completion_count,
                program_fit_score: fit_score,
                availability_match: true, // TODO: Check against program/caller needs
                mode_match: true,          // TODO: Check against program mode
            };

            comparisons.push(comparison);
        }

        Ok(comparisons)
    }

    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Grant authorization to a contract to record completions.
    pub fn authorize_program(
        env: Env,
        admin: Address,
        program_contract: Address,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::add_authorized_program(&env, program_contract.clone());

        env.events()
            .publish((symbol_short!("auth_prog"),), program_contract);

        Ok(())
    }

    /// Remove authorization from a contract.
    pub fn revoke_program_authorization(
        env: Env,
        admin: Address,
        program_contract: Address,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::remove_authorized_program(&env, program_contract.clone());

        env.events()
            .publish((symbol_short!("revoke_prog"),), program_contract);

        Ok(())
    }
}
