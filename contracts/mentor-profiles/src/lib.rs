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
mod types;
mod errors;
mod events;
mod storage;

use soroban_sdk::{contract, contractimpl, Address, Bytes, BytesN, Env};
use types::{AvailabilityStatus, MentorProfile, RatingSubmission};
use errors::ContractError;
use storage::*;

pub use types::{DeliveryMode, TimeWindow, DirectoryEntry};

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
    /// Initialize the contract with admin and backend signer
    /// 
    /// # Arguments
    /// * `admin` - The admin address (can manage backend signer)
    /// * `backend_signer` - The address authorized to submit ratings
    pub fn init(env: Env, admin: Address, backend_signer: Address) -> Result<(), ContractError> {
        // Require that admin authorizes the initialization
        admin.require_auth();

        // Prevent double initialization
        if get_admin(&env).is_some() {
            return Err(ContractError::UnauthorizedAdmin);
        }

        set_admin(&env, &admin);
        set_backend_signer(&env, &backend_signer);

        events::emit_profile_published(
            &env,
            &admin,
            &Bytes::new(&env, [0; 32]),
            0,
        );

        Ok(())
    }

    /// Publish a new mentor profile
    /// 
    /// # Arguments
    /// * `expertise_hash` - Hash of expertise metadata (32 bytes)
    /// * `timezone_offset` - UTC offset (-12 to +14)
    /// * `delivery_modes` - Bitmask of supported delivery modes
    /// * `rate_per_hour` - Hourly rate in stroops
    /// * `public_attestation_hash` - Hash of public endorsements (32 bytes)
    /// * `policy_hash` - Hash of engagement policies (32 bytes)
    pub fn publish_profile(
        env: Env,
        expertise_hash: Bytes,
        timezone_offset: i32,
        delivery_modes: u32,
        rate_per_hour: u128,
        public_attestation_hash: Bytes,
        policy_hash: Bytes,
    ) -> Result<(), ContractError> {
        let caller = env.current_contract_address();
        let mentor_id = env.invoker();
        mentor_id.require_auth();

        // Validate inputs
        if expertise_hash.len() != 32 {
            return Err(ContractError::InvalidHashLength);
        }
        if timezone_offset < -12 || timezone_offset > 14 {
            return Err(ContractError::InvalidTimezone);
        }
        if delivery_modes == 0 {
            return Err(ContractError::InvalidDeliveryMode);
        }
        if rate_per_hour == 0 {
            return Err(ContractError::InvalidRate);
        }
        if public_attestation_hash.len() != 32 {
            return Err(ContractError::InvalidHashLength);
        }
        if policy_hash.len() != 32 {
            return Err(ContractError::InvalidHashLength);
        }

        // Check if profile already exists
        if profile_exists(&env, &mentor_id) {
            return Err(ContractError::ProfileAlreadyExists);
        }

        let current_ledger = env.ledger().sequence();

        let profile = MentorProfile {
            mentor_id: mentor_id.clone(),
            expertise_hash: expertise_hash.clone(),
            availability_status: AvailabilityStatus::Available,
            timezone_offset,
            delivery_modes,
            rate_per_hour,
            rating_count: 0,
            rating_sum: 0,
            reputation_score: 0,
            public_attestation_hash,
            policy_hash,
            created_at: current_ledger,
            updated_at: current_ledger,
        };

        set_profile(&env, &mentor_id, &profile);
        events::emit_profile_published(&env, &mentor_id, &expertise_hash, rate_per_hour);

        Ok(())
    }

    /// Update mentor availability status and timezone
    /// 
    /// # Arguments
    /// * `status` - New availability status (0=Available, 1=Booked, 2=Archived)
    /// * `timezone_offset` - Updated timezone offset
    pub fn update_availability(
        env: Env,
        status: u32,
        timezone_offset: i32,
    ) -> Result<(), ContractError> {
        let mentor_id = env.invoker();
        mentor_id.require_auth();

        // Validate status
        let availability_status = match status {
            0 => AvailabilityStatus::Available,
            1 => AvailabilityStatus::Booked,
            2 => AvailabilityStatus::Archived,
            _ => return Err(ContractError::InvalidStatus),
        };

        // Validate timezone
        if timezone_offset < -12 || timezone_offset > 14 {
            return Err(ContractError::InvalidTimezone);
        }

        // Get existing profile
        let mut profile = get_profile(&env, &mentor_id)
            .ok_or(ContractError::ProfileNotFound)?;

        profile.availability_status = availability_status;
        profile.timezone_offset = timezone_offset;
        profile.updated_at = env.ledger().sequence();

        set_profile(&env, &mentor_id, &profile);
        events::emit_availability_updated(&env, &mentor_id, status);

        Ok(())
    }

    /// Get a mentor's profile (public read)
    pub fn get_profile(env: Env, mentor_id: Address) -> Result<MentorProfile, ContractError> {
        get_profile(&env, &mentor_id).ok_or(ContractError::ProfileNotFound)
    }

    /// Archive the caller's profile
    pub fn archive_profile(env: Env) -> Result<(), ContractError> {
        let mentor_id = env.invoker();
        mentor_id.require_auth();

        let mut profile = get_profile(&env, &mentor_id)
            .ok_or(ContractError::ProfileNotFound)?;

        profile.availability_status = AvailabilityStatus::Archived;
        profile.updated_at = env.ledger().sequence();

        set_profile(&env, &mentor_id, &profile);
        events::emit_profile_archived(&env, &mentor_id);

        Ok(())
    }

    /// Restore the caller's profile from archived status
    pub fn restore_profile(env: Env) -> Result<(), ContractError> {
        let mentor_id = env.invoker();
        mentor_id.require_auth();

        let mut profile = get_profile(&env, &mentor_id)
            .ok_or(ContractError::ProfileNotFound)?;

        if profile.availability_status != AvailabilityStatus::Archived {
            return Err(ContractError::InvalidStatus);
        }

        profile.availability_status = AvailabilityStatus::Available;
        profile.updated_at = env.ledger().sequence();

        set_profile(&env, &mentor_id, &profile);
        events::emit_profile_restored(&env, &mentor_id);

        Ok(())
    }

    /// Record a rating for a mentor (backend only)
    /// 
    /// # Arguments
    /// * `mentor_id` - The mentor being rated
    /// * `score` - Rating score (1-5)
    /// * `review_hash` - Hash of review content (32 bytes)
    /// * `engagement_hash` - Hash of engagement reference (32 bytes)
    pub fn record_rating(
        env: Env,
        mentor_id: Address,
        score: u32,
        review_hash: Bytes,
        engagement_hash: Bytes,
    ) -> Result<(), ContractError> {
        // Only backend signer can submit ratings
        let backend = get_backend_signer(&env)
            .ok_or(ContractError::UnauthorizedBackend)?;
        backend.require_auth();

        // Validate rating score
        if score < 1 || score > 5 {
            return Err(ContractError::InvalidRatingScore);
        }

        // Validate hashes
        if review_hash.len() != 32 || engagement_hash.len() != 32 {
            return Err(ContractError::InvalidHashLength);
        }

        let mut profile = get_profile(&env, &mentor_id)
            .ok_or(ContractError::ProfileNotFound)?;

        // Update rating
        profile.rating_sum = profile.rating_sum.saturating_add(score as u32);
        profile.rating_count = profile.rating_count.saturating_add(1);

        // Update reputation score (simplified: 10x weight on average rating)
        let avg_rating = profile.rating_sum / profile.rating_count.max(1);
        let multiplier = get_reputation_multiplier(&env);
        profile.reputation_score = (avg_rating * multiplier).min(100);

        profile.updated_at = env.ledger().sequence();

        set_profile(&env, &mentor_id, &profile);
        increment_rating_counter(&env);

        let avg_rating_for_event = profile.rating_sum / profile.rating_count;
        events::emit_rating_updated(&env, &mentor_id, avg_rating_for_event, profile.rating_count);

        Ok(())
    }

    /// Update backend signer (admin only)
    pub fn set_backend_signer(env: Env, new_signer: Address) -> Result<(), ContractError> {
        let caller = env.invoker();
        require_admin(&env, &caller)?;

        storage::set_backend_signer(&env, &new_signer);

        Ok(())
    }

    /// Transfer admin to a new address (two-step process: propose then accept)
    pub fn propose_admin_transfer(env: Env, new_admin: Address) -> Result<(), ContractError> {
        let caller = env.invoker();
        require_admin(&env, &caller)?;

        set_pending_admin(&env, &new_admin);

        Ok(())
    }

    /// Accept admin transfer (must be called by the proposed admin)
    pub fn accept_admin_transfer(env: Env) -> Result<(), ContractError> {
        let caller = env.invoker();
        let pending = get_pending_admin(&env)
            .ok_or(ContractError::UnauthorizedAdmin)?;

        if pending != caller {
            return Err(ContractError::UnauthorizedAdmin);
        }

        caller.require_auth();

        set_admin(&env, &caller);
        clear_pending_admin(&env);

        Ok(())
    }

    /// Get the current admin
    pub fn get_admin(env: Env) -> Option<Address> {
        get_admin(&env)
    }

    /// Get the backend signer
    pub fn get_backend_signer(env: Env) -> Option<Address> {
        get_backend_signer(&env)
    }
}
