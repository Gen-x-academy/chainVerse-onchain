#![no_std]
//! Stable, opaque public identifiers for off-chain mentorship records.
//!
//! Issuers submit a 32-byte commitment to their private relational key. The
//! contract stores that commitment only as a lookup key and emits only the
//! generated public and correlation identifiers. Public identifiers are
//! domain-separated SHA-256 digests of the contract, commitment, and a private
//! monotonic issuance counter; they do not reveal the counter or a sequential
//! database key.
//!
//! Persistent entries are extended to the network's supported maximum TTL
//! whenever read or written. Deployments must keep active identifiers alive
//! with periodic reads; Soroban storage is not perpetual without TTL renewal.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 100_000;
const RECORD_MAX_TTL: u32 = 6_220_800;
const ADMIN_MIN_TTL: u32 = 100_000;
const ADMIN_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    NotFound = 4,
    Overflow = 5,
    IdentifierCollision = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipIdentity {
    pub public_id: BytesN<32>,
    pub correlation_id: BytesN<32>,
    pub created_at: u64,
    pub issued_by: Address,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    IssuanceCount,
    ByReference(BytesN<32>),
    ByPublicId(BytesN<32>),
}

#[contract]
pub struct MentorshipIdentifiersContract;

#[contractimpl]
impl MentorshipIdentifiersContract {
    /// Set the trusted issuer. The supplied address must authorize setup.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(ADMIN_MIN_TTL, ADMIN_MAX_TTL);
        Ok(())
    }

    /// Register a private internal key commitment and return stable public
    /// and correlation IDs. Repeating a registration returns the original
    /// identifiers and does not emit a second registration event.
    ///
    /// `reference_hash` must be a high-entropy commitment to an off-chain
    /// relational key, not the raw key or a hash of low-entropy personal data.
    pub fn register(
        env: Env,
        issuer: Address,
        reference_hash: BytesN<32>,
    ) -> Result<MentorshipIdentity, ContractError> {
        Self::require_admin(&env, &issuer)?;

        let reference_key = DataKey::ByReference(reference_hash.clone());
        if let Some(identity) = env
            .storage()
            .persistent()
            .get::<DataKey, MentorshipIdentity>(&reference_key)
        {
            Self::extend_record_ttl(&env, &reference_key);
            Self::extend_record_ttl(
                &env,
                &DataKey::ByPublicId(identity.public_id.clone()),
            );
            return Ok(identity);
        }

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::IssuanceCount)
            .unwrap_or(0);
        let next = count.checked_add(1).ok_or(ContractError::Overflow)?;
        let public_id = Self::derive_id(
            &env,
            b"CHAINVERSE_MENTOR_PUBLIC_ID_V1",
            &reference_hash,
            next,
        );
        let correlation_id =
            Self::derive_id(&env, b"CHAINVERSE_MENTOR_CORRELATION_ID_V1", &public_id, next);
        let public_key = DataKey::ByPublicId(public_id.clone());

        if env.storage().persistent().has(&public_key) {
            return Err(ContractError::IdentifierCollision);
        }

        let identity = MentorshipIdentity {
            public_id: public_id.clone(),
            correlation_id: correlation_id.clone(),
            created_at: env.ledger().timestamp(),
            issued_by: issuer,
        };

        env.storage()
            .persistent()
            .set(&reference_key, &identity);
        env.storage()
            .persistent()
            .set(&public_key, &reference_hash);
        Self::extend_record_ttl(&env, &reference_key);
        Self::extend_record_ttl(&env, &public_key);
        env.storage().instance().set(&DataKey::IssuanceCount, &next);
        env.storage()
            .instance()
            .extend_ttl(ADMIN_MIN_TTL, ADMIN_MAX_TTL);

        env.events()
            .publish((symbol_short!("MTRREG"),), (public_id, correlation_id));
        Ok(identity)
    }

    /// Resolve a known private reference commitment to its stable identity.
    pub fn get_by_reference(
        env: Env,
        reference_hash: BytesN<32>,
    ) -> Result<MentorshipIdentity, ContractError> {
        let key = DataKey::ByReference(reference_hash);
        let identity = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend_record_ttl(&env, &key);
        Self::extend_record_ttl(
            &env,
            &DataKey::ByPublicId(identity.public_id.clone()),
        );
        env.storage()
            .instance()
            .extend_ttl(ADMIN_MIN_TTL, ADMIN_MAX_TTL);
        Ok(identity)
    }

    /// Resolve a public identifier without revealing the internal reference.
    pub fn get_by_public_id(
        env: Env,
        public_id: BytesN<32>,
    ) -> Result<MentorshipIdentity, ContractError> {
        let key = DataKey::ByPublicId(public_id);
        let reference_hash: BytesN<32> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend_record_ttl(&env, &key);
        let reference_key = DataKey::ByReference(reference_hash);
        let identity = env
            .storage()
            .persistent()
            .get(&reference_key)
            .ok_or(ContractError::NotFound)?;
        Self::extend_record_ttl(&env, &reference_key);
        env.storage()
            .instance()
            .extend_ttl(ADMIN_MIN_TTL, ADMIN_MAX_TTL);
        Ok(identity)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAuthorized);
        }
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(ADMIN_MIN_TTL, ADMIN_MAX_TTL);
        Ok(())
    }

    fn derive_id(
        env: &Env,
        domain: &[u8],
        seed: &BytesN<32>,
        counter: u64,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, domain));
        input.append(&env.current_contract_address().to_xdr(env));
        input.append(&Bytes::from_slice(env, &counter.to_be_bytes()));
        input.append(&Bytes::from_slice(env, &seed.to_array()));
        env.crypto().sha256(&input).into()
    }

    fn extend_record_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
