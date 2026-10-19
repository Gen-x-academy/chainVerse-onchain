#![no_std]

mod errors;
mod events;
mod storage;
mod types;

pub use errors::MentorshipError;
pub use types::*;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct MentorshipContract;

#[contractimpl]
impl MentorshipContract {
    /// Initialize the contract with an admin
    pub fn init(env: Env, admin: Address) -> Result<(), MentorshipError> {
        if storage::get_admin(&env).is_some() {
            return Err(MentorshipError::AlreadyInitialized);
        }
        admin.require_auth();
        storage::set_admin(&env, &admin);
        Ok(())
    }

    /// Set a new admin (only callable by current admin)
    pub fn set_admin(env: Env, new_admin: Address) -> Result<(), MentorshipError> {
        let current_admin = storage::get_admin(&env).ok_or(MentorshipError::NotInitialized)?;
        current_admin.require_auth();
        storage::set_admin(&env, &new_admin);
        events::config_updated(&env, &current_admin, "admin");
        Ok(())
    }

    /// Configure inactivity threshold (in seconds)
    pub fn set_inactivity_threshold(
        env: Env,
        admin: Address,
        threshold: u64,
    ) -> Result<(), MentorshipError> {
        storage::require_admin(&env, &admin)?;
        storage::set_inactivity_threshold(&env, threshold);
        events::config_updated(&env, &admin, "inactivity_threshold");
        Ok(())
    }

    /// Configure whether pause requests require mutual approval
    pub fn set_pause_approval_policy(
        env: Env,
        admin: Address,
        requires_mutual: bool,
    ) -> Result<(), MentorshipError> {
        storage::require_admin(&env, &admin)?;
        storage::set_pause_requires_mutual_approval(&env, requires_mutual);
        events::config_updated(&env, &admin, "pause_approval_policy");
        Ok(())
    }

    // ========================================================================
    // ENGAGEMENT LIFECYCLE (MENTOR-050)
    // ========================================================================

    /// Create a new mentorship engagement
    /// If requires_guardian_consent is true, engagement starts in PendingConsent status
    pub fn create_engagement(
        env: Env,
        mentor: Address,
        mentee: Address,
        agreement_hash: BytesN<32>,
        payment_per_session: i128,
        payment_token: Address,
        planned_sessions: u32,
        requires_guardian_consent: bool,
    ) -> Result<u64, MentorshipError> {
        // Both parties must authorize
        mentor.require_auth();
        mentee.require_auth();

        if payment_per_session <= 0 {
            return Err(MentorshipError::InvalidAmount);
        }

        if planned_sessions == 0 {
            return Err(MentorshipError::InvalidSessionCount);
        }

        let engagement_id = storage::increment_engagement_id(&env);
        let now = env.ledger().timestamp();

        let status = if requires_guardian_consent {
            EngagementStatus::PendingConsent
        } else {
            EngagementStatus::Active
        };

        let engagement = Engagement {
            id: engagement_id,
            mentor: mentor.clone(),
            mentee: mentee.clone(),
            status: status.clone(),
            agreement_hash: agreement_hash.clone(),
            payment_per_session,
            payment_token,
            created_at: now,
            last_activity_at: now,
            requires_guardian_consent,
            guardian_consent: None,
            active_pause: None,
            completed_sessions: 0,
            planned_sessions,
        };

        storage::set_engagement(&env, &engagement);

        events::engagement_created(
            &env,
            engagement_id,
            &mentor,
            &mentee,
            requires_guardian_consent,
        );

        if status == EngagementStatus::Active {
            events::engagement_activated(&env, engagement_id);
        }

        Ok(engagement_id)
    }

    /// Record a completed session
    pub fn record_session(
        env: Env,
        caller: Address,
        engagement_id: u64,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Only participants can record sessions
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        // Must be active
        if engagement.status != EngagementStatus::Active {
            return Err(MentorshipError::InvalidStatus);
        }

        engagement.completed_sessions += 1;
        engagement.last_activity_at = env.ledger().timestamp();

        events::session_recorded(&env, engagement_id, engagement.completed_sessions);

        // Auto-complete if all sessions done
        if engagement.completed_sessions >= engagement.planned_sessions {
            engagement.status = EngagementStatus::Completed;
            let outcome = TerminationOutcome {
                engagement_id,
                reason: TerminationReason::Completed,
                terminated_at: env.ledger().timestamp(),
                initiated_by: caller,
                outcome_hash: engagement.agreement_hash.clone(),
                final_session_count: engagement.completed_sessions,
                settlement_complete: false,
            };
            storage::set_termination_outcome(&env, &outcome);
            events::engagement_completed(&env, engagement_id, engagement.completed_sessions);
        }

        storage::set_engagement(&env, &engagement);
        Ok(())
    }

    /// Complete an engagement early by mutual agreement
    pub fn complete_engagement(
        env: Env,
        caller: Address,
        engagement_id: u64,
        outcome_hash: BytesN<32>,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Must be a participant
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        // Must be active or paused
        if engagement.status != EngagementStatus::Active
            && engagement.status != EngagementStatus::Paused
        {
            return Err(MentorshipError::InvalidStatus);
        }

        engagement.status = EngagementStatus::Completed;

        let outcome = TerminationOutcome {
            engagement_id,
            reason: TerminationReason::Completed,
            terminated_at: env.ledger().timestamp(),
            initiated_by: caller.clone(),
            outcome_hash,
            final_session_count: engagement.completed_sessions,
            settlement_complete: false,
        };

        storage::set_engagement(&env, &engagement);
        storage::set_termination_outcome(&env, &outcome);

        events::engagement_completed(&env, engagement_id, engagement.completed_sessions);

        Ok(())
    }

    /// Terminate an engagement (mutual, policy, or safeguarding)
    pub fn terminate_engagement(
        env: Env,
        caller: Address,
        engagement_id: u64,
        reason: TerminationReason,
        outcome_hash: BytesN<32>,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Authorization logic depends on reason
        match reason {
            TerminationReason::Mutual => {
                // Must be a participant
                if caller != engagement.mentor && caller != engagement.mentee {
                    return Err(MentorshipError::NotParticipant);
                }
            }
            TerminationReason::PolicyViolation | TerminationReason::Safeguarding => {
                // Only admin can terminate for policy/safeguarding
                storage::require_admin(&env, &caller)?;
            }
            TerminationReason::ConsentRevoked => {
                // Will be handled by revoke_guardian_consent
                return Err(MentorshipError::Unauthorized);
            }
            TerminationReason::Inactivity => {
                // Anyone can trigger inactivity check
                let threshold = storage::get_inactivity_threshold(&env);
                let now = env.ledger().timestamp();
                if now < engagement.last_activity_at + threshold {
                    return Err(MentorshipError::InvalidStatus);
                }
            }
            _ => {}
        }

        let term_status = if reason == TerminationReason::Safeguarding {
            EngagementStatus::SafeguardingTermination
        } else {
            EngagementStatus::Terminated
        };

        engagement.status = term_status;

        let outcome = TerminationOutcome {
            engagement_id,
            reason: reason.clone(),
            terminated_at: env.ledger().timestamp(),
            initiated_by: caller.clone(),
            outcome_hash,
            final_session_count: engagement.completed_sessions,
            settlement_complete: false,
        };

        storage::set_engagement(&env, &engagement);
        storage::set_termination_outcome(&env, &outcome);

        events::engagement_terminated(&env, engagement_id, reason, &caller);

        Ok(())
    }

    // ========================================================================
    // GUARDIAN CONSENT (MENTOR-047)
    // ========================================================================

    /// Grant guardian consent for a mentorship engagement
    pub fn grant_guardian_consent(
        env: Env,
        guardian: Address,
        engagement_id: u64,
        terms_hash: BytesN<32>,
        expires_at: Option<u64>,
    ) -> Result<(), MentorshipError> {
        guardian.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        if !engagement.requires_guardian_consent {
            return Err(MentorshipError::ConsentRequired);
        }

        if engagement.status != EngagementStatus::PendingConsent {
            return Err(MentorshipError::InvalidStatus);
        }

        let now = env.ledger().timestamp();

        // Validate expiry if provided
        if let Some(expiry) = expires_at {
            if expiry <= now {
                return Err(MentorshipError::InvalidTimestamp);
            }
        }

        let consent = GuardianConsent {
            guardian: guardian.clone(),
            mentee: engagement.mentee.clone(),
            engagement_id,
            terms_hash: terms_hash.clone(),
            granted_at: now,
            expires_at,
            is_active: true,
            last_material_change: None,
        };

        engagement.guardian_consent = Some(consent.clone());
        engagement.status = EngagementStatus::Active;
        engagement.last_activity_at = now;

        storage::set_guardian_consent(&env, &consent);
        storage::set_engagement(&env, &engagement);

        events::guardian_consent_granted(&env, engagement_id, &guardian, &engagement.mentee, &terms_hash);
        events::engagement_activated(&env, engagement_id);

        Ok(())
    }

    /// Revoke guardian consent
    pub fn revoke_guardian_consent(
        env: Env,
        guardian: Address,
        engagement_id: u64,
        reason_hash: BytesN<32>,
    ) -> Result<(), MentorshipError> {
        guardian.require_auth();

        let mut consent = storage::get_guardian_consent(&env, engagement_id)?;

        if guardian != consent.guardian {
            return Err(MentorshipError::NotGuardian);
        }

        if !consent.is_active {
            return Err(MentorshipError::ConsentNotActive);
        }

        consent.is_active = false;
        storage::set_guardian_consent(&env, &consent);

        // Terminate the engagement
        let mut engagement = storage::get_engagement(&env, engagement_id)?;
        engagement.status = EngagementStatus::Terminated;

        let outcome = TerminationOutcome {
            engagement_id,
            reason: TerminationReason::ConsentRevoked,
            terminated_at: env.ledger().timestamp(),
            initiated_by: guardian.clone(),
            outcome_hash: reason_hash.clone(),
            final_session_count: engagement.completed_sessions,
            settlement_complete: false,
        };

        storage::set_engagement(&env, &engagement);
        storage::set_termination_outcome(&env, &outcome);

        events::guardian_consent_revoked(&env, engagement_id, &guardian, &reason_hash);
        events::engagement_terminated(&env, engagement_id, TerminationReason::ConsentRevoked, &guardian);

        Ok(())
    }

    /// Mark material change requiring re-consent
    pub fn mark_material_change(
        env: Env,
        caller: Address,
        engagement_id: u64,
        change_hash: BytesN<32>,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let engagement = storage::get_engagement(&env, engagement_id)?;

        // Must be participant
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        if !engagement.requires_guardian_consent {
            return Err(MentorshipError::ConsentRequired);
        }

        let mut consent = storage::get_guardian_consent(&env, engagement_id)?;
        consent.last_material_change = Some(env.ledger().timestamp());
        storage::set_guardian_consent(&env, &consent);

        events::consent_renewal_required(&env, engagement_id, &change_hash);

        Ok(())
    }

    /// Re-consent after material change
    pub fn reconsent(
        env: Env,
        guardian: Address,
        engagement_id: u64,
        new_terms_hash: BytesN<32>,
    ) -> Result<(), MentorshipError> {
        guardian.require_auth();

        let mut consent = storage::get_guardian_consent(&env, engagement_id)?;

        if guardian != consent.guardian {
            return Err(MentorshipError::NotGuardian);
        }

        if consent.last_material_change.is_none() {
            return Err(MentorshipError::MaterialChangeRequiresReconsent);
        }

        consent.terms_hash = new_terms_hash.clone();
        consent.granted_at = env.ledger().timestamp();
        consent.last_material_change = None;

        storage::set_guardian_consent(&env, &consent);

        events::guardian_consent_granted(
            &env,
            engagement_id,
            &guardian,
            &consent.mentee,
            &new_terms_hash,
        );

        Ok(())
    }

    // ========================================================================
    // PAUSE AND RESUME (MENTOR-049)
    // ========================================================================

    /// Request to pause an engagement
    pub fn request_pause(
        env: Env,
        caller: Address,
        engagement_id: u64,
        reason: PauseReason,
        reason_hash: BytesN<32>,
        effective_date: u64,
        resume_date: Option<u64>,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Must be a participant
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        // Must be active
        if engagement.status != EngagementStatus::Active {
            return Err(MentorshipError::InvalidStatus);
        }

        // Check if already paused
        if engagement.active_pause.is_some() {
            return Err(MentorshipError::AlreadyPaused);
        }

        let now = env.ledger().timestamp();

        // Validate dates
        if effective_date < now {
            return Err(MentorshipError::InvalidTimestamp);
        }

        if let Some(resume) = resume_date {
            if resume <= effective_date {
                return Err(MentorshipError::InvalidTimestamp);
            }
        }

        let requires_approval = storage::get_pause_requires_mutual_approval(&env);

        let pause_request = PauseRequest {
            requester: caller.clone(),
            reason: reason.clone(),
            reason_hash: reason_hash.clone(),
            effective_date,
            resume_date,
            requested_at: now,
            requires_approval,
            is_approved: !requires_approval, // Auto-approve if not required
        };

        engagement.active_pause = Some(pause_request);

        // If approval not required, pause immediately (if effective date reached)
        if !requires_approval && effective_date <= now {
            engagement.status = EngagementStatus::Paused;
            events::engagement_paused(&env, engagement_id, reason.clone());
        }

        storage::set_engagement(&env, &engagement);

        events::pause_requested(&env, engagement_id, &caller, reason, effective_date);

        Ok(())
    }

    /// Approve a pause request (if mutual approval required)
    pub fn approve_pause(
        env: Env,
        caller: Address,
        engagement_id: u64,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Must be the other participant
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        let mut pause = engagement
            .active_pause
            .clone()
            .ok_or(MentorshipError::PauseRequestNotFound)?;

        // Can't approve your own request
        if caller == pause.requester {
            return Err(MentorshipError::Unauthorized);
        }

        if !pause.requires_approval {
            return Err(MentorshipError::PauseNotApproved);
        }

        if pause.is_approved {
            return Err(MentorshipError::PauseAlreadyApproved);
        }

        pause.is_approved = true;
        engagement.active_pause = Some(pause.clone());

        // Apply pause if effective date reached
        let now = env.ledger().timestamp();
        if pause.effective_date <= now {
            engagement.status = EngagementStatus::Paused;
            events::engagement_paused(&env, engagement_id, pause.reason.clone());
        }

        storage::set_engagement(&env, &engagement);

        events::pause_approved(&env, engagement_id, &caller);

        Ok(())
    }

    /// Resume a paused engagement
    pub fn resume_engagement(
        env: Env,
        caller: Address,
        engagement_id: u64,
    ) -> Result<(), MentorshipError> {
        caller.require_auth();

        let mut engagement = storage::get_engagement(&env, engagement_id)?;

        // Must be a participant
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(MentorshipError::NotParticipant);
        }

        // Must be paused
        if engagement.status != EngagementStatus::Paused {
            return Err(MentorshipError::NotPaused);
        }

        engagement.status = EngagementStatus::Active;
        engagement.active_pause = None;
        engagement.last_activity_at = env.ledger().timestamp();

        storage::set_engagement(&env, &engagement);

        events::engagement_resumed(&env, engagement_id);

        Ok(())
    }

    // ========================================================================
    // MATCHING FEEDBACK (MENTOR-043)
    // ========================================================================

    /// Submit matching feedback
    pub fn submit_feedback(
        env: Env,
        user: Address,
        feedback_hash: BytesN<32>,
        feedback_type: FeedbackType,
        reference_id: Option<u64>,
    ) -> Result<u64, MentorshipError> {
        user.require_auth();

        let feedback_id = storage::increment_feedback_id(&env);
        let now = env.ledger().timestamp();

        let feedback = MatchingFeedback {
            id: feedback_id,
            user: user.clone(),
            feedback_hash,
            feedback_type: feedback_type.clone(),
            submitted_at: now,
            reference_id,
        };

        storage::set_feedback(&env, &feedback);

        events::feedback_submitted(&env, feedback_id, &user, feedback_type);

        Ok(feedback_id)
    }

    // ========================================================================
    // QUERY FUNCTIONS
    // ========================================================================

    /// Get engagement details
    pub fn get_engagement(env: Env, engagement_id: u64) -> Result<Engagement, MentorshipError> {
        storage::get_engagement(&env, engagement_id)
    }

    /// Get guardian consent
    pub fn get_guardian_consent(
        env: Env,
        engagement_id: u64,
    ) -> Result<GuardianConsent, MentorshipError> {
        storage::get_guardian_consent(&env, engagement_id)
    }

    /// Get termination outcome
    pub fn get_termination_outcome(
        env: Env,
        engagement_id: u64,
    ) -> Option<TerminationOutcome> {
        storage::get_termination_outcome(&env, engagement_id)
    }

    /// Get feedback
    pub fn get_feedback(env: Env, feedback_id: u64) -> Result<MatchingFeedback, MentorshipError> {
        storage::get_feedback(&env, feedback_id)
    }

    /// Check if consent is currently valid
    pub fn is_consent_valid(env: Env, engagement_id: u64) -> Result<bool, MentorshipError> {
        let consent = storage::get_guardian_consent(&env, engagement_id)?;
        
        if !consent.is_active {
            return Ok(false);
        }

        // Check expiry
        if let Some(expiry) = consent.expires_at {
            if env.ledger().timestamp() >= expiry {
                return Ok(false);
            }
        }

        // Check if material change requires re-consent
        if consent.last_material_change.is_some() {
            return Ok(false);
        }

        Ok(true)
    }
}
