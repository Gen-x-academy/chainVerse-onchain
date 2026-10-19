#![no_std]

//! Immutable, privacy-minimized mentorship program terms and per-application
//! acceptance receipts for MENTOR-016.
//!
//! Term text and applicant data remain off-chain. This contract stores only
//! fixed-size commitments, public addresses, timestamps, version numbers, and
//! status. Each acceptance snapshots its complete terms record so later
//! publications cannot change the terms an existing applicant accepted.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const MAX_TERMS_VERSIONS: u32 = 64;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ProgramNotFound = 4,
    ProgramAlreadyExists = 5,
    NotProgramOwner = 6,
    NoTermsPublished = 7,
    TermsVersionNotFound = 8,
    InvalidExpiry = 9,
    InvalidTermsHash = 10,
    VersionLimitReached = 11,
    VersionOverflow = 12,
    ApplicationAlreadyAccepted = 13,
    AcceptanceNotFound = 14,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Program(BytesN<32>),
    TermsVersion(BytesN<32>),
    Terms(BytesN<32>, u32),
    Acceptance(BytesN<32>, BytesN<32>),
}

/// Fixed-size hashes commit to off-chain content for each published term.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TermsHashes {
    pub eligibility: BytesN<32>,
    pub fees: BytesN<32>,
    pub cancellation: BytesN<32>,
    pub conduct: BytesN<32>,
    pub privacy: BytesN<32>,
    pub evidence: BytesN<32>,
    pub completion: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub owner: Address,
}

/// A published record is immutable. `expires_at` controls new acceptances;
/// it does not invalidate receipts already recorded against this version.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TermsVersion {
    pub version: u32,
    pub hashes: TermsHashes,
    pub published_at: u64,
    pub expires_at: u64,
}

/// The receipt embeds the published version so accepted terms remain
/// inspectable even if the separate published-version entry expires from TTL.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acceptance {
    pub program_id: BytesN<32>,
    pub application_ref: BytesN<32>,
    pub mentee: Address,
    pub accepted_at: u64,
    pub terms: TermsVersion,
}

#[contract]
pub struct MentorshipTermsContract;

#[contractimpl]
impl MentorshipTermsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
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

    fn extend_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    /// The admin binds a stable public program commitment to its publisher.
    /// Ownership cannot be reassigned through this contract.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        owner: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }
        env.storage()
            .persistent()
            .set(&key, &Program { owner: owner.clone() });
        Self::extend_ttl(&env, &key);
        env.events()
            .publish((symbol_short!("PROGREG"),), (program_id, owner));
        Ok(())
    }

    /// The registered publisher appends a new immutable terms version.
    /// A per-program cap bounds persistent storage and publication work.
    pub fn publish_terms(
        env: Env,
        owner: Address,
        program_id: BytesN<32>,
        hashes: TermsHashes,
        expires_at: u64,
    ) -> Result<u32, ContractError> {
        let program_key = DataKey::Program(program_id.clone());
        let program: Program = env
            .storage()
            .persistent()
            .get(&program_key)
            .ok_or(ContractError::ProgramNotFound)?;
        if program.owner != owner {
            return Err(ContractError::NotProgramOwner);
        }
        owner.require_auth();

        if expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        if !Self::valid_hashes(&env, &hashes) {
            return Err(ContractError::InvalidTermsHash);
        }

        let version_key = DataKey::TermsVersion(program_id.clone());
        let previous: u32 = env
            .storage()
            .persistent()
            .get(&version_key)
            .unwrap_or(0);
        if previous >= MAX_TERMS_VERSIONS {
            return Err(ContractError::VersionLimitReached);
        }
        let version = previous
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let terms = TermsVersion {
            version,
            hashes: hashes.clone(),
            published_at: env.ledger().timestamp(),
            expires_at,
        };

        let terms_key = DataKey::Terms(program_id.clone(), version);
        env.storage().persistent().set(&terms_key, &terms);
        Self::extend_ttl(&env, &terms_key);
        env.storage().persistent().set(&version_key, &version);
        Self::extend_ttl(&env, &version_key);
        Self::extend_ttl(&env, &program_key);

        env.events().publish(
            (symbol_short!("TERMPUB"),),
            (program_id, version, hashes, expires_at),
        );
        Ok(version)
    }

    fn valid_hashes(env: &Env, hashes: &TermsHashes) -> bool {
        let zero = BytesN::from_array(env, &[0; 32]);
        hashes.eligibility != zero
            && hashes.fees != zero
            && hashes.cancellation != zero
            && hashes.conduct != zero
            && hashes.privacy != zero
            && hashes.evidence != zero
            && hashes.completion != zero
    }

    pub fn get_latest_version(env: Env, program_id: BytesN<32>) -> Result<u32, ContractError> {
        let key = DataKey::TermsVersion(program_id);
        let version = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NoTermsPublished)?;
        Self::extend_ttl(&env, &key);
        Ok(version)
    }

    pub fn get_terms(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<TermsVersion, ContractError> {
        let key = DataKey::Terms(program_id, version);
        let terms = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::TermsVersionNotFound)?;
        Self::extend_ttl(&env, &key);
        Ok(terms)
    }

    /// The mentee accepts exactly the latest unexpired version for a unique
    /// application commitment. The duplicate key makes retries effect-free.
    pub fn accept_terms(
        env: Env,
        mentee: Address,
        program_id: BytesN<32>,
        application_ref: BytesN<32>,
    ) -> Result<u32, ContractError> {
        mentee.require_auth();
        let acceptance_key = DataKey::Acceptance(program_id.clone(), application_ref.clone());
        if env.storage().persistent().has(&acceptance_key) {
            return Err(ContractError::ApplicationAlreadyAccepted);
        }

        let version = Self::get_latest_version(env.clone(), program_id.clone())?;
        let terms = Self::get_terms(env.clone(), program_id.clone(), version)?;
        if env.ledger().timestamp() >= terms.expires_at {
            return Err(ContractError::InvalidExpiry);
        }

        let acceptance = Acceptance {
            program_id: program_id.clone(),
            application_ref: application_ref.clone(),
            mentee: mentee.clone(),
            accepted_at: env.ledger().timestamp(),
            terms: terms.clone(),
        };
        env.storage()
            .persistent()
            .set(&acceptance_key, &acceptance);
        Self::extend_ttl(&env, &acceptance_key);
        env.events().publish(
            (symbol_short!("TERMACK"),),
            (
                program_id,
                application_ref,
                mentee,
                terms.version,
                acceptance.accepted_at,
            ),
        );
        Ok(terms.version)
    }

    pub fn get_acceptance(
        env: Env,
        program_id: BytesN<32>,
        application_ref: BytesN<32>,
    ) -> Result<Acceptance, ContractError> {
        let key = DataKey::Acceptance(program_id, application_ref);
        let acceptance = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AcceptanceNotFound)?;
        Self::extend_ttl(&env, &key);
        Ok(acceptance)
    }

    /// Storage schema version for deployment and upgrade compatibility checks.
    pub fn storage_version() -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;