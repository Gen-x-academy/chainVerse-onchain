#![no_std]

//! Environment-scoped, privacy-minimized mentorship capability settings.
//!
//! Feature flags are rollout controls, not authorization. Calling contracts
//! must enforce participant roles and settlement policy separately. Session
//! evidence remains off-chain; this contract stores only public rules and
//! currency contract addresses.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 15_552_000;
const RECORD_MAX_TTL: u32 = 31_104_000;
const MAX_CURRENCIES: u32 = 8;
const MAX_SESSION_DURATIONS: u32 = 8;
const MIN_SESSION_MINUTES: u32 = 15;
const MAX_SESSION_MINUTES: u32 = 240;
const MAX_ACTIVE_ENGAGEMENTS: u32 = 100;
const MAX_SESSIONS_PER_ENGAGEMENT: u32 = 100;
const MAX_CANCELLATION_WINDOW_SECONDS: u64 = 2_592_000;
const MAX_EVIDENCE_REFERENCES: u32 = 8;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    InvalidLimits = 4,
    TooManyCurrencies = 5,
    DuplicateCurrency = 6,
    CurrencyRequired = 7,
    TooManySessionDurations = 8,
    InvalidSessionDuration = 9,
    DuplicateSessionDuration = 10,
    SessionDurationRequired = 11,
    InvalidCancellationWindow = 12,
    InvalidEvidenceRules = 13,
    VersionOverflow = 14,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Environment {
    Development = 1,
    Staging = 2,
    Production = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRules {
    /// Minimum number of opaque evidence references required by the workflow.
    pub minimum_references: u32,
    pub require_tutor_attestation: bool,
    pub require_student_confirmation: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipSettings {
    /// Disabled by default in every environment.
    pub enabled: bool,
    pub max_active_engagements_per_participant: u32,
    pub max_sessions_per_engagement: u32,
    /// Soroban token contract addresses; symbols/tickers are not trusted identifiers.
    pub supported_currencies: Vec<Address>,
    /// Allowed session lengths, in whole minutes.
    pub session_durations_minutes: Vec<u32>,
    pub cancellation_window_seconds: u64,
    pub evidence_rules: EvidenceRules,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigSnapshot {
    pub settings: MentorshipSettings,
    pub version: u32,
    pub updated_at: u64,
    pub updated_by: Option<Address>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Config(Environment),
}

#[contract]
pub struct MentorshipConfigContract;

#[contractimpl]
impl MentorshipConfigContract {
    /// Initialize the configuration administrator. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only update for one environment. Each successful update creates
    /// a new version and emits its public settings for audit/indexing.
    pub fn set_config(
        env: Env,
        admin: Address,
        environment: Environment,
        settings: MentorshipSettings,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_settings(&settings)?;

        let key = DataKey::Config(environment);
        let current: Option<ConfigSnapshot> = env.storage().persistent().get(&key);
        let version = current
            .map(|snapshot| snapshot.version)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let snapshot = ConfigSnapshot {
            settings,
            version,
            updated_at: env.ledger().timestamp(),
            updated_by: Some(admin),
        };

        env.storage().persistent().set(&key, &snapshot);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("CFGSET"),),
            (environment, snapshot.clone()),
        );
        Ok(version)
    }

    /// Read a configuration. An unconfigured environment returns a safe,
    /// disabled default rather than enabling mentorship implicitly.
    pub fn get_config(env: Env, environment: Environment) -> ConfigSnapshot {
        let key = DataKey::Config(environment);
        if let Some(snapshot) = env.storage().persistent().get::<_, ConfigSnapshot>(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            snapshot
        } else {
            Self::safe_default(&env)
        }
    }

    /// Convenience feature-flag read. This is not an authorization check.
    pub fn is_enabled(env: Env, environment: Environment) -> bool {
        Self::get_config(env, environment).settings.enabled
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &admin != caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn safe_default(env: &Env) -> ConfigSnapshot {
        ConfigSnapshot {
            settings: MentorshipSettings {
                enabled: false,
                max_active_engagements_per_participant: 1,
                max_sessions_per_engagement: 1,
                supported_currencies: Vec::new(env),
                session_durations_minutes: Vec::new(env),
                cancellation_window_seconds: 86_400,
                evidence_rules: EvidenceRules {
                    minimum_references: 1,
                    require_tutor_attestation: true,
                    require_student_confirmation: true,
                },
            },
            version: 0,
            updated_at: 0,
            updated_by: None,
        }
    }

    fn validate_settings(settings: &MentorshipSettings) -> Result<(), ContractError> {
        if settings.max_active_engagements_per_participant == 0
            || settings.max_active_engagements_per_participant > MAX_ACTIVE_ENGAGEMENTS
            || settings.max_sessions_per_engagement == 0
            || settings.max_sessions_per_engagement > MAX_SESSIONS_PER_ENGAGEMENT
        {
            return Err(ContractError::InvalidLimits);
        }
        if settings.supported_currencies.len() > MAX_CURRENCIES {
            return Err(ContractError::TooManyCurrencies);
        }
        if settings.enabled && settings.supported_currencies.is_empty() {
            return Err(ContractError::CurrencyRequired);
        }
        for index in 0..settings.supported_currencies.len() {
            let currency = settings.supported_currencies.get(index).unwrap();
            for prior_index in 0..index {
                if settings.supported_currencies.get(prior_index).unwrap() == currency {
                    return Err(ContractError::DuplicateCurrency);
                }
            }
        }

        if settings.session_durations_minutes.len() > MAX_SESSION_DURATIONS {
            return Err(ContractError::TooManySessionDurations);
        }
        if settings.enabled && settings.session_durations_minutes.is_empty() {
            return Err(ContractError::SessionDurationRequired);
        }
        for index in 0..settings.session_durations_minutes.len() {
            let duration = settings.session_durations_minutes.get(index).unwrap();
            if duration < MIN_SESSION_MINUTES || duration > MAX_SESSION_MINUTES {
                return Err(ContractError::InvalidSessionDuration);
            }
            for prior_index in 0..index {
                if settings.session_durations_minutes.get(prior_index).unwrap() == duration {
                    return Err(ContractError::DuplicateSessionDuration);
                }
            }
        }

        if settings.cancellation_window_seconds > MAX_CANCELLATION_WINDOW_SECONDS {
            return Err(ContractError::InvalidCancellationWindow);
        }
        if settings.evidence_rules.minimum_references > MAX_EVIDENCE_REFERENCES
            || (settings.enabled
                && (settings.evidence_rules.minimum_references == 0
                    || !settings.evidence_rules.require_tutor_attestation))
        {
            return Err(ContractError::InvalidEvidenceRules);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;