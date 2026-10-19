#![no_std]

//! Mentorship code-of-conduct contract (work item MENTOR-051, issue #1247).
//!
//! The contract makes conduct acceptance a provable, versioned precondition
//! for a mentorship engagement:
//!
//! - **Versioned policies.** An admin publishes immutable policy versions,
//!   each with a privacy-preserving `policy_hash` commitment over the
//!   off-chain policy text, an `effective_at`, an audience, and a
//!   `mandatory` flag. Versions are strictly sequential, so a client can
//!   always tell whether it is looking at the newest policy.
//! - **Provable acceptance.** A participant accepts a specific policy
//!   version in a specific role. The acceptance record stores the version,
//!   the policy hash, and the ledger timestamp. Accepting again is a no-op
//!   (safe retries, no duplicated effects), and acceptance survives policy
//!   retirement so it can always be proven after the fact.
//! - **Activation gate.** `activate_engagement` requires both parties'
//!   authorization and each party's acceptance of the latest *mandatory*
//!   policy that applies to their role. Publishing a newer mandatory policy
//!   makes `engagement_compliant` return `false` until the parties
//!   re-acknowledge (`requires_acknowledgement`), which is how an updated
//!   policy prompts re-acceptance without rewriting the engagement.
//! - **Breach references.** The admin can record a breach case reference
//!   against a subject and policy version. Only a case reference, severity,
//!   and evidence hash are stored; narratives, identities of reporters, and
//!   evidence files stay off-chain and are never copied on-chain.
//!
//! Privacy boundary: this contract stores no policy text, no breach
//! narrative, and no reporter identity. It stores hashes, roles, versions,
//! addresses, severity, and timestamps. See
//! `contracts/docs/mentorship-conduct.md`.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Bounded storage: policy version numbers are capped so the number of
/// stored policy records can never grow without bound.
const MAX_POLICY_VERSION: u32 = 1_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// The caller is authenticated but is not one of the engagement parties.
    NotAuthorized = 4,
    PolicyNotFound = 5,
    /// Versions must increase by exactly one; clients cache the latest.
    PolicyVersionOutOfSequence = 6,
    /// A retired policy can no longer be accepted for the first time.
    PolicyRetired = 7,
    /// The policy does not apply to the role the participant is accepting for.
    PolicyNotApplicable = 8,
    /// The policy hash commitment must not be all zeroes.
    InvalidPolicyHash = 9,
    PolicyAlreadyRetired = 10,
    /// `MAX_POLICY_VERSION` reached; publish no further versions.
    PolicyVersionCapReached = 11,
    /// The latest applicable mandatory policy has not been accepted.
    AcceptanceRequired = 12,
    AcceptanceNotFound = 13,
    EngagementNotFound = 14,
    EngagementAlreadyExists = 15,
    EngagementNotPending = 16,
    EngagementNotActive = 17,
    EngagementNotSuspended = 18,
    /// A mentor cannot mentor themselves.
    InvalidParties = 19,
    InvalidEngagementId = 20,
    BreachAlreadyRecorded = 21,
    BreachNotFound = 22,
    BreachAlreadyResolved = 23,
    VersionOverflow = 24,
    /// `effective_at` must be greater than zero.
    InvalidEffectiveTime = 25,
    /// The evidence commitment must not be all zeroes.
    InvalidEvidenceHash = 26,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Highest policy version published so far (any audience). 0 = none.
    PolicyVersion,
    /// Latest version published for a role, used to resolve the policy a
    /// participant must currently accept.
    LatestPolicy(Role),
    /// An immutable policy version.
    Policy(u32),
    /// A participant's acceptance of one policy version in one role.
    Acceptance(Address, u32, Role),
    /// A mentorship engagement.
    Engagement(BytesN<32>),
    /// A breach case reference.
    Breach(BytesN<32>),
}

/// The participant roles a policy can apply to.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Mentor,
    Mentee,
}

/// The audience a policy version applies to.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    Mentor,
    Mentee,
    Both,
}

impl Audience {
    fn applies_to(self, role: Role) -> bool {
        matches!(
            (self, role),
            (Audience::Both, _)
                | (Audience::Mentor, Role::Mentor)
                | (Audience::Mentee, Role::Mentee)
        )
    }
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    pub version: u32,
    pub audience: Audience,
    /// Commitment over the off-chain policy text. The text itself is never
    /// stored on-chain.
    pub policy_hash: BytesN<32>,
    /// Ledger timestamp from which this version is the current policy.
    pub effective_at: u64,
    /// Mandatory policies gate engagement activation and re-acknowledgement.
    pub mandatory: bool,
    pub published_at: u64,
    pub retired: bool,
}

/// Proof that `actor` accepted `policy_version` in `role`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acceptance {
    pub actor: Address,
    pub role: Role,
    pub policy_version: u32,
    /// The accepted policy's hash, so the proof pins the exact text version.
    pub policy_hash: BytesN<32>,
    pub accepted_at: u64,
}

/// A mentorship engagement between two parties.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngagementState {
    Pending,
    Active,
    Suspended,
    Ended,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Engagement {
    pub id: BytesN<32>,
    pub mentor: Address,
    pub mentee: Address,
    pub state: EngagementState,
    /// Policy versions each party accepted when the engagement activated.
    pub mentor_policy_version: u32,
    pub mentee_policy_version: u32,
    pub created_at: u64,
    pub activated_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreachSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// A breach case reference. Private case details stay in the off-chain
/// safety system; only the commitment and public bookkeeping are on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BreachRecord {
    pub case_ref: BytesN<32>,
    pub subject: Address,
    pub policy_version: u32,
    pub severity: BreachSeverity,
    pub evidence_hash: BytesN<32>,
    pub recorded_at: u64,
    pub resolved: bool,
    pub resolved_at: u64,
}

#[contract]
pub struct MentorshipConductContract;

#[contractimpl]
impl MentorshipConductContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn put_persistent<T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(
        env: &Env,
        key: &DataKey,
        value: &T,
    ) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_policy(env: &Env, version: u32) -> Result<Policy, ContractError> {
        let key = DataKey::Policy(version);
        let policy: Policy = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::PolicyNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(policy)
    }

    fn latest_policy_for_role(env: &Env, role: Role) -> Result<Policy, ContractError> {
        let version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LatestPolicy(role))
            .ok_or(ContractError::PolicyNotFound)?;
        Self::load_policy(env, version)
    }

    /// Store the version as the latest for every role it applies to.
    fn record_latest(env: &Env, policy: &Policy) {
        if policy.audience.applies_to(Role::Mentor) {
            env.storage()
                .persistent()
                .set(&DataKey::LatestPolicy(Role::Mentor), &policy.version);
            env.storage().persistent().extend_ttl(
                &DataKey::LatestPolicy(Role::Mentor),
                RECORD_MIN_TTL,
                RECORD_MAX_TTL,
            );
        }
        if policy.audience.applies_to(Role::Mentee) {
            env.storage()
                .persistent()
                .set(&DataKey::LatestPolicy(Role::Mentee), &policy.version);
            env.storage().persistent().extend_ttl(
                &DataKey::LatestPolicy(Role::Mentee),
                RECORD_MIN_TTL,
                RECORD_MAX_TTL,
            );
        }
    }

    // ── MENTOR-051 — versioned policy registry ───────────────────────────

    /// Admin-only: publish the next immutable policy version and return its
    /// version number. Versions are sequential so clients can detect a newer
    /// mandatory policy and prompt re-acknowledgement.
    pub fn publish_policy(
        env: Env,
        admin: Address,
        audience: Audience,
        policy_hash: BytesN<32>,
        effective_at: u64,
        mandatory: bool,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;

        if policy_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidPolicyHash);
        }
        if effective_at == 0 {
            return Err(ContractError::InvalidEffectiveTime);
        }

        let current: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::PolicyVersion)
            .unwrap_or(0);
        let version = current
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        if version > MAX_POLICY_VERSION {
            return Err(ContractError::PolicyVersionCapReached);
        }

        let policy = Policy {
            version,
            audience,
            policy_hash,
            effective_at,
            mandatory,
            published_at: env.ledger().timestamp(),
            retired: false,
        };
        Self::put_persistent(&env, &DataKey::Policy(version), &policy);
        env.storage()
            .persistent()
            .set(&DataKey::PolicyVersion, &version);
        env.storage().persistent().extend_ttl(
            &DataKey::PolicyVersion,
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );
        Self::record_latest(&env, &policy);

        env.events()
            .publish((symbol_short!("POLPUB"),), (version, audience));
        Ok(version)
    }

    /// Admin-only: retire a policy version. It can no longer be accepted for
    /// the first time and no longer gates activation, but existing
    /// acceptance proofs remain verifiable.
    pub fn retire_policy(env: Env, admin: Address, version: u32) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let mut policy = Self::load_policy(&env, version)?;
        if policy.retired {
            return Err(ContractError::PolicyAlreadyRetired);
        }
        policy.retired = true;
        Self::put_persistent(&env, &DataKey::Policy(version), &policy);
        env.events().publish((symbol_short!("POLRTD"),), (version,));
        Ok(())
    }

    pub fn get_policy(env: Env, version: u32) -> Result<Policy, ContractError> {
        Self::load_policy(&env, version)
    }

    /// The version a participant in `role` is currently expected to accept.
    pub fn latest_policy(env: Env, role: Role) -> Result<Policy, ContractError> {
        Self::latest_policy_for_role(&env, role)
    }

    // ── MENTOR-051 — acceptance ──────────────────────────────────────────

    /// Record the caller's acceptance of `version` in `role`.
    ///
    /// Idempotent: a retry for an already-recorded (actor, version, role)
    /// returns `Ok(())` without writing again or emitting a second event.
    pub fn accept_policy(
        env: Env,
        actor: Address,
        version: u32,
        role: Role,
    ) -> Result<(), ContractError> {
        actor.require_auth();

        // Retried submissions must never duplicate effects or fail because
        // the policy was retired after the original acceptance.
        if Self::has_accepted(env.clone(), actor.clone(), version, role) {
            return Ok(());
        }

        let policy = Self::load_policy(&env, version)?;
        if policy.retired {
            return Err(ContractError::PolicyRetired);
        }
        if !policy.audience.applies_to(role) {
            return Err(ContractError::PolicyNotApplicable);
        }

        let acceptance = Acceptance {
            actor: actor.clone(),
            role,
            policy_version: version,
            policy_hash: policy.policy_hash,
            accepted_at: env.ledger().timestamp(),
        };
        Self::put_persistent(
            &env,
            &DataKey::Acceptance(actor.clone(), version, role),
            &acceptance,
        );

        env.events()
            .publish((symbol_short!("ACCEPT"),), (actor, version, role));
        Ok(())
    }

    pub fn get_acceptance(
        env: Env,
        actor: Address,
        version: u32,
        role: Role,
    ) -> Result<Acceptance, ContractError> {
        let key = DataKey::Acceptance(actor, version, role);
        let acceptance: Acceptance = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AcceptanceNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(acceptance)
    }

    pub fn has_accepted(env: Env, actor: Address, version: u32, role: Role) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Acceptance(actor, version, role))
    }

    /// Provable acceptance: true when `actor` has a recorded acceptance of
    /// `version` in `role` whose policy hash matches `policy_hash`.
    pub fn verify_acceptance(
        env: Env,
        actor: Address,
        version: u32,
        role: Role,
        policy_hash: BytesN<32>,
    ) -> bool {
        let key = DataKey::Acceptance(actor, version, role);
        match env.storage().persistent().get::<DataKey, Acceptance>(&key) {
            Some(acceptance) => acceptance.policy_hash == policy_hash,
            None => false,
        }
    }

    /// True when a newer mandatory policy applies to `role` and `actor` has
    /// not accepted it yet — the signal behind a re-acknowledgement prompt.
    pub fn requires_acknowledgement(env: Env, actor: Address, role: Role) -> bool {
        match Self::latest_policy_for_role(&env, role) {
            Ok(policy) => {
                policy.mandatory
                    && !policy.retired
                    && !Self::has_accepted(env.clone(), actor, policy.version, role)
            }
            Err(_) => false,
        }
    }

    /// Fail-closed gate used by engagement activation: a mandatory,
    /// unretired policy must be accepted by `actor` in `role`.
    fn require_current_acceptance(
        env: &Env,
        actor: &Address,
        role: Role,
    ) -> Result<u32, ContractError> {
        let policy = Self::latest_policy_for_role(env, role)?;
        if policy.retired {
            return Err(ContractError::PolicyRetired);
        }
        if !policy.mandatory {
            return Ok(policy.version);
        }
        if !Self::has_accepted(env.clone(), actor.clone(), policy.version, role) {
            return Err(ContractError::AcceptanceRequired);
        }
        Ok(policy.version)
    }

    // ── MENTOR-051 — engagements gated on acceptance ─────────────────────

    /// Create a pending engagement. Both parties must authorize, and the
    /// caller must be one of them. Activation is a separate,
    /// acceptance-gated step.
    ///
    /// The caller's own credential is not requested separately: the
    /// engagement only exists once BOTH the mentor and the mentee have
    /// authorized it, which is the stronger condition.
    pub fn create_engagement(
        env: Env,
        caller: Address,
        engagement_id: BytesN<32>,
        mentor: Address,
        mentee: Address,
    ) -> Result<(), ContractError> {
        if caller != mentor && caller != mentee {
            return Err(ContractError::NotAuthorized);
        }
        if mentor == mentee {
            return Err(ContractError::InvalidParties);
        }
        if engagement_id == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidEngagementId);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Engagement(engagement_id.clone()))
        {
            return Err(ContractError::EngagementAlreadyExists);
        }
        mentor.require_auth();
        mentee.require_auth();

        let now = env.ledger().timestamp();
        let engagement = Engagement {
            id: engagement_id.clone(),
            mentor,
            mentee,
            state: EngagementState::Pending,
            mentor_policy_version: 0,
            mentee_policy_version: 0,
            created_at: now,
            activated_at: 0,
            updated_at: now,
        };
        Self::put_persistent(
            &env,
            &DataKey::Engagement(engagement_id.clone()),
            &engagement,
        );

        env.events()
            .publish((symbol_short!("ENGNEW"),), (engagement_id,));
        Ok(())
    }

    fn load_engagement(env: &Env, id: &BytesN<32>) -> Result<Engagement, ContractError> {
        let key = DataKey::Engagement(id.clone());
        let engagement: Engagement = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::EngagementNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(engagement)
    }

    /// Activate a pending engagement. Blocked unless both parties have
    /// accepted the latest mandatory policies that apply to their roles.
    pub fn activate_engagement(
        env: Env,
        caller: Address,
        engagement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let mut engagement = Self::load_engagement(&env, &engagement_id)?;
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(ContractError::NotAuthorized);
        }
        if engagement.state != EngagementState::Pending {
            return Err(ContractError::EngagementNotPending);
        }

        let mentor_version =
            Self::require_current_acceptance(&env, &engagement.mentor, Role::Mentor)?;
        let mentee_version =
            Self::require_current_acceptance(&env, &engagement.mentee, Role::Mentee)?;

        let now = env.ledger().timestamp();
        engagement.state = EngagementState::Active;
        engagement.mentor_policy_version = mentor_version;
        engagement.mentee_policy_version = mentee_version;
        engagement.activated_at = now;
        engagement.updated_at = now;
        Self::put_persistent(
            &env,
            &DataKey::Engagement(engagement_id.clone()),
            &engagement,
        );

        env.events().publish(
            (symbol_short!("ENGACT"),),
            (engagement_id, mentor_version, mentee_version),
        );
        Ok(())
    }

    /// True while both parties remain current with the latest mandatory
    /// policies. A newly published mandatory version makes this `false`
    /// until each party accepts it.
    pub fn engagement_compliant(env: Env, engagement_id: BytesN<32>) -> bool {
        let engagement = match Self::load_engagement(&env, &engagement_id) {
            Ok(value) => value,
            Err(_) => return false,
        };
        if engagement.state != EngagementState::Active {
            return false;
        }

        let parties = [
            (engagement.mentor.clone(), Role::Mentor),
            (engagement.mentee.clone(), Role::Mentee),
        ];
        for (actor, role) in parties {
            let policy = match Self::latest_policy_for_role(&env, role) {
                Ok(value) => value,
                Err(_) => return false,
            };
            if policy.retired {
                return false;
            }
            if policy.mandatory && !Self::has_accepted(env.clone(), actor, policy.version, role) {
                return false;
            }
        }
        true
    }

    pub fn get_engagement(
        env: Env,
        engagement_id: BytesN<32>,
    ) -> Result<Engagement, ContractError> {
        Self::load_engagement(&env, &engagement_id)
    }

    /// Admin-only: suspend an active engagement while a conduct case is
    /// investigated.
    pub fn suspend_engagement(
        env: Env,
        admin: Address,
        engagement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let mut engagement = Self::load_engagement(&env, &engagement_id)?;
        if engagement.state != EngagementState::Active {
            return Err(ContractError::EngagementNotActive);
        }
        engagement.state = EngagementState::Suspended;
        engagement.updated_at = env.ledger().timestamp();
        Self::put_persistent(
            &env,
            &DataKey::Engagement(engagement_id.clone()),
            &engagement,
        );
        env.events()
            .publish((symbol_short!("ENGSUS"),), (engagement_id,));
        Ok(())
    }

    /// Resume a suspended engagement. Both parties must again be current
    /// with the latest mandatory policies, so a paused relationship cannot
    /// bypass a policy update.
    pub fn resume_engagement(
        env: Env,
        caller: Address,
        engagement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let mut engagement = Self::load_engagement(&env, &engagement_id)?;
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(ContractError::NotAuthorized);
        }
        if engagement.state != EngagementState::Suspended {
            return Err(ContractError::EngagementNotSuspended);
        }

        engagement.mentor_policy_version =
            Self::require_current_acceptance(&env, &engagement.mentor, Role::Mentor)?;
        engagement.mentee_policy_version =
            Self::require_current_acceptance(&env, &engagement.mentee, Role::Mentee)?;
        engagement.state = EngagementState::Active;
        engagement.updated_at = env.ledger().timestamp();
        Self::put_persistent(
            &env,
            &DataKey::Engagement(engagement_id.clone()),
            &engagement,
        );

        env.events()
            .publish((symbol_short!("ENGRES"),), (engagement_id,));
        Ok(())
    }

    /// End an engagement. Either party may end it; the state transition is
    /// one-way and repeated calls fail with `EngagementNotActive`.
    pub fn end_engagement(
        env: Env,
        caller: Address,
        engagement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let mut engagement = Self::load_engagement(&env, &engagement_id)?;
        if caller != engagement.mentor && caller != engagement.mentee {
            return Err(ContractError::NotAuthorized);
        }
        if engagement.state == EngagementState::Ended {
            return Err(ContractError::EngagementNotActive);
        }
        engagement.state = EngagementState::Ended;
        engagement.updated_at = env.ledger().timestamp();
        Self::put_persistent(
            &env,
            &DataKey::Engagement(engagement_id.clone()),
            &engagement,
        );
        env.events()
            .publish((symbol_short!("ENGEND"),), (engagement_id,));
        Ok(())
    }

    // ── MENTOR-051 — breach references ───────────────────────────────────

    /// Admin-only: record a breach case reference against a subject and the
    /// policy version that was breached. Only commitments are stored.
    pub fn record_breach(
        env: Env,
        admin: Address,
        case_ref: BytesN<32>,
        subject: Address,
        policy_version: u32,
        severity: BreachSeverity,
        evidence_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if evidence_hash == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidEvidenceHash);
        }
        // The referenced policy version must exist so breach references
        // cannot point at nonexistent policy text.
        Self::load_policy(&env, policy_version)?;
        if env
            .storage()
            .persistent()
            .has(&DataKey::Breach(case_ref.clone()))
        {
            return Err(ContractError::BreachAlreadyRecorded);
        }

        let record = BreachRecord {
            case_ref: case_ref.clone(),
            subject: subject.clone(),
            policy_version,
            severity,
            evidence_hash,
            recorded_at: env.ledger().timestamp(),
            resolved: false,
            resolved_at: 0,
        };
        Self::put_persistent(&env, &DataKey::Breach(case_ref.clone()), &record);

        env.events()
            .publish((symbol_short!("BREACH"),), (case_ref, subject, severity));
        Ok(())
    }

    /// Admin-only: mark a breach case reference resolved.
    pub fn resolve_breach(
        env: Env,
        admin: Address,
        case_ref: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Breach(case_ref.clone());
        let mut record: BreachRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BreachNotFound)?;
        if record.resolved {
            return Err(ContractError::BreachAlreadyResolved);
        }
        record.resolved = true;
        record.resolved_at = env.ledger().timestamp();
        Self::put_persistent(&env, &key, &record);
        env.events().publish((symbol_short!("BRRES"),), (case_ref,));
        Ok(())
    }

    pub fn get_breach(env: Env, case_ref: BytesN<32>) -> Result<BreachRecord, ContractError> {
        let key = DataKey::Breach(case_ref);
        let record: BreachRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::BreachNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    /// Versions of the policies a participant has accepted, newest first is
    /// not guaranteed; callers resolve current versions from `latest_policy`.
    pub fn accepted_versions(env: Env, actor: Address, role: Role, versions: Vec<u32>) -> Vec<u32> {
        let mut accepted: Vec<u32> = Vec::new(&env);
        for version in versions.iter() {
            if Self::has_accepted(env.clone(), actor.clone(), version, role) {
                accepted.push_back(version);
            }
        }
        accepted
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
