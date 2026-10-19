#![no_std]

//! Mentorship eligibility verification contract.
//!
//! Manages mentor and mentee eligibility status and preflight checks per program.
//! Works with mentorship-core to enforce eligibility before match creation.

mod errors;
mod storage;
mod types;

pub use errors::ContractError;
pub use types::{EligibilityStatus, EligibilityReason};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;

#[contract]
pub struct MentorshipEligibilityContract;

#[contractimpl]
impl MentorshipEligibilityContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&storage::DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&storage::DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: Set mentor eligibility for a program.
    pub fn set_mentor_eligibility(
        env: Env,
        admin: Address,
        mentor: Address,
        program_id: BytesN<32>,
        status: EligibilityStatus,
        reason_hash: BytesN<32>,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        let now = env.ledger().timestamp();
        if expires_at < now {
            return Err(ContractError::InvalidExpiry);
        }

        storage::put_mentor_eligibility(
            &env,
            &mentor,
            &program_id,
            status,
            reason_hash,
            expires_at,
        );
        Ok(())
    }

    /// Admin-only: Set mentee eligibility for a program.
    pub fn set_mentee_eligibility(
        env: Env,
        admin: Address,
        mentee: Address,
        program_id: BytesN<32>,
        status: EligibilityStatus,
        reason_hash: BytesN<32>,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        let now = env.ledger().timestamp();
        if expires_at < now {
            return Err(ContractError::InvalidExpiry);
        }

        storage::put_mentee_eligibility(
            &env,
            &mentee,
            &program_id,
            status,
            reason_hash,
            expires_at,
        );
        Ok(())
    }

    /// Check if a mentor is eligible for a program.
    /// Returns true if status is Eligible and not expired.
    pub fn is_mentor_eligible(
        env: Env,
        mentor: Address,
        program_id: BytesN<32>,
    ) -> bool {
        storage::is_mentor_eligible(&env, &mentor, &program_id, env.ledger().timestamp())
    }

    /// Check if a mentee is eligible for a program.
    /// Returns true if status is Eligible and not expired.
    pub fn is_mentee_eligible(
        env: Env,
        mentee: Address,
        program_id: BytesN<32>,
    ) -> bool {
        storage::is_mentee_eligible(&env, &mentee, &program_id, env.ledger().timestamp())
    }

    /// Get full eligibility details for a mentor in a program.
    pub fn get_mentor_eligibility(
        env: Env,
        mentor: Address,
        program_id: BytesN<32>,
    ) -> Result<types::EligibilityRecord, ContractError> {
        storage::get_mentor_eligibility(&env, &mentor, &program_id)
            .ok_or(ContractError::NotFound)
    }

    /// Get full eligibility details for a mentee in a program.
    pub fn get_mentee_eligibility(
        env: Env,
        mentee: Address,
        program_id: BytesN<32>,
    ) -> Result<types::EligibilityRecord, ContractError> {
        storage::get_mentee_eligibility(&env, &mentee, &program_id)
            .ok_or(ContractError::NotFound)
    }

    /// Preflight: Check if mentor and mentee are both eligible for a program.
    pub fn preflight_check(
        env: Env,
        mentor: Address,
        mentee: Address,
        program_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let now = env.ledger().timestamp();

        if !storage::is_mentor_eligible(&env, &mentor, &program_id, now) {
            return Err(ContractError::MentorIneligible);
        }

        if !storage::is_mentee_eligible(&env, &mentee, &program_id, now) {
            return Err(ContractError::MenteeIneligible);
        }

        Ok(())
    }
}
