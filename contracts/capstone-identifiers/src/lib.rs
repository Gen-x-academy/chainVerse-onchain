#![no_std]

//! Stable, non-sequential public identifiers for capstone projects.
//!
//! Internal keys and off-chain content stay private. The contract stores only
//! the derived public id, a correlation commitment, the authority, and a
//! monotonically increasing version reference.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyRegistered = 1,
    IdentityNotFound = 2,
    Unauthorized = 3,
    CorrelationAlreadyUsed = 4,
    VersionOverflow = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Identity(BytesN<32>),
    Correlation(BytesN<32>),
    Version(BytesN<32>, u32),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIdentity {
    pub public_id: BytesN<32>,
    pub internal_key: BytesN<32>,
    pub correlation_id: BytesN<32>,
    pub authority: Address,
    pub version: u32,
    pub created_at: u64,
}

#[contract]
pub struct CapstoneIdentifiersContract;

#[contractimpl]
impl CapstoneIdentifiersContract {
    /// Derive a collision-resistant public id from private-facing inputs and
    /// ledger entropy. The same inputs at the same ledger position always
    /// produce the same id, so retries cannot register a second identity.
    pub fn register_project(
        env: Env,
        authority: Address,
        internal_key: BytesN<32>,
        correlation_id: BytesN<32>,
        entropy: BytesN<32>,
    ) -> Result<BytesN<32>, ContractError> {
        authority.require_auth();

        let public_id = Self::derive_id(
            &env,
            &internal_key,
            &correlation_id,
            &entropy,
            env.ledger().timestamp(),
            env.ledger().sequence(),
            0,
        );
        let key = DataKey::Identity(public_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyRegistered);
        }
        let correlation_key = DataKey::Correlation(correlation_id.clone());
        if env.storage().persistent().has(&correlation_key) {
            return Err(ContractError::CorrelationAlreadyUsed);
        }

        let identity = ProjectIdentity {
            public_id: public_id.clone(),
            internal_key,
            correlation_id: correlation_id.clone(),
            authority: authority.clone(),
            version: 0,
            created_at: env.ledger().timestamp(),
        };
        Self::write(&env, &key, &identity);
        env.storage().persistent().set(&correlation_key, &public_id);
        env.storage()
            .persistent()
            .extend_ttl(&correlation_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("CAPIDNEW"),),
            (public_id.clone(), correlation_id, authority),
        );
        Ok(public_id)
    }

    /// Publish the next version reference for an existing public id.
    pub fn next_version(
        env: Env,
        authority: Address,
        public_id: BytesN<32>,
        entropy: BytesN<32>,
    ) -> Result<u32, ContractError> {
        authority.require_auth();

        let key = DataKey::Identity(public_id.clone());
        let mut identity: ProjectIdentity = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::IdentityNotFound)?;
        if identity.authority != authority {
            return Err(ContractError::Unauthorized);
        }
        let version = identity
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let version_ref = Self::derive_id(
            &env,
            &identity.internal_key,
            &identity.correlation_id,
            &entropy,
            identity.created_at,
            env.ledger().sequence(),
            version,
        );
        let version_key = DataKey::Version(public_id.clone(), version);
        env.storage().persistent().set(&version_key, &version_ref);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        identity.version = version;
        Self::write(&env, &key, &identity);
        env.events().publish(
            (symbol_short!("CAPIDVER"),),
            (public_id, version, version_ref),
        );
        Ok(version)
    }

    pub fn get_identity(env: Env, public_id: BytesN<32>) -> Option<ProjectIdentity> {
        env.storage()
            .persistent()
            .get(&DataKey::Identity(public_id))
    }

    pub fn resolve_correlation(env: Env, correlation_id: BytesN<32>) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::Correlation(correlation_id))
    }

    pub fn get_version_ref(env: Env, public_id: BytesN<32>, version: u32) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&DataKey::Version(public_id, version))
    }

    fn derive_id(
        env: &Env,
        internal_key: &BytesN<32>,
        correlation_id: &BytesN<32>,
        entropy: &BytesN<32>,
        timestamp: u64,
        sequence: u32,
        version: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&internal_key.to_array());
        preimage.extend_from_array(&correlation_id.to_array());
        preimage.extend_from_array(&entropy.to_array());
        preimage.extend_from_array(&timestamp.to_be_bytes());
        preimage.extend_from_array(&sequence.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        env.crypto().sha256(&preimage)
    }

    fn write(env: &Env, key: &DataKey, identity: &ProjectIdentity) {
        env.storage().persistent().set(key, identity);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
