use soroban_sdk::{Address, BytesN, Env};

use crate::{
    errors::MentorshipError,
    events,
    storage::{
        get_agreement, get_amendment, increment_amendment_id, set_agreement, set_amendment,
    },
    types::{Agreement, Amendment, AgreementStatus, AmendmentStatus},
};

/// Propose an amendment to an active agreement (MENTOR-048)
#[allow(clippy::too_many_arguments)]
pub fn propose_amendment(
    env: &Env,
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
    // Authorization
    proposer.require_auth();

    // Load agreement
    let agreement = get_agreement(env, agreement_id)?;

    // Validation: agreement must be active
    if agreement.status != AgreementStatus::Active {
        return Err(MentorshipError::MustBeActive);
    }

    // Validation: proposer must be a participant
    if proposer != agreement.mentor && proposer != agreement.mentee {
        return Err(MentorshipError::InvalidProposer);
    }

    // Validation: at least one change must be proposed
    if new_cadence.is_none()
        && new_scope_hash.is_none()
        && new_start_time.is_none()
        && new_end_time.is_none()
        && new_price.is_none()
        && new_cancellation_hash.is_none()
    {
        return Err(MentorshipError::NoChanges);
    }

    // Validation: if price is being changed, it must be non-negative
    if let Some(price) = new_price {
        if price < 0 {
            return Err(MentorshipError::InvalidPrice);
        }
    }

    // Validation: if cadence is being changed, it must be positive
    if let Some(cadence) = new_cadence {
        if cadence == 0 {
            return Err(MentorshipError::InvalidCadence);
        }
    }

    // Validation: if time range is being changed, ensure it's valid
    let start = new_start_time.unwrap_or(agreement.start_time);
    let end = new_end_time.unwrap_or(agreement.end_time);
    if start >= end {
        return Err(MentorshipError::InvalidTimeRange);
    }

    // Generate amendment ID
    let amendment_id = increment_amendment_id(env);

    // Create amendment
    // Proposer auto-approves their own proposal
    let is_mentor = proposer == agreement.mentor;
    let amendment = Amendment {
        id: amendment_id,
        agreement_id,
        proposer: proposer.clone(),
        new_cadence,
        new_scope_hash,
        new_start_time,
        new_end_time,
        new_price,
        new_cancellation_hash,
        impact_hash: impact_hash.clone(),
        proposed_at: env.ledger().timestamp(),
        status: AmendmentStatus::Proposed,
        mentor_approved: is_mentor,
        mentee_approved: !is_mentor,
        activated_at: None,
    };

    // Store amendment
    set_amendment(env, &amendment);

    // Emit event
    events::amendment_proposed(env, amendment_id, agreement_id, &proposer, &impact_hash);

    Ok(amendment_id)
}

/// Approve an amendment
pub fn approve_amendment(
    env: &Env,
    approver: Address,
    amendment_id: u64,
) -> Result<(), MentorshipError> {
    // Authorization
    approver.require_auth();

    // Load amendment
    let mut amendment = get_amendment(env, amendment_id)?;

    // Validation: amendment must be proposed
    if amendment.status != AmendmentStatus::Proposed {
        return Err(MentorshipError::AmendmentAlreadyProcessed);
    }

    // Load associated agreement
    let agreement = get_agreement(env, amendment.agreement_id)?;

    // Validation: agreement must still be active
    if agreement.status != AgreementStatus::Active {
        return Err(MentorshipError::MustBeActive);
    }

    // Validation: approver must be a participant who hasn't approved yet
    let is_mentor = approver == agreement.mentor;
    let is_mentee = approver == agreement.mentee;

    if !is_mentor && !is_mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Check if already approved
    if (is_mentor && amendment.mentor_approved) || (is_mentee && amendment.mentee_approved) {
        return Err(MentorshipError::AlreadyAccepted);
    }

    // Record approval
    if is_mentor {
        amendment.mentor_approved = true;
    } else {
        amendment.mentee_approved = true;
    }

    // Check if fully approved
    let fully_approved = amendment.mentor_approved && amendment.mentee_approved;

    if fully_approved {
        amendment.status = AmendmentStatus::Approved;
    }

    // Store updated amendment
    set_amendment(env, &amendment);

    // Emit event
    events::amendment_approved(env, amendment_id, &approver, fully_approved);

    Ok(())
}

/// Reject an amendment
pub fn reject_amendment(
    env: &Env,
    rejector: Address,
    amendment_id: u64,
) -> Result<(), MentorshipError> {
    // Authorization
    rejector.require_auth();

    // Load amendment
    let mut amendment = get_amendment(env, amendment_id)?;

    // Validation: amendment must be proposed
    if amendment.status != AmendmentStatus::Proposed {
        return Err(MentorshipError::AmendmentAlreadyProcessed);
    }

    // Load associated agreement
    let agreement = get_agreement(env, amendment.agreement_id)?;

    // Validation: rejector must be a participant
    if rejector != agreement.mentor && rejector != agreement.mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Update status
    amendment.status = AmendmentStatus::Rejected;
    set_amendment(env, &amendment);

    // Emit event
    events::amendment_rejected(env, amendment_id, &rejector);

    Ok(())
}

/// Activate an approved amendment (MENTOR-048: Old terms govern until activation)
pub fn activate_amendment(
    env: &Env,
    caller: Address,
    amendment_id: u64,
) -> Result<(), MentorshipError> {
    // Authorization
    caller.require_auth();

    // Load amendment
    let mut amendment = get_amendment(env, amendment_id)?;

    // Validation: amendment must be approved
    if amendment.status != AmendmentStatus::Approved {
        return Err(MentorshipError::RequiresApproval);
    }

    // Load agreement
    let mut agreement = get_agreement(env, amendment.agreement_id)?;

    // Validation: agreement must still be active
    if agreement.status != AgreementStatus::Active {
        return Err(MentorshipError::MustBeActive);
    }

    // Validation: caller must be a participant
    if caller != agreement.mentor && caller != agreement.mentee {
        return Err(MentorshipError::Unauthorized);
    }

    // Apply changes to agreement
    if let Some(cadence) = amendment.new_cadence {
        agreement.cadence_seconds = cadence;
    }
    if let Some(scope_hash) = amendment.new_scope_hash.clone() {
        agreement.scope_hash = scope_hash;
    }
    if let Some(start_time) = amendment.new_start_time {
        agreement.start_time = start_time;
    }
    if let Some(end_time) = amendment.new_end_time {
        agreement.end_time = end_time;
    }
    if let Some(price) = amendment.new_price {
        agreement.price_per_session = price;
    }
    if let Some(cancellation_hash) = amendment.new_cancellation_hash.clone() {
        agreement.cancellation_policy_hash = cancellation_hash;
    }

    // Update agreement timestamp
    agreement.updated_at = env.ledger().timestamp();
    set_agreement(env, &agreement);

    // Update amendment status
    let activated_at = env.ledger().timestamp();
    amendment.status = AmendmentStatus::Activated;
    amendment.activated_at = Some(activated_at);
    set_amendment(env, &amendment);

    // Emit event
    events::amendment_activated(env, amendment_id, agreement.id, activated_at);

    Ok(())
}

/// Get amendment details
pub fn get_amendment_info(env: &Env, amendment_id: u64) -> Result<Amendment, MentorshipError> {
    get_amendment(env, amendment_id)
}
