#![no_std]

mod errors;
mod storage;
mod types;

pub use errors::MentorError;
pub use types::*;

use mentor_types::{AbuseSignals, MentorPublicProjection, PriceBucket};
use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, BytesN, Env, String,
};
use storage::{MAX_TTL, MIN_TTL};

#[contract]
pub struct MentorRateLimit;

// Rate limit types (used as identifiers)
pub const RATE_LIMIT_DIRECTORY_QUERY: u8 = 0;
pub const RATE_LIMIT_PROFILE_ENUM: u8 = 1;
pub const RATE_LIMIT_PROGRAM_DISCOVERY: u8 = 2;
pub const RATE_LIMIT_COMPARISON_QUERY: u8 = 3;

// Default rate limit values (requests per window)
pub const DEFAULT_DIRECTORY_MAX: u32 = 10;
pub const DEFAULT_DIRECTORY_WINDOW: u32 = 60;

pub const DEFAULT_ENUM_MAX: u32 = 20;
pub const DEFAULT_ENUM_WINDOW: u32 = 60;

pub const DEFAULT_PROGRAM_MAX: u32 = 15;
pub const DEFAULT_PROGRAM_WINDOW: u32 = 60;

pub const DEFAULT_COMPARE_MAX: u32 = 25;
pub const DEFAULT_COMPARE_WINDOW: u32 = 60;

#[contractimpl]
impl MentorRateLimit {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the rate limiting contract.
    pub fn init(env: Env, admin: Address) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if storage::get_admin(&env).is_some() {
            return Err(MentorError::AlreadyInitialized);
        }

        admin.require_auth();
        storage::set_admin(&env, &admin);

        // Initialize default rate limits
        storage::set_rate_limit(&env, RATE_LIMIT_DIRECTORY_QUERY, DEFAULT_DIRECTORY_MAX, DEFAULT_DIRECTORY_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_PROFILE_ENUM, DEFAULT_ENUM_MAX, DEFAULT_ENUM_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_PROGRAM_DISCOVERY, DEFAULT_PROGRAM_MAX, DEFAULT_PROGRAM_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_COMPARISON_QUERY, DEFAULT_COMPARE_MAX, DEFAULT_COMPARE_WINDOW);

        env.events()
            .publish((symbol_short!("admin_set"),), admin);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rate Limit Checking
    // -----------------------------------------------------------------------

    /// Check if an account has exceeded rate limits for an operation.
    pub fn check_rate_limit(
        env: Env,
        account: Address,
        limit_type: u8,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if limit_type > 3 {
            return Err(MentorError::InvalidInput);
        }

        let (max_requests, window_seconds) = storage::get_rate_limit(&env, limit_type);

        let now = env.ledger().timestamp();
        let window_start = now.saturating_sub(window_seconds as u64);

        let current_count = storage::get_request_count(&env, &account, limit_type, window_start);

        if current_count >= max_requests {
            // Publish abuse signal
            env.events().publish(
                (symbol_short!("rate_limit_exc"),),
                (account.clone(), limit_type),
            );
            return Err(MentorError::RateLimitExceeded);
        }

        // Increment counter
        storage::increment_request_count(&env, &account, limit_type, now);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rate Limit Configuration
    // -----------------------------------------------------------------------

    /// Set rate limit for an operation type (admin only).
    pub fn set_rate_limit(
        env: Env,
        admin: Address,
        limit_type: u8,
        max_requests: u32,
        window_seconds: u32,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        if limit_type > 3 || max_requests == 0 || window_seconds == 0 {
            return Err(MentorError::InvalidInput);
        }

        storage::set_rate_limit(&env, limit_type, max_requests, window_seconds);

        env.events().publish(
            (symbol_short!("limit_set"),),
            (limit_type, max_requests, window_seconds),
        );

        Ok(())
    }

    /// Get current rate limit configuration.
    pub fn get_rate_limit_config(env: Env, limit_type: u8) -> Result<(u32, u32), MentorError> {
        if limit_type > 3 {
            return Err(MentorError::InvalidInput);
        }

        Ok(storage::get_rate_limit(&env, limit_type))
    }

    // -----------------------------------------------------------------------
    // Anti-Enumeration: Public Projections
    // -----------------------------------------------------------------------

    /// Get a mentor's public projection (limited fields for anti-enumeration).
    pub fn get_mentor_projection(
        env: Env,
        mentor_id: Address,
        program_id: Option<u32>,
    ) -> MentorPublicProjection {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        // TODO: Query registry contract for mentor data
        // For now, return safe projection
        MentorPublicProjection {
            mentor_id: mentor_id.clone(),
            availability_match: false,
            price_bucket: PriceBucket::Medium,
            has_verified_badge: false,
            completion_count_range: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Abuse Signal Detection
    // -----------------------------------------------------------------------

    /// Get abuse signals for an account.
    pub fn get_abuse_signals(env: Env, account: Address) -> AbuseSignals {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        storage::get_abuse_signals(&env, &account)
    }

    /// Record abuse signal for account monitoring.
    pub fn record_abuse_signal(
        env: Env,
        account: Address,
        signal_type: u8,
    ) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        if signal_type > 2 {
            return Err(MentorError::InvalidInput);
        }

        storage::increment_abuse_signal(&env, &account, signal_type);

        env.events().publish(
            (symbol_short!("abuse_sig"),),
            (account.clone(), signal_type),
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Metrics
    // -----------------------------------------------------------------------

    /// Get request count for account in rate limit window.
    pub fn get_request_count(
        env: Env,
        account: Address,
        limit_type: u8,
        window_start: u64,
    ) -> Result<u32, MentorError> {
        if limit_type > 3 {
            return Err(MentorError::InvalidInput);
        }

        Ok(storage::get_request_count(&env, &account, limit_type, window_start))
    }

    // -----------------------------------------------------------------------
    // Admin Functions
    // -----------------------------------------------------------------------

    /// Reset rate limits to defaults.
    pub fn reset_rate_limits(env: Env, admin: Address) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::set_rate_limit(&env, RATE_LIMIT_DIRECTORY_QUERY, DEFAULT_DIRECTORY_MAX, DEFAULT_DIRECTORY_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_PROFILE_ENUM, DEFAULT_ENUM_MAX, DEFAULT_ENUM_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_PROGRAM_DISCOVERY, DEFAULT_PROGRAM_MAX, DEFAULT_PROGRAM_WINDOW);
        storage::set_rate_limit(&env, RATE_LIMIT_COMPARISON_QUERY, DEFAULT_COMPARE_MAX, DEFAULT_COMPARE_WINDOW);

        env.events()
            .publish((symbol_short!("limits_reset"),), ());

        Ok(())
    }

    /// Clear abuse signals for an account.
    pub fn clear_abuse_signals(env: Env, admin: Address, account: Address) -> Result<(), MentorError> {
        env.storage()
            .instance()
            .extend_ttl(MIN_TTL, MAX_TTL);

        admin.require_auth();
        storage::require_admin(&env, &admin)?;

        storage::clear_abuse_signals(&env, &account);

        env.events()
            .publish((symbol_short!("abuse_cleared"),), account);

        Ok(())
    }
}
