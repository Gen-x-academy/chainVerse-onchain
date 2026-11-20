#![no_std]

//! Immutable, versioned snapshots of published capstone call terms.
//!
//! The terms themselves stay off-chain. Each version stores only bounded
//! commitments to the eligibility, evaluation, IP, conduct, data, funding,
//! publication, and completion sections.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    InvalidVersion = 1,
    VersionAlreadyPublished = 2,
    UnauthorizedOwner = 3,
    CallNotFound = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Owner(BytesN<32>),
    Terms(BytesN<32>, u32),
    Latest(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallTerms {
    pub eligibility: BytesN<32>,
    pub evaluation: BytesN<32>,
    pub intellectual_property: BytesN<32>,
    pub conduct: BytesN<32>,
    pub data: BytesN<32>,
    pub funding: BytesN<32>,
    pub publication: BytesN<32>,
    pub completion: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedTerms {
    pub version: u32,
    pub published_by: Address,
    pub published_at: u64,
    pub terms: CallTerms,
}

#[contract]
pub struct CapstoneCallTermsContract;

#[contractimpl]
impl CapstoneCallTermsContract {
    pub fn publish_version(
        env: Env,
        authority: Address,
        call_id: BytesN<32>,
        version: u32,
        terms: CallTerms,
    ) -> Result<(), ContractError> {
        authority.require_auth();
        if version == 0 {
            return Err(ContractError::InvalidVersion);
        }

        let owner_key = DataKey::Owner(call_id.clone());
        let expected_owner: Option<Address> = env.storage().persistent().get(&owner_key);
        match expected_owner {
            Some(owner) => {
                if owner != authority {
                    return Err(ContractError::UnauthorizedOwner);
                }
                let latest: u32 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Latest(call_id.clone()))
                    .ok_or(ContractError::CallNotFound)?;
                if version <= latest {
                    return Err(ContractError::VersionAlreadyPublished);
                }
            }
            None => {
                env.storage().persistent().set(&owner_key, &authority);
                Self::extend(&env, &owner_key);
            }
        }

        let terms_key = DataKey::Terms(call_id.clone(), version);
        if env.storage().persistent().has(&terms_key) {
            return Err(ContractError::VersionAlreadyPublished);
        }
        let published = PublishedTerms {
            version,
            published_by: authority.clone(),
            published_at: env.ledger().timestamp(),
            terms,
        };
        env.storage().persistent().set(&terms_key, &published);
        Self::extend(&env, &terms_key);
        let latest_key = DataKey::Latest(call_id.clone());
        env.storage().persistent().set(&latest_key, &version);
        Self::extend(&env, &latest_key);
        env.events().publish(
            (symbol_short!("CAPTERM"),),
            (call_id, version, authority),
        );
        Ok(())
    }

    pub fn get_version(
        env: Env,
        call_id: BytesN<32>,
        version: u32,
    ) -> Option<PublishedTerms> {
        env.storage()
            .persistent()
            .get(&DataKey::Terms(call_id, version))
    }

    pub fn latest_version(env: Env, call_id: BytesN<32>) -> Option<u32> {
        env.storage().persistent().get(&DataKey::Latest(call_id))
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
