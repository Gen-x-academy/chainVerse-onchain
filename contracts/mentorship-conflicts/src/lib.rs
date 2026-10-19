#![no_std]

//! Mentorship conflict-of-interest detection contract.
//!
//! Manages and checks organizational, reporting, financial, and declared conflicts
//! before mentor-mentee matches are created. Implements MENTOR-040.

mod errors;
mod storage;
mod types;

pub use errors::ContractError;
pub use types::{ConflictType, ConflictCheckResult};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;

#[contract]
pub struct MentorshipConflictsContract;

#[contractimpl]
impl MentorshipConflictsContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&storage::DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&storage::DataKey::Admin, &admin);
        Ok(())
    }

    /// Admin-only: Register an organization for a participant.
    pub fn register_organization(
        env: Env,
        admin: Address,
        participant: Address,
        organization: Address,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;
        storage::set_organization(&env, &participant, &organization);
        Ok(())
    }

    /// Admin-only: Register a reporting relationship (A reports to B).
    pub fn register_reporting_relationship(
        env: Env,
        admin: Address,
        subordinate: Address,
        supervisor: Address,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;
        storage::set_reports_to(&env, &subordinate, &supervisor);
        Ok(())
    }

    /// Admin-only: Register a financial interest between two parties.
    pub fn register_financial_conflict(
        env: Env,
        admin: Address,
        party_a: Address,
        party_b: Address,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;
        storage::set_financial_interest(&env, &party_a, &party_b);
        Ok(())
    }

    /// Either party can declare a conflict with the other.
    pub fn register_declared_conflict(
        env: Env,
        declarer: Address,
        other_party: Address,
    ) -> Result<(), ContractError> {
        declarer.require_auth();
        storage::set_declared_conflict(&env, &declarer, &other_party);
        Ok(())
    }

    /// Preflight check: Does a mentor-mentee relationship have conflicts?
    /// Returns Ok(check_result) with passed=true if no conflicts found.
    /// Returns Err(ConflictDetected) if conflicts are found.
    pub fn check_conflicts(
        env: Env,
        check_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
    ) -> Result<ConflictCheckResult, ContractError> {
        let now = env.ledger().timestamp();

        // Check 1: Same organization
        let mentor_org = storage::get_organization(&env, &mentor);
        let mentee_org = storage::get_organization(&env, &mentee);
        if let (Some(mo), Some(me)) = (mentor_org, mentee_org) {
            if mo == me {
                let result = ConflictCheckResult {
                    passed: false,
                    reason_hash: BytesN::from_array(&env, &[1u8; 32]),
                    checked_at: now,
                };
                storage::record_check(&env, &check_id, &result);
                return Err(ContractError::ConflictDetected);
            }
        }

        // Check 2: Reporting relationship (either direction)
        if let Some(supervisor) = storage::get_reports_to(&env, &mentor) {
            if supervisor == mentee {
                let result = ConflictCheckResult {
                    passed: false,
                    reason_hash: BytesN::from_array(&env, &[2u8; 32]),
                    checked_at: now,
                };
                storage::record_check(&env, &check_id, &result);
                return Err(ContractError::ConflictDetected);
            }
        }
        if let Some(supervisor) = storage::get_reports_to(&env, &mentee) {
            if supervisor == mentor {
                let result = ConflictCheckResult {
                    passed: false,
                    reason_hash: BytesN::from_array(&env, &[3u8; 32]),
                    checked_at: now,
                };
                storage::record_check(&env, &check_id, &result);
                return Err(ContractError::ConflictDetected);
            }
        }

        // Check 3: Financial interest
        if storage::has_financial_interest(&env, &mentor, &mentee) {
            let result = ConflictCheckResult {
                passed: false,
                reason_hash: BytesN::from_array(&env, &[4u8; 32]),
                checked_at: now,
            };
            storage::record_check(&env, &check_id, &result);
            return Err(ContractError::ConflictDetected);
        }

        // Check 4: Declared conflict
        if storage::has_declared_conflict(&env, &mentor, &mentee) {
            let result = ConflictCheckResult {
                passed: false,
                reason_hash: BytesN::from_array(&env, &[5u8; 32]),
                checked_at: now,
            };
            storage::record_check(&env, &check_id, &result);
            return Err(ContractError::ConflictDetected);
        }

        // All checks passed
        let result = ConflictCheckResult {
            passed: true,
            reason_hash: BytesN::from_array(&env, &[0u8; 32]),
            checked_at: now,
        };
        storage::record_check(&env, &check_id, &result);
        Ok(result)
    }

    /// Get the result of a previous conflict check.
    pub fn get_check_result(
        env: Env,
        check_id: BytesN<32>,
    ) -> Result<ConflictCheckResult, ContractError> {
        storage::get_check_result(&env, &check_id).ok_or(ContractError::NotFound)
    }

    /// Get the organization for a participant.
    pub fn get_organization(env: Env, participant: Address) -> Option<Address> {
        storage::get_organization(&env, &participant)
    }

    /// Get who a participant reports to.
    pub fn get_reports_to(env: Env, participant: Address) -> Option<Address> {
        storage::get_reports_to(&env, &participant)
    }

    /// Check if there is a financial conflict between two parties.
    pub fn has_financial_interest(env: Env, party_a: Address, party_b: Address) -> bool {
        storage::has_financial_interest(&env, &party_a, &party_b)
    }

    /// Check if there is a declared conflict between two parties.
    pub fn has_declared_conflict(env: Env, party_a: Address, party_b: Address) -> bool {
        storage::has_declared_conflict(&env, &party_a, &party_b)
    }
}
