#![no_std]

//! Scholarship/bursary eligibility contract.
//!
//! Scope of this pass (issues #1066, #1068): let an admin publish
//! versioned, bounded "eligibility rules" per program (a required set of
//! attestation types), let authorized issuers submit expiring, revocable
//! attestations that claim a subject satisfies one of those types, and
//! deterministically evaluate whether a subject currently meets a
//! program's published rule.
//!
//! Privacy-minimized by design: this contract never stores *why* a claim
//! is true (grades, income figures, documents) — only that an authorized
//! issuer vouches for a named attestation type, for a subject, within a
//! scope and expiry. The evidence backing an attestation lives entirely
//! off-chain with the issuer. See
//! `contracts/docs/scholarship-eligibility.md` for ownership, privacy,
//! migration, and operational notes.
//!
//! Prerequisite/exclusion graphs between programs (#1067) are tracked
//! separately and are not implemented here.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1066 — bounded storage: a rule may require at most this many
/// attestation types, so evaluation cost and storage size stay predictable.
const MAX_RULE_REQUIREMENTS: u32 = 10;

/// An attestation scoped to this value applies to every program, not just
/// one — e.g. a platform-wide "identity verified" claim.
fn global_scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotAuthorizedIssuer = 4,
    /// #1066 — a published rule must require at least one attestation type.
    EmptyRule = 5,
    /// #1066 — a rule may not require more than `MAX_RULE_REQUIREMENTS` types.
    RuleTooLarge = 6,
    NoRulePublished = 7,
    RuleNotFound = 8,
    /// #1068 — an attestation's expiry must be in the future when issued.
    InvalidExpiry = 9,
    AttestationNotFound = 10,
    /// #1068 — only the issuer who created an attestation (or the admin)
    /// may revoke it.
    NotAttestationIssuer = 11,
    VersionOverflow = 12,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// #1068 — allowlist of addresses trusted to issue attestations.
    Issuer(Address),
    /// #1066 — latest published rule version for a program.
    RuleVersion(BytesN<32>),
    /// #1066 — an immutable, published rule version.
    Rule(BytesN<32>, u32),
    /// #1068 — an attestation for (subject, scope, attestation_type).
    Attestation(Address, BytesN<32>, Symbol),
}

/// #1066 — a program's eligibility rule: the subject must hold a valid
/// (unexpired, unrevoked) attestation for every type listed here, from an
/// authorized issuer, scoped either to this program or globally.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityRule {
    pub version: u32,
    pub required_types: Vec<Symbol>,
    pub published_at: u64,
}

/// #1068 — a claim that `subject` satisfies `attestation_type`, vouched
/// for by `issuer`, valid until `expiry` unless revoked first.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attestation {
    pub issuer: Address,
    pub expiry: u64,
    pub issued_at: u64,
    pub revoked: bool,
}

#[contract]
pub struct ScholarshipEligibilityContract;

#[contractimpl]
impl ScholarshipEligibilityContract {
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

    // ── Issuer allowlist ──────────────────────────────────────────────────

    /// Admin-only: authorize `issuer` to submit attestations.
    pub fn add_issuer(env: Env, admin: Address, issuer: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Issuer(issuer);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Admin-only: revoke an issuer's authorization to submit *new*
    /// attestations. Attestations they already issued keep whatever
    /// validity they'd otherwise have — revoke those explicitly via
    /// `revoke_attestation` if they must be invalidated too.
    pub fn remove_issuer(env: Env, admin: Address, issuer: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Issuer(issuer), &false);
        Ok(())
    }

    pub fn is_authorized_issuer(env: Env, issuer: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Issuer(issuer))
            .unwrap_or(false)
    }

    // ── #1066 — composable eligibility rules ─────────────────────────────

    /// Admin-only: publish a new, immutable eligibility rule version for a
    /// program. Rejects an empty or oversized requirement list before
    /// publishing ("rules validate before publication"). Publishing again
    /// always creates version N+1; it never mutates an existing version,
    /// so evaluation against a specific historical version stays
    /// reproducible.
    pub fn publish_eligibility_rule(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        required_types: Vec<Symbol>,
    ) -> Result<u32, ContractError> {
        Self::require_admin(&env, &admin)?;

        if required_types.is_empty() {
            return Err(ContractError::EmptyRule);
        }
        if required_types.len() > MAX_RULE_REQUIREMENTS {
            return Err(ContractError::RuleTooLarge);
        }

        let version_key = DataKey::RuleVersion(program_id.clone());
        let next_version: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&version_key)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;

        let rule = EligibilityRule {
            version: next_version,
            required_types,
            published_at: env.ledger().timestamp(),
        };

        let rule_key = DataKey::Rule(program_id.clone(), next_version);
        env.storage().persistent().set(&rule_key, &rule);
        env.storage()
            .persistent()
            .extend_ttl(&rule_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.storage().persistent().set(&version_key, &next_version);
        env.storage()
            .persistent()
            .extend_ttl(&version_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("RULEPUB"),),
            (program_id, next_version),
        );

        Ok(next_version)
    }

    pub fn get_latest_rule_version(env: Env, program_id: BytesN<32>) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::RuleVersion(program_id))
            .ok_or(ContractError::NoRulePublished)
    }

    pub fn get_eligibility_rule(
        env: Env,
        program_id: BytesN<32>,
        version: u32,
    ) -> Result<EligibilityRule, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Rule(program_id, version))
            .ok_or(ContractError::RuleNotFound)
    }

    // ── #1068 — attestations ──────────────────────────────────────────────

    /// Authorized-issuer-only: issue (or re-issue) an attestation that
    /// `subject` satisfies `attestation_type`, scoped to `scope` (a
    /// program ID, or the all-zero global scope from `global_scope()`),
    /// valid until `expiry`. Re-issuing replaces any existing attestation
    /// for the same (subject, scope, type), resetting `revoked`.
    pub fn issue_attestation(
        env: Env,
        issuer: Address,
        subject: Address,
        scope: BytesN<32>,
        attestation_type: Symbol,
        expiry: u64,
    ) -> Result<(), ContractError> {
        issuer.require_auth();
        if !Self::is_authorized_issuer(env.clone(), issuer.clone()) {
            return Err(ContractError::NotAuthorizedIssuer);
        }
        if expiry <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }

        let attestation = Attestation {
            issuer: issuer.clone(),
            expiry,
            issued_at: env.ledger().timestamp(),
            revoked: false,
        };

        let key = DataKey::Attestation(subject.clone(), scope.clone(), attestation_type.clone());
        env.storage().persistent().set(&key, &attestation);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("ATTEST"),),
            (issuer, subject, attestation_type),
        );

        Ok(())
    }

    /// The original issuer, or the admin, may revoke an attestation —
    /// it becomes invalid immediately (`has_valid_attestation` returns
    /// `false` from the next read onward).
    pub fn revoke_attestation(
        env: Env,
        caller: Address,
        subject: Address,
        scope: BytesN<32>,
        attestation_type: Symbol,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let key = DataKey::Attestation(subject, scope, attestation_type);
        let mut attestation: Attestation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AttestationNotFound)?;

        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        let is_admin = admin.as_ref() == Some(&caller);
        if caller != attestation.issuer && !is_admin {
            return Err(ContractError::NotAttestationIssuer);
        }

        attestation.revoked = true;
        env.storage().persistent().set(&key, &attestation);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_attestation(
        env: Env,
        subject: Address,
        scope: BytesN<32>,
        attestation_type: Symbol,
    ) -> Result<Attestation, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Attestation(subject, scope, attestation_type))
            .ok_or(ContractError::AttestationNotFound)
    }

    /// True iff `subject` holds an unrevoked, unexpired attestation for
    /// `attestation_type` within `scope`.
    pub fn has_valid_attestation(
        env: Env,
        subject: Address,
        scope: BytesN<32>,
        attestation_type: Symbol,
    ) -> bool {
        let record: Option<Attestation> =
            env.storage()
                .persistent()
                .get(&DataKey::Attestation(subject, scope, attestation_type));
        match record {
            Some(a) => !a.revoked && a.expiry > env.ledger().timestamp(),
            None => false,
        }
    }

    pub fn global_scope(env: Env) -> BytesN<32> {
        global_scope(&env)
    }

    // ── deterministic evaluation ──────────────────────────────────────────

    /// #1066/#1068 — evaluate whether `subject` currently meets `program_id`'s
    /// latest published eligibility rule: every required attestation type
    /// must have a valid attestation for `subject`, either scoped to this
    /// program or globally scoped. Deterministic: depends only on current
    /// on-chain state, no randomness or off-chain input.
    pub fn evaluate_eligibility(
        env: Env,
        program_id: BytesN<32>,
        subject: Address,
    ) -> Result<bool, ContractError> {
        let latest_version: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::RuleVersion(program_id.clone()))
            .ok_or(ContractError::NoRulePublished)?;
        let rule: EligibilityRule = env
            .storage()
            .persistent()
            .get(&DataKey::Rule(program_id.clone(), latest_version))
            .ok_or(ContractError::RuleNotFound)?;

        let scope = global_scope(&env);
        for attestation_type in rule.required_types.iter() {
            let satisfied = Self::has_valid_attestation(
                env.clone(),
                subject.clone(),
                program_id.clone(),
                attestation_type.clone(),
            ) || Self::has_valid_attestation(
                env.clone(),
                subject.clone(),
                scope.clone(),
                attestation_type,
            );
            if !satisfied {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;

// Issue #1146 — error-path coverage.
#[cfg(test)]
mod error_tests;
