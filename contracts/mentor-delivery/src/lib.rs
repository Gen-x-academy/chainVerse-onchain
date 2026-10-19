#![no_std]

//! Versioned delivery-mode and safety commitments for mentorship programs
//! (MENTOR-020).
//!
//! Program owners configure remote, in-person, or hybrid delivery. Venue
//! details and safety-plan contents stay off-chain; the contract stores only
//! salted commitments, coarse visibility policy, a safety attestation, and
//! version metadata. Visibility is a signal for the off-chain service to
//! enforce, not on-chain access control.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
pub const STORAGE_VERSION: u32 = 1;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyConfigured = 1,
    ProgramNotFound = 2,
    InvalidIdentifier = 3,
    InvalidCommitment = 4,
    InvalidSchemaVersion = 5,
    InvalidModeConfiguration = 6,
    VersionOverflow = 7,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config(BytesN<32>),
    VersionCount(BytesN<32>),
    Revision(BytesN<32>, u32),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    Remote,
    InPerson,
    Hybrid,
}

/// How much location information the off-chain service is permitted to show.
/// `PublicArea` means a coarse area only; it never authorizes publication of
/// a precise venue or participant location.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationVisibility {
    NotApplicable,
    Hidden,
    MatchedOnly,
    PublicArea,
}

/// Current delivery settings for a program. Venue and safety-plan contents
/// are represented only by commitments to off-chain data.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryConfig {
    pub program_id: BytesN<32>,
    pub owner: Address,
    pub version: u32,
    pub mode: DeliveryMode,
    pub location_visibility: LocationVisibility,
    pub location_commitment: Option<BytesN<32>>,
    pub safety_policy_hash: BytesN<32>,
    /// Owner attests that mode-appropriate physical safety arrangements
    /// described by the committed off-chain policy are in place.
    pub physical_safety_confirmed: bool,
    pub schema_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Immutable configuration snapshot for an indexer or later audit.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryRevision {
    pub program_id: BytesN<32>,
    pub owner: Address,
    pub version: u32,
    pub mode: DeliveryMode,
    pub location_visibility: LocationVisibility,
    pub location_commitment: Option<BytesN<32>>,
    pub safety_policy_hash: BytesN<32>,
    pub physical_safety_confirmed: bool,
    pub schema_version: u32,
    pub updated_at: u64,
}

#[contract]
pub struct MentorDeliveryContract;

#[contractimpl]
impl MentorDeliveryContract {
    /// Create delivery configuration for a program. The owner must sign.
    pub fn configure_program(
        env: Env,
        owner: Address,
        program_id: BytesN<32>,
        mode: DeliveryMode,
        location_visibility: LocationVisibility,
        location_commitment: Option<BytesN<32>>,
        safety_policy_hash: BytesN<32>,
        physical_safety_confirmed: bool,
        schema_version: u32,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        Self::validate(
            &env,
            &program_id,
            mode,
            location_visibility,
            &location_commitment,
            &safety_policy_hash,
            physical_safety_confirmed,
            schema_version,
        )?;
        let key = DataKey::Config(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyConfigured);
        }
        let version = Self::next_version(&env, &program_id)?;
        let now = env.ledger().timestamp();
        let config = DeliveryConfig {
            program_id: program_id.clone(),
            owner: owner.clone(),
            version,
            mode,
            location_visibility,
            location_commitment,
            safety_policy_hash,
            physical_safety_confirmed,
            schema_version,
            created_at: now,
            updated_at: now,
        };
        Self::write_config(&env, &key, &config);
        Self::write_revision(&env, &config, now);
        Self::publish_event(&env, symbol_short!("MDNEW"), &config);
        Ok(version)
    }

    /// Replace a program's delivery settings. Only its original owner may
    /// update the configuration; updates preserve the original creation time.
    pub fn update_program(
        env: Env,
        owner: Address,
        program_id: BytesN<32>,
        mode: DeliveryMode,
        location_visibility: LocationVisibility,
        location_commitment: Option<BytesN<32>>,
        safety_policy_hash: BytesN<32>,
        physical_safety_confirmed: bool,
        schema_version: u32,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        Self::validate(
            &env,
            &program_id,
            mode,
            location_visibility,
            &location_commitment,
            &safety_policy_hash,
            physical_safety_confirmed,
            schema_version,
        )?;
        let key = DataKey::Config(program_id.clone());
        let mut config: DeliveryConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        if config.owner != owner {
            return Err(ContractError::ProgramNotFound);
        }
        let version = Self::next_version(&env, &program_id)?;
        let now = env.ledger().timestamp();
        config.version = version;
        config.mode = mode;
        config.location_visibility = location_visibility;
        config.location_commitment = location_commitment;
        config.safety_policy_hash = safety_policy_hash;
        config.physical_safety_confirmed = physical_safety_confirmed;
        config.schema_version = schema_version;
        config.updated_at = now;
        Self::write_config(&env, &key, &config);
        Self::write_revision(&env, &config, now);
        Self::publish_event(&env, symbol_short!("MDUPD"), &config);
        Ok(version)
    }

    pub fn get_config(env: Env, program_id: BytesN<32>) -> Result<DeliveryConfig, ContractError> {
        let key = DataKey::Config(program_id.clone());
        let config: DeliveryConfig = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::touch_version_count(&env, &program_id);
        Ok(config)
    }

    pub fn get_revision(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<DeliveryRevision, ContractError> {
        if version == 0 {
            return Err(ContractError::ProgramNotFound);
        }
        let key = DataKey::Revision(program_id.clone(), version);
        let revision: DeliveryRevision = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::touch_version_count(&env, &program_id);
        Ok(revision)
    }

    fn validate(
        env: &Env,
        program_id: &BytesN<32>,
        mode: DeliveryMode,
        location_visibility: LocationVisibility,
        location_commitment: &Option<BytesN<32>>,
        safety_policy_hash: &BytesN<32>,
        physical_safety_confirmed: bool,
        schema_version: u32,
    ) -> Result<(), ContractError> {
        let zero = BytesN::from_array(env, &[0u8; 32]);
        if *program_id == zero {
            return Err(ContractError::InvalidIdentifier);
        }
        if *safety_policy_hash == zero
            || location_commitment.as_ref().is_some_and(|hash| *hash == zero)
        {
            return Err(ContractError::InvalidCommitment);
        }
        if schema_version == 0 {
            return Err(ContractError::InvalidSchemaVersion);
        }

        let mode_is_physical = matches!(mode, DeliveryMode::InPerson | DeliveryMode::Hybrid);
        let valid_mode_configuration = if mode_is_physical {
            location_visibility != LocationVisibility::NotApplicable
                && location_commitment.is_some()
                && physical_safety_confirmed
        } else {
            location_visibility == LocationVisibility::NotApplicable
                && location_commitment.is_none()
                && !physical_safety_confirmed
        };
        if !valid_mode_configuration {
            return Err(ContractError::InvalidModeConfiguration);
        }
        Ok(())
    }

    fn next_version(env: &Env, program_id: &BytesN<32>) -> Result<u32, ContractError> {
        let key = DataKey::VersionCount(program_id.clone());
        let current: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        let next = current.checked_add(1).ok_or(ContractError::VersionOverflow)?;
        env.storage().persistent().set(&key, &next);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(next)
    }

    fn write_config(env: &Env, key: &DataKey, config: &DeliveryConfig) {
        env.storage().persistent().set(key, config);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_revision(env: &Env, config: &DeliveryConfig, updated_at: u64) {
        let key = DataKey::Revision(config.program_id.clone(), config.version);
        let revision = DeliveryRevision {
            program_id: config.program_id.clone(),
            owner: config.owner.clone(),
            version: config.version,
            mode: config.mode,
            location_visibility: config.location_visibility,
            location_commitment: config.location_commitment.clone(),
            safety_policy_hash: config.safety_policy_hash.clone(),
            physical_safety_confirmed: config.physical_safety_confirmed,
            schema_version: config.schema_version,
            updated_at,
        };
        env.storage().persistent().set(&key, &revision);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn touch_version_count(env: &Env, program_id: &BytesN<32>) {
        let key = DataKey::VersionCount(program_id.clone());
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
    }

    fn publish_event(env: &Env, topic: soroban_sdk::Symbol, config: &DeliveryConfig) {
        env.events().publish(
            (topic,),
            (
                config.program_id.clone(),
                config.owner.clone(),
                config.version,
                config.mode,
                config.location_visibility,
                config.location_commitment.clone(),
                config.safety_policy_hash.clone(),
                config.physical_safety_confirmed,
                config.schema_version,
            ),
        );
    }
}

#[cfg(test)]
mod tests;