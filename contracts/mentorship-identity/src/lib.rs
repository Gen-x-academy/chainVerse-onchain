#![no_std]

//! Privacy-minimized mentorship profile completion and trust indicators.
//!
//! Profile content and performance evidence remain off-chain. This contract
//! stores a salted profile commitment, a self-attested completion count, and
//! public indicator attestations from configured recorder, verifier, and
//! moderator roles.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const REQUIRED_PROFILE_SECTIONS: u32 = 6;
const MAX_RATE_BASIS_POINTS: u32 = 10_000;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorizedRecorder = 4,
    NotAuthorizedVerifier = 5,
    NotAuthorizedModerator = 6,
    ProfileNotFound = 7,
    InvalidCompletion = 8,
    InvalidCommitment = 9,
    InvalidWindow = 10,
    InvalidRate = 11,
    InvalidExpiry = 12,
    VerificationNotFound = 13,
    VerificationAlreadyRevoked = 14,
    ConsentRequired = 15,
    VerificationPending = 16,
    VerificationStillValid = 17,
    VerificationNotActive = 18,
    InvalidReview = 19,
    Overflow = 20,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Recorder,
    Verifier,
    Moderator,
    Profile(Address),
    MenteeProfile(Address),
    Performance(Address),
    Verification(Address),
    Safety(Address),
}

/// On-chain profile representation. Profile details remain off-chain; the
/// count is a coarse self-attestation and section-specific flags are not kept.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorProfile {
    pub profile_commitment: BytesN<32>,
    pub completed_sections: u32,
    pub completion_basis_points: u32,
    pub updated_at: u64,
}

/// Mentee learning attributes committed field-by-field. Their canonical
/// values and any matching inputs remain off-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenteeLearningProfile {
    pub goals_commitment: BytesN<32>,
    pub current_level_commitment: BytesN<32>,
    pub interests_commitment: BytesN<32>,
    pub preferred_language_commitment: BytesN<32>,
    pub time_zone_commitment: BytesN<32>,
    pub accessibility_needs_commitment: BytesN<32>,
    pub matching_preferences_commitment: BytesN<32>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Trusted-recorder attestation computed from off-chain activity records.
/// No request or session identifiers, counts, or participant addresses are stored.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerformanceAttestation {
    pub response_rate_basis_points: u32,
    pub delivery_rate_basis_points: u32,
    pub evidence_commitment: BytesN<32>,
    pub window_start: u64,
    pub window_end: u64,
    pub recorder: Address,
    pub recorded_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    Expired,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationDecision {
    Approve,
    Reject,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationAttestation {
    pub provider_reference: BytesN<32>,
    pub evidence_commitment: BytesN<32>,
    pub consent_given: bool,
    pub consent_version: u32,
    pub consent_given_at: u64,
    pub consent_revoked_at: Option<u64>,
    pub status: VerificationStatus,
    pub reviewer: Option<Address>,
    pub reviewed_at: Option<u64>,
    pub expires_at: u64,
    pub rejection_reason_commitment: BytesN<32>,
    pub attempt: u32,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyStatus {
    Unassessed,
    Clear,
    Review,
    Restricted,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafetyAssessment {
    pub status: SafetyStatus,
    pub evidence_commitment: BytesN<32>,
    pub moderator: Address,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorIndicators {
    pub subject: Address,
    pub profile: MentorProfile,
    pub required_profile_sections: u32,
    pub is_verified: bool,
    pub performance: Option<PerformanceAttestation>,
    pub verification: Option<VerificationAttestation>,
    pub safety: Option<SafetyAssessment>,
    pub safety_status: SafetyStatus,
}

#[contract]
pub struct MentorshipIdentityContract;

#[contractimpl]
impl MentorshipIdentityContract {
    /// One-time role configuration. The admin controls the service accounts
    /// that can attest performance, verification, and safety state.
    pub fn initialize(
        env: Env,
        admin: Address,
        recorder: Address,
        verifier: Address,
        moderator: Address,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Recorder, &recorder);
        env.storage().instance().set(&DataKey::Verifier, &verifier);
        env.storage().instance().set(&DataKey::Moderator, &moderator);
        env.events().publish(
            (symbol_short!("MID_INIT"),),
            (admin, recorder, verifier, moderator),
        );
        Ok(())
    }

    pub fn set_recorder(
        env: Env,
        admin: Address,
        recorder: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Recorder, &recorder);
        env.events()
            .publish((symbol_short!("MID_ROLE"),), (symbol_short!("REC"), recorder));
        Ok(())
    }

    pub fn set_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Verifier, &verifier);
        env.events()
            .publish((symbol_short!("MID_ROLE"),), (symbol_short!("VER"), verifier));
        Ok(())
    }

    pub fn set_moderator(
        env: Env,
        admin: Address,
        moderator: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Moderator, &moderator);
        env.events().publish(
            (symbol_short!("MID_ROLE"),),
            (symbol_short!("MOD"), moderator),
        );
        Ok(())
    }

    /// The mentor submits only a content commitment and a bounded count of
    /// completed checklist sections. Completion is calculated equally across
    /// the six documented sections; no section-level flags or content are stored.
    pub fn update_profile(
        env: Env,
        subject: Address,
        profile_commitment: BytesN<32>,
        completed_sections: u32,
    ) -> Result<(), ContractError> {
        subject.require_auth();
        if Self::is_zero_commitment(&profile_commitment) {
            return Err(ContractError::InvalidCommitment);
        }
        if completed_sections > REQUIRED_PROFILE_SECTIONS {
            return Err(ContractError::InvalidCompletion);
        }

        let completion_basis_points =
            completed_sections * MAX_RATE_BASIS_POINTS / REQUIRED_PROFILE_SECTIONS;
        let now = env.ledger().timestamp();
        let profile = MentorProfile {
            profile_commitment,
            completed_sections,
            completion_basis_points,
            updated_at: now,
        };
        let key = DataKey::Profile(subject.clone());
        env.storage().persistent().set(&key, &profile);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("MID_PROF"),),
            (subject, completed_sections, completion_basis_points, now),
        );
        Ok(())
    }

    /// Create or update a mentee learning profile using only commitments to
    /// private attributes. Matching services resolve the committed values
    /// off-chain after obtaining the mentee's consent.
    #[allow(clippy::too_many_arguments)]
    pub fn set_mentee_learning_profile(
        env: Env,
        subject: Address,
        goals_commitment: BytesN<32>,
        current_level_commitment: BytesN<32>,
        interests_commitment: BytesN<32>,
        preferred_language_commitment: BytesN<32>,
        time_zone_commitment: BytesN<32>,
        accessibility_needs_commitment: BytesN<32>,
        matching_preferences_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        subject.require_auth();
        if Self::is_zero_commitment(&goals_commitment)
            || Self::is_zero_commitment(&current_level_commitment)
            || Self::is_zero_commitment(&interests_commitment)
            || Self::is_zero_commitment(&preferred_language_commitment)
            || Self::is_zero_commitment(&time_zone_commitment)
            || Self::is_zero_commitment(&accessibility_needs_commitment)
            || Self::is_zero_commitment(&matching_preferences_commitment)
        {
            return Err(ContractError::InvalidCommitment);
        }

        let key = DataKey::MenteeProfile(subject.clone());
        let previous: Option<MenteeLearningProfile> =
            env.storage().persistent().get(&key);
        let now = env.ledger().timestamp();
        let profile = MenteeLearningProfile {
            goals_commitment,
            current_level_commitment,
            interests_commitment,
            preferred_language_commitment,
            time_zone_commitment,
            accessibility_needs_commitment,
            matching_preferences_commitment,
            created_at: previous.as_ref().map_or(now, |old| old.created_at),
            updated_at: now,
        };
        env.storage().persistent().set(&key, &profile);
        Self::bump_ttl(&env, &key);
        env.events()
            .publish((symbol_short!("MID_MENT"),), (subject, now));
        Ok(())
    }

    pub fn get_mentee_learning_profile(
        env: Env,
        subject: Address,
    ) -> Result<MenteeLearningProfile, ContractError> {
        let key = DataKey::MenteeProfile(subject);
        let profile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProfileNotFound)?;
        Self::bump_ttl(&env, &key);
        Ok(profile)
    }

    /// Record bounded performance rates computed by the trusted recorder from
    /// off-chain requests and sessions. The evidence commitment allows an
    /// authorized auditor to check the private source data without publishing it.
    pub fn attest_performance(
        env: Env,
        recorder: Address,
        subject: Address,
        response_rate_basis_points: u32,
        delivery_rate_basis_points: u32,
        evidence_commitment: BytesN<32>,
        window_start: u64,
        window_end: u64,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &recorder, &DataKey::Recorder)?;
        Self::require_profile(&env, &subject)?;
        if response_rate_basis_points > MAX_RATE_BASIS_POINTS
            || delivery_rate_basis_points > MAX_RATE_BASIS_POINTS
        {
            return Err(ContractError::InvalidRate);
        }
        if window_start >= window_end || window_end > env.ledger().timestamp() {
            return Err(ContractError::InvalidWindow);
        }
        if Self::is_zero_commitment(&evidence_commitment) {
            return Err(ContractError::InvalidCommitment);
        }

        let now = env.ledger().timestamp();
        let attestation = PerformanceAttestation {
            response_rate_basis_points,
            delivery_rate_basis_points,
            evidence_commitment: evidence_commitment.clone(),
            window_start,
            window_end,
            recorder: recorder.clone(),
            recorded_at: now,
        };
        let key = DataKey::Performance(subject.clone());
        env.storage().persistent().set(&key, &attestation);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("MID_PERF"),),
            (
                subject,
                response_rate_basis_points,
                delivery_rate_basis_points,
                evidence_commitment,
                window_start,
                window_end,
                recorder,
                now,
            ),
        );
        Ok(())
    }

    /// Start or re-start verification. Provider references and evidence are
    /// commitments only; the mentor explicitly consents to the supplied
    /// consent-text version before any provider review can begin.
    pub fn submit_verification(
        env: Env,
        subject: Address,
        provider_reference: BytesN<32>,
        evidence_commitment: BytesN<32>,
        consent: bool,
        consent_version: u32,
    ) -> Result<(), ContractError> {
        subject.require_auth();
        Self::require_profile(&env, &subject)?;
        if !consent || consent_version == 0 {
            return Err(ContractError::ConsentRequired);
        }
        if Self::is_zero_commitment(&provider_reference)
            || Self::is_zero_commitment(&evidence_commitment)
        {
            return Err(ContractError::InvalidCommitment);
        }

        let now = env.ledger().timestamp();
        let key = DataKey::Verification(subject.clone());
        let previous: Option<VerificationAttestation> =
            env.storage().persistent().get(&key);
        if let Some(existing) = previous.as_ref() {
            if existing.status == VerificationStatus::Pending {
                return Err(ContractError::VerificationPending);
            }
            if existing.status == VerificationStatus::Approved
                && existing.consent_revoked_at.is_none()
                && existing.expires_at > now
            {
                return Err(ContractError::VerificationStillValid);
            }
        }
        let attempt = match previous {
            Some(existing) => existing.attempt.checked_add(1).ok_or(ContractError::Overflow)?,
            None => 1,
        };
        let attestation = VerificationAttestation {
            provider_reference: provider_reference.clone(),
            evidence_commitment: evidence_commitment.clone(),
            consent_given: true,
            consent_version,
            consent_given_at: now,
            consent_revoked_at: None,
            status: VerificationStatus::Pending,
            reviewer: None,
            reviewed_at: None,
            expires_at: 0,
            rejection_reason_commitment: Self::zero_commitment(&env),
            attempt,
        };
        env.storage().persistent().set(&key, &attestation);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("VF_SUBMIT"),),
            (
                subject,
                attempt,
                provider_reference,
                evidence_commitment,
                consent_version,
                now,
            ),
        );
        Ok(())
    }

    /// Approve or reject the current pending submission. Rejection rationale
    /// is represented only by a commitment; raw review notes stay off-chain.
    pub fn review_verification(
        env: Env,
        verifier: Address,
        subject: Address,
        decision: VerificationDecision,
        expires_at: u64,
        rejection_reason_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &verifier, &DataKey::Verifier)?;
        let key = DataKey::Verification(subject.clone());
        let mut attestation: VerificationAttestation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::VerificationNotFound)?;
        if attestation.status != VerificationStatus::Pending || !attestation.consent_given {
            return Err(ContractError::InvalidReview);
        }
        let now = env.ledger().timestamp();
        let status = match decision {
            VerificationDecision::Approve => {
                if expires_at <= now || !Self::is_zero_commitment(&rejection_reason_commitment) {
                    return Err(ContractError::InvalidReview);
                }
                VerificationStatus::Approved
            }
            VerificationDecision::Reject => {
                if expires_at != 0 || Self::is_zero_commitment(&rejection_reason_commitment) {
                    return Err(ContractError::InvalidReview);
                }
                VerificationStatus::Rejected
            }
        };
        attestation.status = status;
        attestation.reviewer = Some(verifier.clone());
        attestation.reviewed_at = Some(now);
        attestation.expires_at = expires_at;
        attestation.rejection_reason_commitment = rejection_reason_commitment.clone();
        env.storage().persistent().set(&key, &attestation);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("VF_REVIEW"),),
            (
                subject,
                attestation.attempt,
                decision,
                expires_at,
                rejection_reason_commitment,
                verifier,
                now,
            ),
        );
        Ok(())
    }

    /// A mentor may withdraw consent at any time. This invalidates an
    /// approval immediately and marks an in-flight request as cancelled.
    pub fn cancel_verification(env: Env, subject: Address) -> Result<(), ContractError> {
        subject.require_auth();
        let key = DataKey::Verification(subject.clone());
        let mut attestation: VerificationAttestation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::VerificationNotFound)?;
        if !attestation.consent_given || attestation.consent_revoked_at.is_some() {
            return Err(ContractError::VerificationNotActive);
        }
        let now = env.ledger().timestamp();
        attestation.consent_given = false;
        attestation.consent_revoked_at = Some(now);
        attestation.status = VerificationStatus::Cancelled;
        env.storage().persistent().set(&key, &attestation);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("VF_CANCEL"),),
            (subject, attestation.attempt, now),
        );
        Ok(())
    }

    /// Store only a coarse safety status and a commitment to the off-chain
    /// assessment. No report details, allegations, or case notes are accepted.
    pub fn attest_safety(
        env: Env,
        moderator: Address,
        subject: Address,
        status: SafetyStatus,
        evidence_commitment: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &moderator, &DataKey::Moderator)?;
        Self::require_profile(&env, &subject)?;
        let empty_commitment = Self::is_zero_commitment(&evidence_commitment);
        if (status == SafetyStatus::Unassessed) != empty_commitment {
            return Err(ContractError::InvalidCommitment);
        }

        let now = env.ledger().timestamp();
        let assessment = SafetyAssessment {
            status,
            evidence_commitment: evidence_commitment.clone(),
            moderator: moderator.clone(),
            updated_at: now,
        };
        let key = DataKey::Safety(subject.clone());
        env.storage().persistent().set(&key, &assessment);
        Self::bump_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("MID_SAFE"),),
            (subject, status, evidence_commitment, moderator, now),
        );
        Ok(())
    }

    pub fn get_indicators(
        env: Env,
        subject: Address,
    ) -> Result<MentorIndicators, ContractError> {
        let profile = Self::require_profile(&env, &subject)?;
        let performance = env
            .storage()
            .persistent()
            .get(&DataKey::Performance(subject.clone()));
        let mut verification: Option<VerificationAttestation> = env
            .storage()
            .persistent()
            .get(&DataKey::Verification(subject.clone()));
        let safety: Option<SafetyAssessment> = env
            .storage()
            .persistent()
            .get(&DataKey::Safety(subject.clone()));
        let now = env.ledger().timestamp();
        if let Some(attestation) = verification.as_mut() {
            if attestation.status == VerificationStatus::Approved && attestation.expires_at <= now
            {
                attestation.status = VerificationStatus::Expired;
            }
        }
        let is_verified = verification.as_ref().is_some_and(|attestation| {
            attestation.status == VerificationStatus::Approved
                && attestation.consent_given
                && attestation.consent_revoked_at.is_none()
                && attestation.expires_at > now
        });
        let safety_status = safety
            .as_ref()
            .map(|assessment| assessment.status)
            .unwrap_or(SafetyStatus::Unassessed);

        Ok(MentorIndicators {
            subject,
            profile,
            required_profile_sections: REQUIRED_PROFILE_SECTIONS,
            is_verified,
            performance,
            verification,
            safety,
            safety_status,
        })
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        let configured_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if configured_admin != *admin {
            return Err(ContractError::NotAdmin);
        }
        admin.require_auth();
        Ok(())
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        key: &DataKey,
    ) -> Result<(), ContractError> {
        let expected: Address = env
            .storage()
            .instance()
            .get(key)
            .ok_or(ContractError::NotInitialized)?;
        if expected != *caller {
            // Role-specific errors are selected by the storage key.
            return Err(match key {
                DataKey::Recorder => ContractError::NotAuthorizedRecorder,
                DataKey::Verifier => ContractError::NotAuthorizedVerifier,
                DataKey::Moderator => ContractError::NotAuthorizedModerator,
                _ => ContractError::NotInitialized,
            });
        }
        caller.require_auth();
        Ok(())
    }

    fn require_profile(env: &Env, subject: &Address) -> Result<MentorProfile, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Profile(subject.clone()))
            .ok_or(ContractError::ProfileNotFound)
    }

    fn is_zero_commitment(commitment: &BytesN<32>) -> bool {
        commitment.to_array().iter().all(|byte| *byte == 0)
    }

    fn zero_commitment(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn bump_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;