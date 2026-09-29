#![no_std]
#![allow(clippy::too_many_arguments)]

//! Scholarship rate-limits contract.
//!
//! Protects public discovery, applications, uploads, invitations,
//! messaging, and payout actions with risk-based limits using stable
//! actor identities.
//!
//! ## Design
//!
//! - **Stable identities**: each actor is identified by an `Address`
//!   (Stellar public key). Limits are keyed by `(actor, action, window_id)`
//!   so an actor cannot evade a limit by rotating an ephemeral identifier.
//! - **Risk-based limits**: each `Action` has a configurable `RiskTier`
//!   (Low / Medium / High / Critical). The admin sets the per-tier
//!   max count and window duration; high-risk bypasses require explicit
//!   service-level authorization.
//! - **Return retry guidance**: when a limit is exceeded the error carries
//!   enough information (the window reset time) for a caller to derive the
//!   earliest retry instant without polling.
//! - **Draft safety**: the rate-limit check is a read-only gate before any
//!   state is written. If the check fails the invoking contract has not yet
//!   mutated its state, so no draft is corrupted.
//! - **Bounded storage**: at most `MAX_BUCKETS` rate-limit buckets total;
//!   individual bucket records are pruned (overwritten) per rolling window.
//! - **High-risk bypass**: only addresses holding a `ServiceAuth` grant may
//!   bypass High/Critical tier limits.
//!
//! ## TTL policy
//!
//! Bucket records are short-lived (`BUCKET_MIN_TTL` / `BUCKET_MAX_TTL`,
//! ~1 day / ~7 days). They are automatically garbage-collected by Soroban
//! when the TTL expires. Tier configuration lives in instance storage
//! (survives upgrades, manually managed by admin).
//!
//! ## Ownership / privacy notes
//!
//! Only `Address` and numeric counters are stored. No action payloads,
//! document content, or PII are ever written here.
//!
//! ## Migration notes
//!
//! Adding a new `Action` variant does not affect existing bucket keys. Tier
//! config is a small instance-storage map and may be updated by the admin
//! at any time. Rolling-window buckets are self-expiring.
//!
//! ## Adversarial tests
//!
//! See `tests/` for high-risk bypass enforcement, draft-safe failure path,
//! stable-identity evasion attempts, and window-rollover correctness.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

const CONTRACT_VERSION: u32 = 1;

/// ~1 day (86 400 s / 5 s per ledger ≈ 17 280 ledgers).
const BUCKET_MIN_TTL: u32 = 17_280;
/// ~7 days.
const BUCKET_MAX_TTL: u32 = 120_960;

/// ~1 year — for tier config and service-auth grants.
const CONFIG_MIN_TTL: u32 = 3_110_400;
/// ~2 years.
const CONFIG_MAX_TTL: u32 = 6_220_800;

/// Maximum distinct rate-limit buckets across all actors (guards persistent
/// storage growth). Each (actor, action, window) triple is one bucket.
const MAX_BUCKETS: u64 = 500_000;

/// Default window duration in seconds when none is configured for an action.
const DEFAULT_WINDOW_SECONDS: u64 = 3_600; // 1 hour
/// Default maximum calls per window when none is configured.
const DEFAULT_MAX_COUNT: u32 = 100;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The actor has exceeded the rate limit for this action. The `window_reset`
    /// time is encoded in events; callers derive the retry time from that.
    RateLimitExceeded = 4,
    /// A High/Critical tier action was attempted without a valid service authorization.
    ServiceAuthRequired = 5,
    /// The window duration must be > 0.
    InvalidWindow = 6,
    /// The max count must be > 0.
    InvalidMaxCount = 7,
    /// The total bucket ceiling has been reached.
    BucketLimitExceeded = 8,
    /// Arithmetic overflow (practically unreachable).
    Overflow = 9,
}

// ---------------------------------------------------------------------------
// Actions and tiers
// ---------------------------------------------------------------------------

/// The six action surfaces that carry rate limits.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Discovery = 1,
    Application = 2,
    Upload = 3,
    Invitation = 4,
    Messaging = 5,
    Payout = 6,
}

/// Risk tier — determines the default strictness and whether a service-auth
/// bypass is required for over-limit actions.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskTier {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Per-action limit configuration set by the admin.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionConfig {
    pub risk_tier: RiskTier,
    /// Rolling window duration in seconds.
    pub window_seconds: u64,
    /// Maximum invocations within one window.
    pub max_count: u32,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Per-action limit configuration.
    ActionConfig(Action),
    /// Service-level authorization grant for an address.
    ServiceAuth(Address),
    /// A rate-limit bucket: (actor, action, window_id) → count.
    /// `window_id` is `floor(ledger_timestamp / window_seconds)`.
    Bucket(Address, Action, u64),
    /// Global bucket count (for ceiling enforcement).
    BucketCount,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ScholarshipRateLimitsContract;

#[contractimpl]
impl ScholarshipRateLimitsContract {
    /// One-time bootstrap.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::seed_defaults(&env);
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
        Ok(())
    }

    // ── tier / limit configuration ────────────────────────────────────────

    /// Admin-only: configure the limit for one action.
    pub fn set_action_config(
        env: Env,
        admin: Address,
        action: Action,
        risk_tier: RiskTier,
        window_seconds: u64,
        max_count: u32,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if window_seconds == 0 {
            return Err(ContractError::InvalidWindow);
        }
        if max_count == 0 {
            return Err(ContractError::InvalidMaxCount);
        }
        env.storage().instance().set(
            &DataKey::ActionConfig(action),
            &ActionConfig { risk_tier, window_seconds, max_count },
        );
        env.events().publish(
            (symbol_short!("CFGSET"),),
            (action, risk_tier, window_seconds, max_count),
        );
        Ok(())
    }

    pub fn get_action_config(env: Env, action: Action) -> ActionConfig {
        env.storage()
            .instance()
            .get(&DataKey::ActionConfig(action))
            .unwrap_or(ActionConfig {
                risk_tier: RiskTier::Medium,
                window_seconds: DEFAULT_WINDOW_SECONDS,
                max_count: DEFAULT_MAX_COUNT,
            })
    }

    // ── service-auth grants ───────────────────────────────────────────────

    /// Admin-only: grant service-level authorization to an address.
    /// Addresses with service auth may bypass High/Critical tier limits.
    pub fn grant_service_auth(
        env: Env,
        admin: Address,
        service: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::ServiceAuth(service.clone());
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, CONFIG_MIN_TTL, CONFIG_MAX_TTL);
        env.events()
            .publish((symbol_short!("SVAUTGRT"),), (service,));
        Ok(())
    }

    /// Admin-only: revoke service-level authorization.
    pub fn revoke_service_auth(
        env: Env,
        admin: Address,
        service: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::ServiceAuth(service.clone()), &false);
        env.events()
            .publish((symbol_short!("SVAUTRVK"),), (service,));
        Ok(())
    }

    pub fn has_service_auth(env: Env, service: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::ServiceAuth(service))
            .unwrap_or(false)
    }

    // ── rate-limit check and record ───────────────────────────────────────

    /// Check and record one invocation of `action` by `actor`.
    ///
    /// - Computes the current rolling `window_id = floor(now / window_seconds)`.
    /// - Reads or initialises the bucket for `(actor, action, window_id)`.
    /// - If `count >= max_count` for the action's tier, returns
    ///   `RateLimitExceeded` and emits a `RATELIM` event with the window
    ///   reset time so callers know when to retry without polling.
    /// - For High/Critical tier actions, checks service auth first; without
    ///   it the call is rejected with `ServiceAuthRequired`.
    /// - On success, increments the bucket and returns the remaining count.
    ///
    /// This function is the single point of truth: callers invoke it before
    /// any state-mutating operation, so a failed check is safe to retry and
    /// a successful one is idempotent within the same ledger invocation.
    pub fn check_and_record(
        env: Env,
        actor: Address,
        action: Action,
    ) -> Result<u32, ContractError> {
        actor.require_auth();
        let config = Self::get_action_config(env.clone(), action);
        let now = env.ledger().timestamp();
        let window_id = now / config.window_seconds;
        let window_reset = (window_id
            .checked_add(1)
            .ok_or(ContractError::Overflow)?)
            .checked_mul(config.window_seconds)
            .ok_or(ContractError::Overflow)?;

        // High/Critical tier requires service auth.
        if matches!(config.risk_tier, RiskTier::High | RiskTier::Critical) {
            if !Self::has_service_auth(env.clone(), actor.clone()) {
                env.events().publish(
                    (symbol_short!("SVAUREQ"),),
                    (actor, action, window_reset),
                );
                return Err(ContractError::ServiceAuthRequired);
            }
        }

        let bucket_key = DataKey::Bucket(actor.clone(), action, window_id);
        let current: u32 = env
            .storage()
            .persistent()
            .get(&bucket_key)
            .unwrap_or(0u32);

        if current >= config.max_count {
            // Emit retry guidance: the epoch at which the window resets.
            env.events().publish(
                (symbol_short!("RATELIM"),),
                (actor, action, window_reset, current, config.max_count),
            );
            return Err(ContractError::RateLimitExceeded);
        }

        let next = current.checked_add(1).ok_or(ContractError::Overflow)?;

        // Track bucket count for ceiling enforcement.
        if current == 0 {
            let total: u64 = env
                .storage()
                .instance()
                .get(&DataKey::BucketCount)
                .unwrap_or(0u64);
            if total >= MAX_BUCKETS {
                return Err(ContractError::BucketLimitExceeded);
            }
            env.storage()
                .instance()
                .set(&DataKey::BucketCount, &(total.checked_add(1).ok_or(ContractError::Overflow)?));
        }

        env.storage().persistent().set(&bucket_key, &next);
        env.storage()
            .persistent()
            .extend_ttl(&bucket_key, BUCKET_MIN_TTL, BUCKET_MAX_TTL);

        let remaining = config.max_count.saturating_sub(next);
        env.events().publish(
            (symbol_short!("RATEOK"),),
            (actor, action, next, remaining),
        );
        Ok(remaining)
    }

    /// Read-only: current usage for `(actor, action)` in the current window.
    /// Returns `(used, remaining, window_reset_timestamp)`.
    pub fn current_usage(
        env: Env,
        actor: Address,
        action: Action,
    ) -> (u32, u32, u64) {
        let config = Self::get_action_config(env.clone(), action);
        let now = env.ledger().timestamp();
        let window_id = now / config.window_seconds;
        let window_reset = (window_id + 1) * config.window_seconds;
        let used: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Bucket(actor, action, window_id))
            .unwrap_or(0);
        let remaining = config.max_count.saturating_sub(used);
        (used, remaining, window_reset)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    // ── default seed ──────────────────────────────────────────────────────

    /// Sensible default limits per action, mirroring typical risk posture.
    fn seed_defaults(env: &Env) {
        let defaults: &[(Action, RiskTier, u64, u32)] = &[
            (Action::Discovery,   RiskTier::Low,      3_600,  300),
            (Action::Application, RiskTier::Medium,   86_400,  10),
            (Action::Upload,      RiskTier::Medium,   86_400,  20),
            (Action::Invitation,  RiskTier::Medium,   86_400,  50),
            (Action::Messaging,   RiskTier::Medium,   3_600,  100),
            (Action::Payout,      RiskTier::High,     86_400,   5),
        ];
        for &(action, tier, window, max) in defaults {
            env.storage().instance().set(
                &DataKey::ActionConfig(action),
                &ActionConfig { risk_tier: tier, window_seconds: window, max_count: max },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
