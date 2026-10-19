#![no_std]

//! Privacy-minimized, owner-controlled professional profile commitments
//! (MENTOR-008).
//!
//! The profile manifest lives off-chain and may contain a headline,
//! biography, expertise IDs, languages, time zone, experience, teaching
//! approach, and links. This contract stores only a salted commitment to the
//! manifest, its schema version, profile visibility/status, and revision
//! metadata. Visibility is an off-chain policy signal, not confidentiality:
//! contract state and invocation arguments are public.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
pub const STORAGE_VERSION: u32 = 1;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyExists = 1,
    ProfileNotFound = 2,
    InvalidCommitment = 3,
    InvalidSchemaVersion = 4,
    VersionOverflow = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Profile(Address),
    ProfileVersionCount(Address),
    ProfileRevision(Address, u32),
}

/// Desired off-chain discovery visibility. This does not make the manifest
/// private on-chain; callers must enforce it in the profile service.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileVisibility {
    Private,
    Matched,
    Public,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStatus {
    Active,
    Deactivated,
}

/// Latest public profile commitment for a mentor. The owner address is the
/// profile key and the only identity field stored by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorProfile {
    pub owner: Address,
    pub version: u32,
    pub profile_commitment: BytesN<32>,
    pub schema_version: u32,
    pub visibility: ProfileVisibility,
    pub status: ProfileStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Immutable snapshot of profile metadata at one revision. The manifest
/// itself is never stored here.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileRevision {
    pub owner: Address,
    pub version: u32,
    pub profile_commitment: BytesN<32>,
    pub schema_version: u32,
    pub visibility: ProfileVisibility,
    pub status: ProfileStatus,
    pub updated_at: u64,
}

#[contract]
pub struct MentorProfilesContract;

#[contractimpl]
impl MentorProfilesContract {
    /// Create the caller's profile; one active record exists per owner.
    pub fn create_profile(
        env: Env,
        owner: Address,
        profile_commitment: BytesN<32>,
        schema_version: u32,
        visibility: ProfileVisibility,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        let profile_key = DataKey::Profile(owner.clone());
        if env.storage().persistent().has(&profile_key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::validate_profile(&env, &profile_commitment, schema_version)?;
        let version = Self::next_version(&env, &owner)?;
        let now = env.ledger().timestamp();
        let profile = MentorProfile {
            owner: owner.clone(),
            version,
            profile_commitment: profile_commitment.clone(),
            schema_version,
            visibility,
            status: ProfileStatus::Active,
            created_at: now,
            updated_at: now,
        };
        Self::write_profile(&env, &profile_key, &profile);
        Self::write_revision(&env, &profile, now);
        env.events().publish(
            (symbol_short!("MPRNEW"),),
            (owner, version, profile_commitment, schema_version, visibility),
        );
        Ok(version)
    }

    /// Replace the off-chain profile manifest commitment and visibility.
    pub fn update_profile(
        env: Env,
        owner: Address,
        profile_commitment: BytesN<32>,
        schema_version: u32,
        visibility: ProfileVisibility,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        Self::validate_profile(&env, &profile_commitment, schema_version)?;
        let profile_key = DataKey::Profile(owner.clone());
        let mut profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&profile_key)
            .ok_or(ContractError::ProfileNotFound)?;
        let version = Self::next_version(&env, &owner)?;
        let now = env.ledger().timestamp();
        profile.version = version;
        profile.profile_commitment = profile_commitment.clone();
        profile.schema_version = schema_version;
        profile.visibility = visibility;
        profile.updated_at = now;
        Self::write_profile(&env, &profile_key, &profile);
        Self::write_revision(&env, &profile, now);
        env.events().publish(
            (symbol_short!("MPRUPD"),),
            (owner, version, profile_commitment, schema_version, visibility),
        );
        Ok(version)
    }

    /// Change visibility without replacing the profile manifest.
    pub fn set_visibility(
        env: Env,
        owner: Address,
        visibility: ProfileVisibility,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        let profile_key = DataKey::Profile(owner.clone());
        let mut profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&profile_key)
            .ok_or(ContractError::ProfileNotFound)?;
        if profile.visibility == visibility {
            return Ok(profile.version);
        }
        let version = Self::next_version(&env, &owner)?;
        let now = env.ledger().timestamp();
        profile.version = version;
        profile.visibility = visibility;
        profile.updated_at = now;
        Self::write_profile(&env, &profile_key, &profile);
        Self::write_revision(&env, &profile, now);
        env.events()
            .publish((symbol_short!("MPRVIS"),), (owner, version, visibility));
        Ok(version)
    }

    /// Deactivate or reactivate the caller's profile. Repeating the current
    /// status is an idempotent no-op.
    pub fn set_status(
        env: Env,
        owner: Address,
        status: ProfileStatus,
    ) -> Result<u32, ContractError> {
        owner.require_auth();
        let profile_key = DataKey::Profile(owner.clone());
        let mut profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&profile_key)
            .ok_or(ContractError::ProfileNotFound)?;
        if profile.status == status {
            return Ok(profile.version);
        }
        let version = Self::next_version(&env, &owner)?;
        let now = env.ledger().timestamp();
        profile.version = version;
        profile.status = status;
        profile.updated_at = now;
        Self::write_profile(&env, &profile_key, &profile);
        Self::write_revision(&env, &profile, now);
        env.events()
            .publish((symbol_short!("MPRSTAT"),), (owner, version, status));
        Ok(version)
    }

    /// Public read returns commitments and policy metadata, never profile
    /// text, contact details, or links.
    pub fn get_profile(env: Env, owner: Address) -> Result<MentorProfile, ContractError> {
        let key = DataKey::Profile(owner);
        let profile: MentorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProfileNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::touch_version_count(&env, &profile.owner);
        Ok(profile)
    }

    /// Read an immutable profile metadata revision (1-based).
    pub fn get_profile_revision(
        env: Env,
        owner: Address,
        version: u32,
    ) -> Result<ProfileRevision, ContractError> {
        if version == 0 {
            return Err(ContractError::ProfileNotFound);
        }
        let key = DataKey::ProfileRevision(owner, version);
        let revision: ProfileRevision = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProfileNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::touch_version_count(&env, &revision.owner);
        Ok(revision)
    }

    fn validate_profile(
        env: &Env,
        commitment: &BytesN<32>,
        schema_version: u32,
    ) -> Result<(), ContractError> {
        if *commitment == BytesN::from_array(env, &[0u8; 32]) {
            return Err(ContractError::InvalidCommitment);
        }
        if schema_version == 0 {
            return Err(ContractError::InvalidSchemaVersion);
        }
        Ok(())
    }

    fn next_version(env: &Env, owner: &Address) -> Result<u32, ContractError> {
        let count_key = DataKey::ProfileVersionCount(owner.clone());
        let current: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0);
        let next = current.checked_add(1).ok_or(ContractError::VersionOverflow)?;
        env.storage().persistent().set(&count_key, &next);
        env.storage()
            .persistent()
            .extend_ttl(&count_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(next)
    }

    fn write_profile(env: &Env, key: &DataKey, profile: &MentorProfile) {
        env.storage().persistent().set(key, profile);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_revision(env: &Env, profile: &MentorProfile, updated_at: u64) {
        let key = DataKey::ProfileRevision(profile.owner.clone(), profile.version);
        let revision = ProfileRevision {
            owner: profile.owner.clone(),
            version: profile.version,
            profile_commitment: profile.profile_commitment.clone(),
            schema_version: profile.schema_version,
            visibility: profile.visibility,
            status: profile.status,
            updated_at,
        };
        env.storage().persistent().set(&key, &revision);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn touch_version_count(env: &Env, owner: &Address) {
        let key = DataKey::ProfileVersionCount(owner.clone());
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
    }
}

#[cfg(test)]
mod tests;