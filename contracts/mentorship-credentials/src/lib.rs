#![no_std]

//! Privacy-minimized mentor credential and experience claim attestations.
//!
//! Claim descriptions and supporting documents stay off-chain. The contract
//! stores only content/reference hashes, a category, an experience amount,
//! public addresses, reviewer decisions, and status.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const MAX_CLAIMS_PER_MENTOR: u32 = 64;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    UnauthorizedReviewer = 4,
    ReviewerAlreadySet = 5,
    ClaimAlreadyExists = 6,
    ClaimNotFound = 7,
    InvalidClaim = 8,
    InvalidReason = 9,
    InvalidStatus = 10,
    ClaimLimitReached = 11,
    CounterOverflow = 12,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Reviewer(Address),
    Claim(Address, BytesN<32>),
    ClaimCount(Address),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    Submitted,
    Approved,
    Rejected,
    Expired,
    Revoked,
}

/// Public, privacy-minimized record for a single mentor assertion.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialClaim {
    pub claim_id: BytesN<32>,
    pub mentor: Address,
    pub category_code: u32,
    pub experience_years: u32,
    pub claim_hash: BytesN<32>,
    pub evidence_ref_hash: BytesN<32>,
    pub submitted_at: u64,
    pub status: ClaimStatus,
    pub reviewer: Option<Address>,
    pub reviewed_at: u64,
    pub reason_code: u32,
}

#[contract]
pub struct MentorshipCredentialsContract;

#[contractimpl]
impl MentorshipCredentialsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if Self::admin(env)? != *caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_reviewer(env: &Env, reviewer: &Address) -> Result<(), ContractError> {
        let key = DataKey::Reviewer(reviewer.clone());
        if !env.storage().persistent().get(&key).unwrap_or(false) {
            return Err(ContractError::UnauthorizedReviewer);
        }
        reviewer.require_auth();
        Self::extend_ttl(env, &key);
        Ok(())
    }

    fn extend_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    /// Reviewer authority is managed by the platform admin.
    pub fn set_reviewer(
        env: Env,
        admin: Address,
        reviewer: Address,
        enabled: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Reviewer(reviewer.clone());
        let current: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if current == enabled {
            return Err(ContractError::ReviewerAlreadySet);
        }
        env.storage().persistent().set(&key, &enabled);
        Self::extend_ttl(&env, &key);
        env.events().publish(
            (if enabled {
                symbol_short!("REVGRNT")
            } else {
                symbol_short!("REVRVK")
            },),
            (reviewer, admin),
        );
        Ok(())
    }

    /// A mentor submits one immutable claim. All descriptive material and
    /// evidence remain off-chain; the two hashes are commitments only.
    pub fn submit_claim(
        env: Env,
        mentor: Address,
        claim_id: BytesN<32>,
        category_code: u32,
        experience_years: u32,
        claim_hash: BytesN<32>,
        evidence_ref_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::admin(&env)?;
        mentor.require_auth();
        if !Self::valid_hash(&env, &claim_id)
            || category_code == 0
            || !Self::valid_hash(&env, &claim_hash)
            || !Self::valid_hash(&env, &evidence_ref_hash)
        {
            return Err(ContractError::InvalidClaim);
        }

        let key = DataKey::Claim(mentor.clone(), claim_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ClaimAlreadyExists);
        }

        let count_key = DataKey::ClaimCount(mentor.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_CLAIMS_PER_MENTOR {
            return Err(ContractError::ClaimLimitReached);
        }
        let next_count = count
            .checked_add(1)
            .ok_or(ContractError::CounterOverflow)?;

        let claim = CredentialClaim {
            claim_id: claim_id.clone(),
            mentor: mentor.clone(),
            category_code,
            experience_years,
            claim_hash,
            evidence_ref_hash,
            submitted_at: env.ledger().timestamp(),
            status: ClaimStatus::Submitted,
            reviewer: None,
            reviewed_at: 0,
            reason_code: 0,
        };
        env.storage().persistent().set(&key, &claim);
        Self::extend_ttl(&env, &key);
        env.storage().persistent().set(&count_key, &next_count);
        Self::extend_ttl(&env, &count_key);
        env.events().publish(
            (symbol_short!("CLMSUB"),),
            (mentor, claim_id, category_code, experience_years),
        );
        Ok(())
    }

    pub fn approve_claim(
        env: Env,
        reviewer: Address,
        mentor: Address,
        claim_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::review_claim(
            &env,
            &reviewer,
            &mentor,
            &claim_id,
            ClaimStatus::Approved,
            0,
        )?;
        env.events().publish(
            (symbol_short!("CLMAPR"),),
            (mentor, claim_id, reviewer, env.ledger().timestamp()),
        );
        Ok(())
    }

    pub fn reject_claim(
        env: Env,
        reviewer: Address,
        mentor: Address,
        claim_id: BytesN<32>,
        reason_code: u32,
    ) -> Result<(), ContractError> {
        if reason_code == 0 {
            return Err(ContractError::InvalidReason);
        }
        Self::review_claim(
            &env,
            &reviewer,
            &mentor,
            &claim_id,
            ClaimStatus::Rejected,
            reason_code,
        )?;
        env.events().publish(
            (symbol_short!("CLMREJ"),),
            (
                mentor,
                claim_id,
                reviewer,
                reason_code,
                env.ledger().timestamp(),
            ),
        );
        Ok(())
    }

    pub fn expire_claim(
        env: Env,
        reviewer: Address,
        mentor: Address,
        claim_id: BytesN<32>,
        reason_code: u32,
    ) -> Result<(), ContractError> {
        if reason_code == 0 {
            return Err(ContractError::InvalidReason);
        }
        Self::review_claim(
            &env,
            &reviewer,
            &mentor,
            &claim_id,
            ClaimStatus::Expired,
            reason_code,
        )?;
        env.events().publish(
            (symbol_short!("CLMEXP"),),
            (
                mentor,
                claim_id,
                reviewer,
                reason_code,
                env.ledger().timestamp(),
            ),
        );
        Ok(())
    }

    pub fn revoke_claim(
        env: Env,
        reviewer: Address,
        mentor: Address,
        claim_id: BytesN<32>,
        reason_code: u32,
    ) -> Result<(), ContractError> {
        if reason_code == 0 {
            return Err(ContractError::InvalidReason);
        }
        let claim = Self::load_claim(&env, &mentor, &claim_id)?;
        if claim.status != ClaimStatus::Approved {
            return Err(ContractError::InvalidStatus);
        }
        Self::review_claim(
            &env,
            &reviewer,
            &mentor,
            &claim_id,
            ClaimStatus::Revoked,
            reason_code,
        )?;
        env.events().publish(
            (symbol_short!("CLMRVK"),),
            (
                mentor,
                claim_id,
                reviewer,
                reason_code,
                env.ledger().timestamp(),
            ),
        );
        Ok(())
    }

    fn review_claim(
        env: &Env,
        reviewer: &Address,
        mentor: &Address,
        claim_id: &BytesN<32>,
        status: ClaimStatus,
        reason_code: u32,
    ) -> Result<(), ContractError> {
        Self::require_reviewer(env, reviewer)?;
        let key = DataKey::Claim(mentor.clone(), claim_id.clone());
        let mut claim: CredentialClaim = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ClaimNotFound)?;
        if claim.status != ClaimStatus::Submitted
            && !(status == ClaimStatus::Revoked && claim.status == ClaimStatus::Approved)
        {
            return Err(ContractError::InvalidStatus);
        }
        claim.status = status;
        claim.reviewer = Some(reviewer.clone());
        claim.reviewed_at = env.ledger().timestamp();
        claim.reason_code = reason_code;
        env.storage().persistent().set(&key, &claim);
        Self::extend_ttl(env, &key);
        Ok(())
    }

    fn load_claim(
        env: &Env,
        mentor: &Address,
        claim_id: &BytesN<32>,
    ) -> Result<CredentialClaim, ContractError> {
        let key = DataKey::Claim(mentor.clone(), claim_id.clone());
        let claim = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ClaimNotFound)?;
        Self::extend_ttl(env, &key);
        Ok(claim)
    }

    fn valid_hash(env: &Env, hash: &BytesN<32>) -> bool {
        *hash != BytesN::from_array(env, &[0; 32])
    }

    /// Claims are public attestations; personal evidence is never returned.
    pub fn get_claim(
        env: Env,
        mentor: Address,
        claim_id: BytesN<32>,
    ) -> Result<CredentialClaim, ContractError> {
        Self::load_claim(&env, &mentor, &claim_id)
    }

    pub fn storage_version() -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
