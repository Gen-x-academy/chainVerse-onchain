use soroban_sdk::{Address, BytesN, Env};

use crate::{
    blocking::check_matching_allowed,
    errors::MentorshipError,
    events,
    storage::{
        get_acceptance, get_agreement, increment_agreement_id, set_acceptance, set_agreement,
        increment_user_nonce,
    },
    types::{Acceptance, Agreement, AgreementStatus, MatchingAllowed},
};

/// Create a new mentorship agreement (MENTOR-045)
#[allow(clippy::too_many_arguments)]
pub fn create_agreement(
    env: &Env,
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
    // Authorization: require both mentor and mentee to authenticate
    mentor.require_auth();
    mentee.require_auth();

    // Validation: participants must be different
    if mentor == mentee {
        return Err(MentorshipError::SameParticipant);
    }

    // Validation: check for blocking (MENTOR-044 integration)
    let matching = check_matching_allowed(env, &mentor, &mentee);
    if matching != MatchingAllowed::Allowed {
        return Err(MentorshipError::UsersBlocked);
    }

    // Validation: time range must be valid
    if start_time >= end_time {
        return Err(MentorshipError::InvalidTimeRange);
    }

    // Validation: cadence must be positive
    if cadence_seconds == 0 {
        return Err(MentorshipError::InvalidCadence);
    }

    // Validation: price must be non-negative
    if price_per_session < 0 {
        return Err(MentorshipError::InvalidPrice);
    }

    // Generate agreement ID
    let agreement_id = increment_agreement_id(env);
    let current_time = env.ledger().timestamp();

    // Create agreement
    let agreement = Agreement {
        id: agreement_id,
        mentor: mentor.clone(),
        mentee: mentee.clone(),
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
        status: AgreementStatus::Pending,
        created_at: current_time,
        updated_at: current_time,
    };

    // Store agreement
    set_agreement(env, &agreement);

    // Emit event
    events::agreement_created(env, agreement_id, &mentor, &mentee, start_time, end_time);

    Ok(agreement_id)
}

/// Accept an agreement (MENTOR-046)
pub fn accept_agreement(
    env: &Env,
    actor: Address,
    agreement_id: u64,
    agreement_hash: BytesN<32>,
    expected_nonce: u64,
) -> Result<(), MentorshipError> {
    // Authorization: require actor to authenticate
    actor.require_auth();

    // Load agreement
    let mut agreement = get_agreement(env, agreement_id)?;

    // Validation: agreement must be pending or active
    if agreement.status != AgreementStatus::Pending && agreement.status != AgreementStatus::Active {
        return Err(MentorshipError::InvalidStatus);
    }

    // Validation: actor must be a participant
    if actor != agreement.mentor && actor != agreement.mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Check if already accepted
    if get_acceptance(env, agreement_id, &actor).is_some() {
        return Err(MentorshipError::AlreadyAccepted);
    }

    // Replay protection: verify and increment nonce
    let current_nonce = increment_user_nonce(env, &actor);
    if current_nonce != expected_nonce {
        return Err(MentorshipError::InvalidNonce);
    }

    // Create acceptance record
    let acceptance = Acceptance {
        actor: actor.clone(),
        agreement_id,
        agreement_hash: agreement_hash.clone(),
        terms_version: agreement.terms_version,
        timestamp: env.ledger().timestamp(),
        nonce: current_nonce,
    };

    // Store acceptance
    set_acceptance(env, &acceptance);

    // Emit acceptance event
    events::agreement_accepted(env, agreement_id, &actor, &agreement_hash, current_nonce);

    // Check if both parties have now accepted
    let mentor_accepted = get_acceptance(env, agreement_id, &agreement.mentor).is_some();
    let mentee_accepted = get_acceptance(env, agreement_id, &agreement.mentee).is_some();

    // If both accepted and still pending, activate the agreement
    if mentor_accepted && mentee_accepted && agreement.status == AgreementStatus::Pending {
        let old_status = agreement.status;
        agreement.status = AgreementStatus::Active;
        agreement.updated_at = env.ledger().timestamp();
        set_agreement(env, &agreement);

        // Emit status change event
        events::agreement_status_changed(env, agreement_id, old_status, AgreementStatus::Active);
    }

    Ok(())
}

/// Get agreement details
pub fn get_agreement_info(env: &Env, agreement_id: u64) -> Result<Agreement, MentorshipError> {
    get_agreement(env, agreement_id)
}

/// Check if a party has accepted an agreement
pub fn has_accepted(env: &Env, agreement_id: u64, actor: &Address) -> bool {
    get_acceptance(env, agreement_id, actor).is_some()
}

/// Cancel an agreement (requires both parties or admin)
pub fn cancel_agreement(
    env: &Env,
    caller: Address,
    agreement_id: u64,
) -> Result<(), MentorshipError> {
    // Authorization
    caller.require_auth();

    // Load agreement
    let mut agreement = get_agreement(env, agreement_id)?;

    // Validation: must be active or pending
    if agreement.status != AgreementStatus::Active && agreement.status != AgreementStatus::Pending {
        return Err(MentorshipError::InvalidStatus);
    }

    // Authorization: caller must be a participant
    if caller != agreement.mentor && caller != agreement.mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Update status
    let old_status = agreement.status;
    agreement.status = AgreementStatus::Cancelled;
    agreement.updated_at = env.ledger().timestamp();
    set_agreement(env, &agreement);

    // Emit event
    events::agreement_status_changed(env, agreement_id, old_status, AgreementStatus::Cancelled);

    Ok(())
}

/// Complete an agreement
pub fn complete_agreement(
    env: &Env,
    caller: Address,
    agreement_id: u64,
) -> Result<(), MentorshipError> {
    // Authorization
    caller.require_auth();

    // Load agreement
    let mut agreement = get_agreement(env, agreement_id)?;

    // Validation: must be active
    if agreement.status != AgreementStatus::Active {
        return Err(MentorshipError::InvalidStatus);
    }

    // Authorization: caller must be a participant
    if caller != agreement.mentor && caller != agreement.mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Update status
    let old_status = agreement.status;
    agreement.status = AgreementStatus::Completed;
    agreement.updated_at = env.ledger().timestamp();
    set_agreement(env, &agreement);

    // Emit event
    events::agreement_status_changed(env, agreement_id, old_status, AgreementStatus::Completed);

    Ok(())
}
