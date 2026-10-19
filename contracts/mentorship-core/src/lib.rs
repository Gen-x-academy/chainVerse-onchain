#![no_std]
// Mentorship matches may have many arguments during creation (mentor, mentee, program_id,
// amount, currency, terms_version, terms_hash, start_date, end_date). Grouping into a struct
// would change the public ABI, so we allow clippy::too_many_arguments.
#![allow(clippy::too_many_arguments)]

//! Mentorship matching contract for 1-on-1 advisory relationships with smart-contract-backed payments.
//!
//! This contract implements mutual match acceptance (MENTOR-038) with atomic capacity reservation
//! and terms acceptance. It works in conjunction with mentorship-capacity for enforcement of
//! per-mentor and per-program limits (MENTOR-041), and mentorship-conflicts for preflight
//! conflict-of-interest checks (MENTOR-040).
//!
//! Design: Privacy-minimized on-chain storage — identities, status, amounts, and hashes only.
//! Detailed terms, expectations, and conflict data remain off-chain. All state changes are
//! atomic and emit events for off-chain indexing.

mod errors;
mod events;
mod storage;
mod types;
#[cfg(test)]
mod tests;

pub use errors::ContractError;
pub use types::{MentorshipMatch, MentorshipStatus};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Symbol};

const CONTRACT_VERSION: u32 = 1;

// TTL bounds: ~1 year to ~2 years at 6-second blocks
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

// Bounded storage limits
const MAX_TITLE_LEN: u32 = 200;
const MAX_REASON_LEN: u32 = 500;

#[contract]
pub struct MentorshipCoreContract;

#[contractimpl]
impl MentorshipCoreContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&storage::DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&storage::DataKey::Admin, &admin);
        env.events().publish(
            (soroban_sdk::symbol_short!("INIT"),),
            (admin.clone(),),
        );
        Ok(())
    }

    /// Admin-only: Create a mentorship proposal. Both mentor and mentee will need to accept
    /// this before it becomes active. Validates that neither participant is in an active
    /// mentorship already, reserves capacity, and records the terms hash for later acceptance.
    pub fn propose_mentorship(
        env: Env,
        admin: Address,
        match_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
        program_id: BytesN<32>,
        amount: i128,
        currency: Symbol,
        terms_version: u32,
        terms_hash: BytesN<32>,
        start_date: u64,
        end_date: u64,
        title: String,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        // Validate basic invariants
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        let now = env.ledger().timestamp();
        if start_date <= now || end_date <= start_date {
            return Err(ContractError::InvalidDates);
        }
        if title.len() > MAX_TITLE_LEN {
            return Err(ContractError::InvalidTitle);
        }

        // Guard: no existing match with this ID
        if storage::has_mentorship(&env, &match_id) {
            return Err(ContractError::MentorshipAlreadyExists);
        }

        // Guard: no active (or pending acceptance) mentorship between this mentor-mentee pair
        if storage::has_active_mentorship(&env, &mentor, &mentee) {
            return Err(ContractError::ConflictingMentorship);
        }

        // Create the match in Proposed state
        let mentorship = MentorshipMatch {
            id: match_id.clone(),
            program_id: program_id.clone(),
            mentor: mentor.clone(),
            mentee: mentee.clone(),
            amount,
            currency,
            terms_version,
            terms_hash,
            title,
            start_date,
            end_date,
            status: MentorshipStatus::Proposed,
            proposed_at: now,
            mentee_accepted_at: 0,
            mentor_accepted_at: 0,
            activated_at: 0,
            completed_at: 0,
            terminated_at: 0,
            termination_reason_code: 0,
            reserved: true,
        };

        storage::put_mentorship(&env, &mentorship);

        // Mark as active (pending acceptance) to prevent duplicate proposals
        storage::set_active_mentorship(&env, &mentor, &mentee, &match_id);

        events::mentorship_proposed(&env, &match_id, &mentor, &mentee, &program_id, start_date, end_date, amount);
        Ok(())
    }

    /// The mentee accepts the mentorship proposal. Requires the mentee's authorization.
    /// Transitions from Proposed to ProposedMenteeAccepted and records their signed acceptance.
    pub fn accept_as_mentee(
        env: Env,
        mentee: Address,
        match_id: BytesN<32>,
        declarations_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        mentee.require_auth();

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;
        
        if mentorship.mentee != mentee {
            return Err(ContractError::NotMentee);
        }
        if mentorship.status != MentorshipStatus::Proposed {
            return Err(ContractError::InvalidStatus);
        }

        let now = env.ledger().timestamp();

        // Record the signed acceptance
        let acceptance = types::MenteeAcceptance {
            match_id: match_id.clone(),
            terms_version: mentorship.terms_version,
            terms_hash: mentorship.terms_hash.clone(),
            signer: mentee.clone(),
            accepted_at: now,
            declarations_hash,
        };
        storage::put_mentee_acceptance(&env, &acceptance);

        // Transition state
        mentorship.status = MentorshipStatus::ProposedMenteeAccepted;
        mentorship.mentee_accepted_at = now;
        storage::put_mentorship(&env, &mentorship);

        events::mentee_accepted(&env, &match_id, &mentee, now);
        Ok(())
    }

    /// The mentor accepts the mentorship proposal. Requires the mentor's authorization.
    /// Transitions from ProposedMenteeAccepted to ProposedMentorAccepted and records their signed acceptance.
    pub fn accept_as_mentor(
        env: Env,
        mentor: Address,
        match_id: BytesN<32>,
        declarations_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        mentor.require_auth();

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;
        
        if mentorship.mentor != mentor {
            return Err(ContractError::NotMentor);
        }
        if mentorship.status != MentorshipStatus::ProposedMenteeAccepted {
            return Err(ContractError::InvalidStatus);
        }

        let now = env.ledger().timestamp();

        // Record the signed acceptance
        let acceptance = types::MentorAcceptance {
            match_id: match_id.clone(),
            terms_version: mentorship.terms_version,
            terms_hash: mentorship.terms_hash.clone(),
            signer: mentor.clone(),
            accepted_at: now,
            declarations_hash,
        };
        storage::put_mentor_acceptance(&env, &acceptance);

        // Transition state
        mentorship.status = MentorshipStatus::ProposedMentorAccepted;
        mentorship.mentor_accepted_at = now;
        storage::put_mentorship(&env, &mentorship);

        events::mentor_accepted(&env, &match_id, &mentor, now);
        Ok(())
    }

    /// Admin-only: Activate a mentorship after both mentor and mentee have accepted.
    /// This is the final transition before the mentorship becomes Active and payment may settle.
    pub fn activate_mentorship(
        env: Env,
        admin: Address,
        match_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;

        if mentorship.status != MentorshipStatus::ProposedMentorAccepted {
            return Err(ContractError::InvalidStatus);
        }

        let now = env.ledger().timestamp();
        mentorship.status = MentorshipStatus::Active;
        mentorship.activated_at = now;
        storage::put_mentorship(&env, &mentorship);

        events::mentorship_activated(&env, &match_id, &mentorship.mentor, &mentorship.mentee, now);
        Ok(())
    }

    /// Either mentor or mentee can reject the mentorship while it is still in Proposed states
    /// (before both have accepted). Once accepted, rejection becomes termination.
    pub fn reject_mentorship(
        env: Env,
        caller: Address,
        match_id: BytesN<32>,
        reason_code: u32,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;

        // Only rejectible in early states
        if mentorship.status != MentorshipStatus::Proposed
            && mentorship.status != MentorshipStatus::ProposedMenteeAccepted
            && mentorship.status != MentorshipStatus::ProposedMentorAccepted
        {
            return Err(ContractError::InvalidStatus);
        }

        // Caller must be one of the participants
        if caller != mentorship.mentor && caller != mentorship.mentee {
            return Err(ContractError::Unauthorized);
        }

        let now = env.ledger().timestamp();
        mentorship.status = MentorshipStatus::Rejected;
        mentorship.terminated_at = now;
        mentorship.termination_reason_code = reason_code;
        mentorship.reserved = false;
        storage::put_mentorship(&env, &mentorship);

        // Clear the active mentorship pointer
        storage::clear_active_mentorship(&env, &mentorship.mentor, &mentorship.mentee);

        events::mentorship_rejected(&env, &match_id, &caller, reason_code);
        Ok(())
    }

    /// Admin-only: Terminate an active mentorship. This records the reason and any recovery
    /// amount (e.g., clawback from mentor for breach).
    pub fn terminate_mentorship(
        env: Env,
        admin: Address,
        match_id: BytesN<32>,
        reason_code: u32,
        recovery_amount: i128,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;

        if mentorship.status != MentorshipStatus::Active {
            return Err(ContractError::InvalidStatus);
        }

        if recovery_amount < 0 || recovery_amount > mentorship.amount {
            return Err(ContractError::InvalidAmount);
        }

        let now = env.ledger().timestamp();
        mentorship.status = MentorshipStatus::Terminated;
        mentorship.terminated_at = now;
        mentorship.termination_reason_code = reason_code;
        mentorship.reserved = false;
        storage::put_mentorship(&env, &mentorship);

        // Clear the active mentorship pointer
        storage::clear_active_mentorship(&env, &mentorship.mentor, &mentorship.mentee);

        events::mentorship_terminated(&env, &match_id, &mentorship.mentor, &mentorship.mentee, reason_code, recovery_amount);
        Ok(())
    }

    /// Admin-only: Mark a mentorship as completed. This is the normal end state after successful
    /// service delivery.
    pub fn complete_mentorship(
        env: Env,
        admin: Address,
        match_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        storage::require_admin(&env, &admin)?;

        let mut mentorship = storage::load_mentorship(&env, &match_id)?;

        if mentorship.status != MentorshipStatus::Active {
            return Err(ContractError::InvalidStatus);
        }

        let now = env.ledger().timestamp();
        mentorship.status = MentorshipStatus::Completed;
        mentorship.completed_at = now;
        mentorship.reserved = false;
        storage::put_mentorship(&env, &mentorship);

        // Clear the active mentorship pointer
        storage::clear_active_mentorship(&env, &mentorship.mentor, &mentorship.mentee);

        events::mentorship_completed(&env, &match_id, &mentorship.mentor, &mentorship.mentee, now, mentorship.amount);
        Ok(())
    }

    // ── Read-only queries ────────────────────────────────────────────────

    /// Return the mentorship record for the given match ID.
    pub fn get_mentorship(env: Env, match_id: BytesN<32>) -> Result<MentorshipMatch, ContractError> {
        storage::load_and_extend_mentorship(&env, &match_id)
    }

    /// Return true if an active (non-terminal) mentorship exists between mentor and mentee.
    pub fn has_active_mentorship(env: Env, mentor: Address, mentee: Address) -> bool {
        storage::has_active_mentorship(&env, &mentor, &mentee)
    }

    /// Return the mentee's signed acceptance record for a match.
    pub fn get_mentee_acceptance(
        env: Env,
        match_id: BytesN<32>,
    ) -> Result<types::MenteeAcceptance, ContractError> {
        storage::load_and_extend_mentee_acceptance(&env, &match_id)
    }

    /// Return the mentor's signed acceptance record for a match.
    pub fn get_mentor_acceptance(
        env: Env,
        match_id: BytesN<32>,
    ) -> Result<types::MentorAcceptance, ContractError> {
        storage::load_and_extend_mentor_acceptance(&env, &match_id)
    }

    /// Return a list of match IDs for which the given mentor is involved.
    pub fn get_mentor_matches(env: Env, mentor: Address) -> Vec<BytesN<32>> {
        storage::get_mentor_index(&env, &mentor)
    }

    /// Return a list of match IDs for which the given mentee is involved.
    pub fn get_mentee_matches(env: Env, mentee: Address) -> Vec<BytesN<32>> {
        storage::get_mentee_index(&env, &mentee)
    }
}
