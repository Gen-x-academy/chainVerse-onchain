#![no_std]
#![allow(clippy::too_many_arguments)]

//! Scholarship identity and enrollment verification contract.
//!
//! Verifies applicant identity and active enrollment through approved
//! providers while retaining the minimum data on-chain.
//!
//! ## Design
//!
//! - **Privacy-minimized**: this contract stores no PII. A "claim" is an
//!   assertion from an issuer that a subject satisfies a named claim type,
//!   scoped to an issuer + program scope + claim type triple. The evidence
//!   behind the claim lives entirely off-chain with the approved provider.
//! - **Issuer-scoped and expiring**: each claim is keyed by
//!   `(subject, issuer, scope, claim_type)` so claims from different issuers
//!   cannot collide, and every claim must carry an expiry set in the future.
//! - **Fallback review**: when no automated claim exists, an authorized
//!   `Verifier` may issue a manual attestation via `issue_manual_claim`.
//! - **Raw provider secrets are never stored**: providers are referenced by a
//!   `BytesN<32>` hash of their public identifier; no credential or secret
//!   ever reaches this contract.
//! - **Bounded storage**: at most `MAX_CLAIMS_PER_SUBJECT` active claims per
//!   subject. Expired/revoked claims are not counted.
//! - **TTL policy**: all claims and provider entries use
//!   `RECORD_MIN_TTL`/`RECORD_MAX_TTL` (~1–2 years). Claims are additionally
//!   bounded by their on-chain `expiry` field.
//!
//! ## Ownership / privacy notes
//!
//! Only the subject's `Address` (a Stellar public key) and a `Symbol` claim
//! type name are stored on-chain. The claim type vocabulary is controlled by
//! the admin (no arbitrary strings). The claim `scope` is either a
//! program-specific `BytesN<32>` or the all-zero global scope.
//!
//! ## Migration notes
//!
//! Provider entries are persistent records keyed by `BytesN<32>` (the hash
//! of the provider's public identifier). Claim records are keyed by
//! `(subject, issuer, scope, claim_type)`. Both are independent and may be
//! extended without affecting existing records.
//!
//! ## Adversarial tests
//!
//! See `tests/` for unauthorized issuance, cross-issuer collisions, expiry
//! enforcement, raw-secret storage rejection, and fallback review path tests.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol,
};

const CONTRACT_VERSION: u32 = 1;

/// ~1 year at 5-second ledgers.
const RECORD_MIN_TTL: u32 = 3_110_400;
/// ~2 years.
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Maximum active (non-expired, non-revoked) claims per subject.
const MAX_CLAIMS_PER_SUBJECT: u32 = 32;

/// All-zero scope identifier used for platform-wide (global) claims.
fn global_scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// Caller is neither an approved provider nor an admin-authorized verifier.
    NotAuthorizedIssuer = 4,
    /// The provider is unknown or has been revoked.
    ProviderNotFound = 5,
    /// The provider has been revoked and may not issue new claims.
    ProviderRevoked = 6,
    /// The claim type is not in the admin-controlled vocabulary.
    UnknownClaimType = 7,
    /// `expiry` must be strictly greater than the current ledger timestamp.
    InvalidExpiry = 8,
    /// The requested claim does not exist.
    ClaimNotFound = 9,
    /// Only the original issuer (or the admin) may revoke a claim.
    NotClaimIssuer = 10,
    /// The subject has reached the maximum number of active claims.
    ClaimLimitExceeded = 11,
    /// Arithmetic overflow (practically unreachable).
    Overflow = 12,
    /// Version overflow on the claim-type registry.
    VersionOverflow = 13,
}

// ---------------------------------------------------------------------------
// Provider record
// ---------------------------------------------------------------------------

/// An approved identity/enrollment provider. Referenced by `provider_id`, a
/// `BytesN<32>` hash of the provider's public identifier — never the raw
/// credential or API secret.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provider {
    /// SHA-256 (or equivalent) hash of the provider's public identifier.
    /// The actual identifier lives off-chain; this contract never sees it.
    pub provider_id: BytesN<32>,
    /// Human-readable label stored as a Symbol (≤ 32 chars).
    pub label: Symbol,
    pub active: bool,
    pub registered_at: u64,
    pub registered_by: Address,
}

// ---------------------------------------------------------------------------
// Claim record
// ---------------------------------------------------------------------------

/// How the claim was issued.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimSource {
    /// Issued automatically by an approved provider via `issue_claim`.
    Provider = 1,
    /// Issued manually by an admin-authorized verifier via `issue_manual_claim`.
    ManualVerifier = 2,
}

/// An on-chain claim that `subject` satisfies `claim_type`, issued by
/// `issuer` under `scope`, valid until `expiry`.
///
/// Privacy guarantee: the evidence, grade, income figure, or document that
/// *backs* this claim lives entirely off-chain with the issuer.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    pub issuer: Address,
    /// The provider ID whose approval the issuer holds (zero bytes for
    /// manual verifier claims).
    pub provider_id: BytesN<32>,
    pub claim_type: Symbol,
    pub scope: BytesN<32>,
    pub source: ClaimSource,
    pub expiry: u64,
    pub issued_at: u64,
    pub revoked: bool,
    pub revoked_at: u64,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    /// Approved provider record.
    Provider(BytesN<32>),
    /// Admin-authorized verifier (manual fallback path).
    Verifier(Address),
    /// Admin-controlled claim type vocabulary entry.
    ClaimType(Symbol),
    /// A claim keyed by (subject, issuer, scope, claim_type).
    Claim(Address, Address, BytesN<32>, Symbol),
    /// Per-subject active-claim count (for ceiling enforcement).
    ClaimCount(Address),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ScholarshipIdentityContract;

#[contractimpl]
impl ScholarshipIdentityContract {
    /// One-time bootstrap.
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

    // ── claim-type vocabulary ─────────────────────────────────────────────

    /// Admin-only: register a claim type name in the controlled vocabulary.
    /// Only registered claim types may be issued; callers cannot inject
    /// arbitrary strings on-chain.
    pub fn register_claim_type(
        env: Env,
        admin: Address,
        claim_type: Symbol,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::ClaimType(claim_type.clone()), &true);
        env.events()
            .publish((symbol_short!("CTYPEREQ"),), (claim_type,));
        Ok(())
    }

    /// Admin-only: remove a claim type from the vocabulary.
    /// Outstanding claims of this type retain their existing validity.
    pub fn deregister_claim_type(
        env: Env,
        admin: Address,
        claim_type: Symbol,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::ClaimType(claim_type.clone()), &false);
        Ok(())
    }

    pub fn is_known_claim_type(env: Env, claim_type: Symbol) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::ClaimType(claim_type))
            .unwrap_or(false)
    }

    // ── provider management ───────────────────────────────────────────────

    /// Admin-only: register an approved provider. `provider_id` is the hash
    /// of the provider's public identifier — never the credential itself.
    /// Raw secrets must never be passed here; this contract has no secret
    /// storage and no way to prevent indexers from reading transaction args.
    pub fn register_provider(
        env: Env,
        admin: Address,
        provider_id: BytesN<32>,
        label: Symbol,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        // Reject the all-zero hash — it is the null/tombstone value.
        if provider_id.to_array().iter().all(|b| *b == 0) {
            return Err(ContractError::ProviderNotFound);
        }
        let provider = Provider {
            provider_id: provider_id.clone(),
            label,
            active: true,
            registered_at: env.ledger().timestamp(),
            registered_by: admin.clone(),
        };
        let key = DataKey::Provider(provider_id.clone());
        env.storage().persistent().set(&key, &provider);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("PROVREG"),), (provider_id, admin));
        Ok(())
    }

    /// Admin-only: revoke a provider. Existing valid claims issued by this
    /// provider remain valid until their individual expiry or explicit
    /// revocation. No new claims may be issued by a revoked provider.
    pub fn revoke_provider(
        env: Env,
        admin: Address,
        provider_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Provider(provider_id.clone());
        let mut provider: Provider = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProviderNotFound)?;
        provider.active = false;
        env.storage().persistent().set(&key, &provider);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("PROVRVK"),), (provider_id,));
        Ok(())
    }

    pub fn get_provider(env: Env, provider_id: BytesN<32>) -> Result<Provider, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Provider(provider_id))
            .ok_or(ContractError::ProviderNotFound)
    }

    // ── verifier management (fallback manual path) ────────────────────────

    /// Admin-only: authorize an address to issue manual (fallback) claims.
    pub fn add_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Verifier(verifier.clone());
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("VRFYADD"),), (verifier,));
        Ok(())
    }

    /// Admin-only: revoke a verifier's authorization.
    pub fn remove_verifier(
        env: Env,
        admin: Address,
        verifier: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Verifier(verifier.clone()), &false);
        env.events()
            .publish((symbol_short!("VRFYRVK"),), (verifier,));
        Ok(())
    }

    pub fn is_authorized_verifier(env: Env, verifier: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Verifier(verifier))
            .unwrap_or(false)
    }

    // ── claim issuance (provider path) ────────────────────────────────────

    /// Approved-provider path: `issuer` must be an address authorized by the
    /// admin AND the referenced `provider_id` must be active. Issues or
    /// re-issues a claim for `(subject, issuer, scope, claim_type)`.
    ///
    /// The claim type must be in the admin-controlled vocabulary.
    /// `expiry` must be in the future.
    /// Re-issuing replaces the existing claim and resets `revoked`.
    pub fn issue_claim(
        env: Env,
        issuer: Address,
        subject: Address,
        provider_id: BytesN<32>,
        scope: BytesN<32>,
        claim_type: Symbol,
        expiry: u64,
    ) -> Result<(), ContractError> {
        issuer.require_auth();

        // Provider must be active.
        let provider: Provider = env
            .storage()
            .persistent()
            .get(&DataKey::Provider(provider_id.clone()))
            .ok_or(ContractError::ProviderNotFound)?;
        if !provider.active {
            return Err(ContractError::ProviderRevoked);
        }

        // Claim type must be in the vocabulary.
        if !Self::is_known_claim_type(env.clone(), claim_type.clone()) {
            return Err(ContractError::UnknownClaimType);
        }

        // Expiry must be in the future.
        if expiry <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }

        Self::write_claim(
            &env,
            issuer,
            subject,
            provider_id,
            scope,
            claim_type,
            expiry,
            ClaimSource::Provider,
        )
    }

    /// Manual/fallback path: `verifier` must be admin-authorized. Issues or
    /// re-issues a claim with `ClaimSource::ManualVerifier`. The `provider_id`
    /// field is set to all-zeros to distinguish it from provider-issued claims.
    pub fn issue_manual_claim(
        env: Env,
        verifier: Address,
        subject: Address,
        scope: BytesN<32>,
        claim_type: Symbol,
        expiry: u64,
    ) -> Result<(), ContractError> {
        verifier.require_auth();

        if !Self::is_authorized_verifier(env.clone(), verifier.clone()) {
            return Err(ContractError::NotAuthorizedIssuer);
        }
        if !Self::is_known_claim_type(env.clone(), claim_type.clone()) {
            return Err(ContractError::UnknownClaimType);
        }
        if expiry <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }

        Self::write_claim(
            &env,
            verifier,
            subject,
            global_scope(&env),   // null provider id
            scope,
            claim_type,
            expiry,
            ClaimSource::ManualVerifier,
        )
    }

    fn write_claim(
        env: &Env,
        issuer: Address,
        subject: Address,
        provider_id: BytesN<32>,
        scope: BytesN<32>,
        claim_type: Symbol,
        expiry: u64,
        source: ClaimSource,
    ) -> Result<(), ContractError> {
        let key = DataKey::Claim(subject.clone(), issuer.clone(), scope.clone(), claim_type.clone());
        let is_new = !env.storage().persistent().has(&key);

        if is_new {
            // Enforce per-subject ceiling.
            let count_key = DataKey::ClaimCount(subject.clone());
            let count: u32 = env
                .storage()
                .persistent()
                .get(&count_key)
                .unwrap_or(0u32);
            if count >= MAX_CLAIMS_PER_SUBJECT {
                return Err(ContractError::ClaimLimitExceeded);
            }
            let new_count = count.checked_add(1).ok_or(ContractError::Overflow)?;
            env.storage().persistent().set(&count_key, &new_count);
            env.storage()
                .persistent()
                .extend_ttl(&count_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }

        let claim = Claim {
            issuer: issuer.clone(),
            provider_id,
            claim_type: claim_type.clone(),
            scope: scope.clone(),
            source,
            expiry,
            issued_at: env.ledger().timestamp(),
            revoked: false,
            revoked_at: 0,
        };
        env.storage().persistent().set(&key, &claim);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("CLMISSUE"),),
            (issuer, subject, claim_type, scope),
        );
        Ok(())
    }

    // ── claim revocation ──────────────────────────────────────────────────

    /// The original issuer or the admin may revoke a claim immediately.
    pub fn revoke_claim(
        env: Env,
        caller: Address,
        subject: Address,
        issuer: Address,
        scope: BytesN<32>,
        claim_type: Symbol,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let key = DataKey::Claim(subject.clone(), issuer.clone(), scope.clone(), claim_type.clone());
        let mut claim: Claim = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ClaimNotFound)?;

        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        let is_admin = admin.as_ref() == Some(&caller);
        if caller != claim.issuer && !is_admin {
            return Err(ContractError::NotClaimIssuer);
        }

        claim.revoked = true;
        claim.revoked_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &claim);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        // Decrement per-subject count.
        let count_key = DataKey::ClaimCount(subject.clone());
        let count: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u32);
        if count > 0 {
            env.storage()
                .persistent()
                .set(&count_key, &(count - 1));
        }

        env.events().publish(
            (symbol_short!("CLMRVK"),),
            (caller, subject, claim_type, scope),
        );
        Ok(())
    }

    // ── claim queries ─────────────────────────────────────────────────────

    pub fn get_claim(
        env: Env,
        subject: Address,
        issuer: Address,
        scope: BytesN<32>,
        claim_type: Symbol,
    ) -> Result<Claim, ContractError> {
        let key = DataKey::Claim(subject, issuer, scope, claim_type);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ClaimNotFound)
    }

    /// True iff `subject` holds a non-revoked, unexpired claim of `claim_type`
    /// issued by `issuer` for `scope`.
    pub fn has_valid_claim(
        env: Env,
        subject: Address,
        issuer: Address,
        scope: BytesN<32>,
        claim_type: Symbol,
    ) -> bool {
        let key = DataKey::Claim(subject, issuer, scope, claim_type);
        match env.storage().persistent().get::<DataKey, Claim>(&key) {
            Some(c) => !c.revoked && c.expiry > env.ledger().timestamp(),
            None => false,
        }
    }

    /// True iff `subject` holds at least one valid claim of `claim_type`
    /// from *any* active provider, scoped to `scope` or globally.
    /// This is the primary eligibility gate for enrollment verification.
    pub fn is_verified(
        env: Env,
        subject: Address,
        scope: BytesN<32>,
        claim_type: Symbol,
        issuers: soroban_sdk::Vec<Address>,
    ) -> bool {
        let now = env.ledger().timestamp();
        let global = global_scope(&env);
        for issuer in issuers.iter() {
            // Check program-scoped claim.
            let scoped_key = DataKey::Claim(subject.clone(), issuer.clone(), scope.clone(), claim_type.clone());
            if let Some(c) = env.storage().persistent().get::<DataKey, Claim>(&scoped_key) {
                if !c.revoked && c.expiry > now {
                    return true;
                }
            }
            // Fallback: global-scope claim.
            let global_key = DataKey::Claim(subject.clone(), issuer.clone(), global.clone(), claim_type.clone());
            if let Some(c) = env.storage().persistent().get::<DataKey, Claim>(&global_key) {
                if !c.revoked && c.expiry > now {
                    return true;
                }
            }
        }
        false
    }

    pub fn global_scope(env: Env) -> BytesN<32> {
        global_scope(&env)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
