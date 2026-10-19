use crate::types::{EngagementStatus, FeedbackType, PauseReason, TerminationReason};
use soroban_sdk::{symbol_short, Address, BytesN, Env};

/// Emitted when a new engagement is created
pub fn engagement_created(
    env: &Env,
    engagement_id: u64,
    mentor: &Address,
    mentee: &Address,
    requires_consent: bool,
) {
    env.events().publish(
        (symbol_short!("ENG_NEW"),),
        (engagement_id, mentor, mentee, requires_consent),
    );
}

/// Emitted when guardian consent is granted
pub fn guardian_consent_granted(
    env: &Env,
    engagement_id: u64,
    guardian: &Address,
    mentee: &Address,
    terms_hash: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("CONSENT"),),
        (engagement_id, guardian, mentee, terms_hash),
    );
}

/// Emitted when guardian consent is revoked
pub fn guardian_consent_revoked(
    env: &Env,
    engagement_id: u64,
    guardian: &Address,
    reason_hash: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("CNS_REVK"),),
        (engagement_id, guardian, reason_hash),
    );
}

/// Emitted when consent needs to be renewed due to material changes
pub fn consent_renewal_required(
    env: &Env,
    engagement_id: u64,
    change_hash: &BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("CNS_RENW"),),
        (engagement_id, change_hash),
    );
}

/// Emitted when an engagement is activated (consent obtained if needed)
pub fn engagement_activated(env: &Env, engagement_id: u64) {
    env.events().publish(
        (symbol_short!("ENG_ACT"),),
        (engagement_id,),
    );
}

/// Emitted when a pause is requested
pub fn pause_requested(
    env: &Env,
    engagement_id: u64,
    requester: &Address,
    reason: PauseReason,
    effective_date: u64,
) {
    env.events().publish(
        (symbol_short!("PAUSE_RQ"),),
        (engagement_id, requester, reason, effective_date),
    );
}

/// Emitted when a pause is approved
pub fn pause_approved(
    env: &Env,
    engagement_id: u64,
    approver: &Address,
) {
    env.events().publish(
        (symbol_short!("PAUSE_OK"),),
        (engagement_id, approver),
    );
}

/// Emitted when an engagement is paused
pub fn engagement_paused(
    env: &Env,
    engagement_id: u64,
    reason: PauseReason,
) {
    env.events().publish(
        (symbol_short!("PAUSED"),),
        (engagement_id, reason),
    );
}

/// Emitted when an engagement is resumed
pub fn engagement_resumed(env: &Env, engagement_id: u64) {
    env.events().publish(
        (symbol_short!("RESUMED"),),
        (engagement_id,),
    );
}

/// Emitted when an engagement is terminated
pub fn engagement_terminated(
    env: &Env,
    engagement_id: u64,
    reason: TerminationReason,
    initiated_by: &Address,
) {
    env.events().publish(
        (symbol_short!("TERM"),),
        (engagement_id, reason, initiated_by),
    );
}

/// Emitted when an engagement is completed successfully
pub fn engagement_completed(
    env: &Env,
    engagement_id: u64,
    total_sessions: u32,
) {
    env.events().publish(
        (symbol_short!("COMPLETE"),),
        (engagement_id, total_sessions),
    );
}

/// Emitted when matching feedback is submitted
pub fn feedback_submitted(
    env: &Env,
    feedback_id: u64,
    user: &Address,
    feedback_type: FeedbackType,
) {
    env.events().publish(
        (symbol_short!("FEEDBACK"),),
        (feedback_id, user, feedback_type),
    );
}

/// Emitted when a session is recorded
pub fn session_recorded(
    env: &Env,
    engagement_id: u64,
    session_number: u32,
) {
    env.events().publish(
        (symbol_short!("SESSION"),),
        (engagement_id, session_number),
    );
}

/// Emitted when admin changes configuration
pub fn config_updated(
    env: &Env,
    admin: &Address,
    config_key: &str,
) {
    env.events().publish(
        (symbol_short!("CFG_UPD"),),
        (admin, config_key),
    );
}
