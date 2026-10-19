#![no_std]

mod agreements;
mod amendments;
mod blocking;
mod errors;
mod events;
mod storage;
mod types;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

pub use errors::MentorshipError;
pub use types::{
    Acceptance, Agreement, AgreementStatus, Amendment, AmendmentStatus, Block, MatchingAllowed,
};

#[contract]
pub struct MentorshipAgreementsContract;

#[contractimpl]
impl MentorshipAgreementsContract {
    /// Initialize the contract
    pub fn init(env: Env, admin: Address) -> Result<(), MentorshipError> {
        // Check if already initialized
        if storage::is_initialized(&env) {
            return Err(MentorshipError::AlreadyInitialized);
        }

        // Set admin
        storage::set_admin(&env, &admin);
        storage::set_initialized(&env);

        // Emit event
        events::initialized(&env, &admin);

        Ok(())
    }

    // ========== BLOCKING FUNCTIONS (MENTOR-044) ==========

    /// Block a user from matching
    pub fn block_user(
        env: Env,
        blocker: Address,
        blocked: Address,
        reason_hash: Option<BytesN<32>>,
    ) -> Result<(), MentorshipError> {
        blocking::block_user(&env, blocker, blocked, reason_hash)
    }

    /// Unblock a user
    pub fn unblock_user(
        env: Env,
        blocker: Address,
        blocked: Address,
    ) -> Result<(), MentorshipError> {
        blocking::unblock_user(&env, blocker, blocked)
    }

    /// Check if two users can be matched (bidirectional block check)
    pub fn check_matching_allowed(
        env: Env,
        user1: Address,
        user2: Address,
    ) -> MatchingAllowed {
        blocking::check_matching_allowed(&env, &user1, &user2)
    }

    /// Get block details
    pub fn get_block(env: Env, blocker: Address, blocked: Address) -> Option<Block> {
        blocking::get_block_info(&env, &blocker, &blocked)
    }

    // ========== AGREEMENT FUNCTIONS (MENTOR-045) ==========

    /// Create a new mentorship agreement
    #[allow(clippy::too_many_arguments)]
    pub fn create_agreement(
        env: Env,
        mentor: Address,
        mentee: Address,
        program_hash: BytesN<32>,
        scope_hash: BytesN<32>,
        start_time: u64,
        end_time: u64,
        cadence_seconds: u64,
        price_per_session: i128,
        token: Address,
        cancellation_policy_hash: BytesN<32>,
        conduct_hash: BytesN<32>,
        evidence_hash: BytesN<32>,
        terms_version: u32,
    ) -> Result<u64, MentorshipError> {
        agreements::create_agreement(
            &env,
            mentor,
            mentee,
            program_hash,
            scope_hash,
            start_time,
            end_time,
            cadence_seconds,
            price_per_session,
            token,
            cancellation_policy_hash,
            conduct_hash,
            evidence_hash,
            terms_version,
        )
    }

    // ========== ACCEPTANCE FUNCTIONS (MENTOR-046) ==========

    /// Accept an agreement with replay protection
    pub fn accept_agreement(
        env: Env,
        actor: Address,
        agreement_id: u64,
        agreement_hash: BytesN<32>,
        expected_nonce: u64,
    ) -> Result<(), MentorshipError> {
        agreements::accept_agreement(&env, actor, agreement_id, agreement_hash, expected_nonce)
    }

    /// Get current nonce for a user (for replay protection)
    pub fn get_user_nonce(env: Env, user: Address) -> u64 {
        storage::get_user_nonce(&env, &user)
    }

    /// Check if a party has accepted an agreement
    pub fn has_accepted(env: Env, agreement_id: u64, actor: Address) -> bool {
        agreements::has_accepted(&env, agreement_id, &actor)
    }

    /// Get acceptance record
    pub fn get_acceptance(env: Env, agreement_id: u64, actor: Address) -> Option<Acceptance> {
        storage::get_acceptance(&env, agreement_id, &actor)
    }

    // ========== AMENDMENT FUNCTIONS (MENTOR-048) ==========

    /// Propose an amendment to an active agreement
    #[allow(clippy::too_many_arguments)]
    pub fn propose_amendment(
        env: Env,
        proposer: Address,
        agreement_id: u64,
        new_cadence: Option<u64>,
        new_scope_hash: Option<BytesN<32>>,
        new_start_time: Option<u64>,
        new_end_time: Option<u64>,
        new_price: Option<i128>,
        new_cancellation_hash: Option<BytesN<32>>,
        impact_hash: BytesN<32>,
    ) -> Result<u64, MentorshipError> {
        amendments::propose_amendment(
            &env,
            proposer,
            agreement_id,
            new_cadence,
            new_scope_hash,
            new_start_time,
            new_end_time,
            new_price,
            new_cancellation_hash,
            impact_hash,
        )
    }

    /// Approve an amendment
    pub fn approve_amendment(
        env: Env,
        approver: Address,
        amendment_id: u64,
    ) -> Result<(), MentorshipError> {
        amendments::approve_amendment(&env, approver, amendment_id)
    }

    /// Reject an amendment
    pub fn reject_amendment(
        env: Env,
        rejector: Address,
        amendment_id: u64,
    ) -> Result<(), MentorshipError> {
        amendments::reject_amendment(&env, rejector, amendment_id)
    }

    /// Activate an approved amendment
    pub fn activate_amendment(
        env: Env,
        caller: Address,
        amendment_id: u64,
    ) -> Result<(), MentorshipError> {
        amendments::activate_amendment(&env, caller, amendment_id)
    }

    /// Get amendment details
    pub fn get_amendment(env: Env, amendment_id: u64) -> Result<Amendment, MentorshipError> {
        amendments::get_amendment_info(&env, amendment_id)
    }

    // ========== AGREEMENT MANAGEMENT ==========

    /// Get agreement details
    pub fn get_agreement(env: Env, agreement_id: u64) -> Result<Agreement, MentorshipError> {
        agreements::get_agreement_info(&env, agreement_id)
    }

    /// Cancel an agreement
    pub fn cancel_agreement(
        env: Env,
        caller: Address,
        agreement_id: u64,
    ) -> Result<(), MentorshipError> {
        agreements::cancel_agreement(&env, caller, agreement_id)
    }

    /// Complete an agreement
    pub fn complete_agreement(
        env: Env,
        caller: Address,
        agreement_id: u64,
    ) -> Result<(), MentorshipError> {
        agreements::complete_agreement(&env, caller, agreement_id)
    }

    // ========== ADMIN FUNCTIONS ==========

    /// Get admin address
    pub fn get_admin(env: Env) -> Result<Address, MentorshipError> {
        storage::get_admin(&env)
    }
}

#[cfg(test)]
mod test;
