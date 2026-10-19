use crate::errors::MentorshipError;
use crate::types::{Engagement, GuardianConsent, MatchingFeedback, TerminationOutcome};
use soroban_sdk::{Address, Env};

// Storage keys
#[derive(Clone)]
pub enum DataKey {
    /// Contract admin
    Admin,
    /// Next engagement ID counter
    NextEngagementId,
    /// Engagement by ID
    Engagement(u64),
    /// Guardian consent by (engagement_id, guardian)
    GuardianConsent(u64),
    /// Next feedback ID counter
    NextFeedbackId,
    /// Feedback by ID
    Feedback(u64),
    /// Termination outcome by engagement ID
    TerminationOutcome(u64),
    /// Inactivity threshold (in seconds)
    InactivityThreshold,
    /// Pause approval required flag
    PauseRequiresMutualApproval,
}

// Admin functions
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), MentorshipError> {
    let admin = get_admin(env).ok_or(MentorshipError::NotInitialized)?;
    if caller != &admin {
        return Err(MentorshipError::NotAdmin);
    }
    caller.require_auth();
    Ok(())
}

// Engagement ID management
pub fn get_next_engagement_id(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::NextEngagementId)
        .unwrap_or(1)
}

pub fn increment_engagement_id(env: &Env) -> u64 {
    let next_id = get_next_engagement_id(env);
    env.storage()
        .instance()
        .set(&DataKey::NextEngagementId, &(next_id + 1));
    next_id
}

// Engagement storage
pub fn get_engagement(env: &Env, engagement_id: u64) -> Result<Engagement, MentorshipError> {
    env.storage()
        .persistent()
        .get(&DataKey::Engagement(engagement_id))
        .ok_or(MentorshipError::EngagementNotFound)
}

pub fn set_engagement(env: &Env, engagement: &Engagement) {
    env.storage()
        .persistent()
        .set(&DataKey::Engagement(engagement.id), engagement);
    // Extend TTL for 90 days
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::Engagement(engagement.id), 7776000, 7776000);
}

pub fn has_engagement(env: &Env, engagement_id: u64) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Engagement(engagement_id))
}

// Guardian consent storage
pub fn get_guardian_consent(
    env: &Env,
    engagement_id: u64,
) -> Result<GuardianConsent, MentorshipError> {
    env.storage()
        .persistent()
        .get(&DataKey::GuardianConsent(engagement_id))
        .ok_or(MentorshipError::ConsentNotFound)
}

pub fn set_guardian_consent(env: &Env, consent: &GuardianConsent) {
    env.storage()
        .persistent()
        .set(&DataKey::GuardianConsent(consent.engagement_id), consent);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::GuardianConsent(consent.engagement_id), 7776000, 7776000);
}

// Feedback ID management
pub fn get_next_feedback_id(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::NextFeedbackId)
        .unwrap_or(1)
}

pub fn increment_feedback_id(env: &Env) -> u64 {
    let next_id = get_next_feedback_id(env);
    env.storage()
        .instance()
        .set(&DataKey::NextFeedbackId, &(next_id + 1));
    next_id
}

// Feedback storage
pub fn get_feedback(env: &Env, feedback_id: u64) -> Result<MatchingFeedback, MentorshipError> {
    env.storage()
        .persistent()
        .get(&DataKey::Feedback(feedback_id))
        .ok_or(MentorshipError::FeedbackNotFound)
}

pub fn set_feedback(env: &Env, feedback: &MatchingFeedback) {
    env.storage()
        .persistent()
        .set(&DataKey::Feedback(feedback.id), feedback);
    env.storage()
        .persistent()
        .extend_ttl(&DataKey::Feedback(feedback.id), 7776000, 7776000);
}

// Termination outcome storage
pub fn get_termination_outcome(
    env: &Env,
    engagement_id: u64,
) -> Option<TerminationOutcome> {
    env.storage()
        .persistent()
        .get(&DataKey::TerminationOutcome(engagement_id))
}

pub fn set_termination_outcome(env: &Env, outcome: &TerminationOutcome) {
    env.storage()
        .persistent()
        .set(&DataKey::TerminationOutcome(outcome.engagement_id), outcome);
    env.storage()
        .persistent()
        .extend_ttl(
            &DataKey::TerminationOutcome(outcome.engagement_id),
            7776000,
            7776000,
        );
}

// Configuration
pub fn get_inactivity_threshold(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::InactivityThreshold)
        .unwrap_or(2592000) // Default: 30 days in seconds
}

pub fn set_inactivity_threshold(env: &Env, threshold: u64) {
    env.storage()
        .instance()
        .set(&DataKey::InactivityThreshold, &threshold);
}

pub fn get_pause_requires_mutual_approval(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::PauseRequiresMutualApproval)
        .unwrap_or(true)
}

pub fn set_pause_requires_mutual_approval(env: &Env, required: bool) {
    env.storage()
        .instance()
        .set(&DataKey::PauseRequiresMutualApproval, &required);
}
