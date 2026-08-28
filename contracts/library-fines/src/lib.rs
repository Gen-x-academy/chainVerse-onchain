#![no_std]
//! #978 — Create signed fine assessments.
//! #979 — Enforce fine caps and grace periods.
//!
//! The institution (off-chain) signs fine assessments with an ed25519 key
//! registered in the contract. Each signed payload commits to: loan id, policy
//! id, rule version, amount, loan start timestamp, a one-time nonce, and an
//! expiry. The domain separator `CHAINVERSE_FINE:` prevents cross-contract
//! replay. The nonce is consumed on first use; re-submitting the same nonce
//! returns `NonceAlreadyUsed` and changes no state.
//!
//! Each policy stores a per-assessment cap, a cumulative debt cap, and a grace
//! period. An assessment whose `amount` exceeds the per-assessment cap is
//! rejected with `CapExceeded`. An assessment that would push the loan's total
//! debt above the cumulative cap is rejected with `CumulativeCapExceeded`. If
//! `now < loan_start + grace_period` the assessment is rejected with
//! `GracePeriodActive` — fines cannot accrue during the grace window.
//!
//! Waivers reduce a fine's recorded amount and the loan's cumulative debt.
//! Neither value may go below zero: a waiver exceeding the fine's current
//! amount returns `WaiverExceedsDebt`.
//!
//! ## ABI
//! `initialize`, `rotate_institution_key`, `set_policy`, `assess_fine`,
//! `waive_fine`, `get_fine`, `cumulative_debt`.
//!
//! ## Storage
//! Instance: `Admin`, `InstitutionPubkey`.
//! Persistent (TTL-tiered): `Policy(id)`, `Fine(id)`, `NonceUsed(nonce)`,
//! `LoanDebt(loan_id)`.
//!
//! ## Events
//! `FINE_NEW`, `FINE_WAV`, `POL_SET`.
//!
//! ## Migration
//! New independent contract; no prior on-chain state.

const FINE_MIN_TTL: u32 = 100_000;
const FINE_MAX_TTL: u32 = 500_000;

use ed25519_dalek::{Signature, VerifyingKey};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum FineError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    InvalidSignature = 5,
    NonceAlreadyUsed = 6,
    AssessmentExpired = 7,
    CapExceeded = 8,
    CumulativeCapExceeded = 9,
    GracePeriodActive = 10,
    WaiverExceedsDebt = 11,
    InvalidAmount = 12,
    Overflow = 13,
    PolicyNotFound = 14,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FineStatus {
    Active,
    Waived,
    Settled,
}

/// Per-policy parameters governing fine assessments.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinePolicy {
    /// Maximum amount allowed for a single signed assessment.
    pub per_assessment_cap: i128,
    /// Maximum cumulative debt a single loan may accrue under this policy.
    pub cumulative_cap: i128,
    /// Seconds after `loan_start` during which fines cannot accrue.
    pub grace_period: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FineRecord {
    pub loan_id: BytesN<32>,
    pub policy_id: BytesN<32>,
    pub rule_version: u32,
    pub amount: i128,
    pub accrued_at: u64,
    pub status: FineStatus,
}

#[contracttype]
pub enum DataKey {
    Admin,
    InstitutionPubkey,
    FineCount,
    Policy(BytesN<32>),
    Fine(BytesN<32>),
    NonceUsed(BytesN<32>),
    LoanDebt(BytesN<32>),
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), FineError> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(FineError::NotInitialized)?;
    if *caller != admin {
        return Err(FineError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// Build the domain-separated payload the institution signs. The prefix
/// `CHAINVERSE_FINE:` prevents cross-contract replay.
fn build_fine_payload(
    env: &Env,
    loan_id: &BytesN<32>,
    policy_id: &BytesN<32>,
    rule_version: u32,
    amount: i128,
    loan_start: u64,
    nonce: &BytesN<32>,
    expiry: u64,
) -> Bytes {
    let mut msg = Bytes::new(env);
    msg.append(&Bytes::from_slice(env, b"CHAINVERSE_FINE:"));
    msg.append(&Bytes::from_slice(env, &loan_id.to_array()));
    msg.append(&Bytes::from_slice(env, &policy_id.to_array()));
    msg.append(&Bytes::from_slice(env, &rule_version.to_be_bytes()));
    msg.append(&Bytes::from_slice(env, &amount.to_be_bytes()));
    msg.append(&Bytes::from_slice(env, &loan_start.to_be_bytes()));
    msg.append(&Bytes::from_slice(env, &nonce.to_array()));
    msg.append(&Bytes::from_slice(env, &expiry.to_be_bytes()));
    msg
}

/// Collision-resistant fine id derivation (ADR-0001 I3).
fn next_fine_id(env: &Env, loan_id: &BytesN<32>, nonce: &BytesN<32>) -> Result<BytesN<32>, FineError> {
    let n: u64 = env
        .storage()
        .instance()
        .get(&DataKey::FineCount)
        .unwrap_or(0u64);
    let next = n.checked_add(1).ok_or(FineError::Overflow)?;
    env.storage().instance().set(&DataKey::FineCount, &next);
    let mut input = Bytes::new(env);
    input.append(&Bytes::from_slice(env, &next.to_be_bytes()));
    input.append(&Bytes::from_slice(env, &env.ledger().timestamp().to_be_bytes()));
    input.append(&Bytes::from_slice(env, &loan_id.to_array()));
    input.append(&Bytes::from_slice(env, &nonce.to_array()));
    Ok(env.crypto().sha256(&input).into())
}

fn save_fine(env: &Env, id: &BytesN<32>, record: &FineRecord) {
    env.storage()
        .persistent()
        .set(&DataKey::Fine(id.clone()), record);
    env.storage().persistent().extend_ttl(
        &DataKey::Fine(id.clone()),
        FINE_MIN_TTL,
        FINE_MAX_TTL,
    );
}

fn loan_debt(env: &Env, loan_id: &BytesN<32>) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::LoanDebt(loan_id.clone()))
        .unwrap_or(0i128)
}

fn set_loan_debt(env: &Env, loan_id: &BytesN<32>, debt: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::LoanDebt(loan_id.clone()), &debt);
    env.storage().persistent().extend_ttl(
        &DataKey::LoanDebt(loan_id.clone()),
        FINE_MIN_TTL,
        FINE_MAX_TTL,
    );
}

#[contract]
pub struct LibraryFines;

#[contractimpl]
impl LibraryFines {
    /// One-time bootstrap: registers the admin and the institution's ed25519
    /// public key used to verify signed fine assessments.
    pub fn initialize(
        env: Env,
        admin: Address,
        institution_pubkey: BytesN<32>,
    ) -> Result<(), FineError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(FineError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::InstitutionPubkey, &institution_pubkey);
        Ok(())
    }

    /// Admin-only: rotate the institution's signing key.
    pub fn rotate_institution_key(
        env: Env,
        caller: Address,
        new_pubkey: BytesN<32>,
    ) -> Result<(), FineError> {
        require_admin(&env, &caller)?;
        env.storage()
            .instance()
            .set(&DataKey::InstitutionPubkey, &new_pubkey);
        Ok(())
    }

    /// Admin-only: create or update a fine policy.
    pub fn set_policy(
        env: Env,
        caller: Address,
        policy_id: BytesN<32>,
        per_assessment_cap: i128,
        cumulative_cap: i128,
        grace_period: u64,
    ) -> Result<(), FineError> {
        require_admin(&env, &caller)?;
        if per_assessment_cap <= 0 || cumulative_cap <= 0 {
            return Err(FineError::InvalidAmount);
        }
        let policy = FinePolicy {
            per_assessment_cap,
            cumulative_cap,
            grace_period,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Policy(policy_id.clone()), &policy);
        env.storage().persistent().extend_ttl(
            &DataKey::Policy(policy_id.clone()),
            FINE_MIN_TTL,
            FINE_MAX_TTL,
        );
        env.events().publish(
            (symbol_short!("POL_SET"),),
            (policy_id, per_assessment_cap, cumulative_cap, grace_period),
        );
        Ok(())
    }

    /// Accept an institution-signed fine assessment. Validation order:
    /// 1. Signature verification (domain-separated ed25519).
    /// 2. Expiry check — a stale assessment is rejected before nonce consumption.
    /// 3. Nonce consumption — replay prevention.
    /// 4. Grace period — fines cannot accrue during `loan_start + grace_period`.
    /// 5. Per-assessment cap — `amount` must not exceed `policy.per_assessment_cap`.
    /// 6. Cumulative cap — total loan debt must not exceed `policy.cumulative_cap`.
    ///
    /// If any check fails, no state is changed (invalid proofs change no state).
    pub fn assess_fine(
        env: Env,
        loan_id: BytesN<32>,
        policy_id: BytesN<32>,
        rule_version: u32,
        amount: i128,
        loan_start: u64,
        nonce: BytesN<32>,
        expiry: u64,
        sig: BytesN<64>,
    ) -> Result<BytesN<32>, FineError> {
        if amount <= 0 {
            return Err(FineError::InvalidAmount);
        }

        // 1. Verify ed25519 signature before consuming any state.
        let pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::InstitutionPubkey)
            .ok_or(FineError::NotInitialized)?;
        let payload = build_fine_payload(
            &env,
            &loan_id,
            &policy_id,
            rule_version,
            amount,
            loan_start,
            &nonce,
            expiry,
        );
        let pubkey_arr: [u8; 32] = pubkey.to_array();
        let vk = VerifyingKey::from_bytes(&pubkey_arr)
            .map_err(|_| FineError::InvalidSignature)?;
        let sig_arr: [u8; 64] = sig.to_array();
        let signature = Signature::from_bytes(&sig_arr);
        let mut payload_arr = [0u8; 148];
        payload.copy_into_slice(&mut payload_arr);
        vk.verify_strict(&payload_arr, &signature)
            .map_err(|_| FineError::InvalidSignature)?;

        // 2. Expiry check.
        let now = env.ledger().timestamp();
        if now >= expiry {
            return Err(FineError::AssessmentExpired);
        }

        // 3. Nonce consumption — reject replay before any writes.
        let nonce_key = DataKey::NonceUsed(nonce.clone());
        let already_used: bool = env
            .storage()
            .persistent()
            .get(&nonce_key)
            .unwrap_or(false);
        if already_used {
            return Err(FineError::NonceAlreadyUsed);
        }
        env.storage().persistent().set(&nonce_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&nonce_key, FINE_MIN_TTL, FINE_MAX_TTL);

        // 4. Load and validate policy.
        let policy: FinePolicy = env
            .storage()
            .persistent()
            .get(&DataKey::Policy(policy_id.clone()))
            .ok_or(FineError::PolicyNotFound)?;

        // 5. Grace period — fines cannot accrue until `loan_start + grace_period`.
        let grace_end = loan_start
            .checked_add(policy.grace_period)
            .ok_or(FineError::Overflow)?;
        if now < grace_end {
            return Err(FineError::GracePeriodActive);
        }

        // 6. Per-assessment cap.
        if amount > policy.per_assessment_cap {
            return Err(FineError::CapExceeded);
        }

        // 7. Cumulative cap.
        let current_debt = loan_debt(&env, &loan_id);
        let new_debt = current_debt
            .checked_add(amount)
            .ok_or(FineError::Overflow)?;
        if new_debt > policy.cumulative_cap {
            return Err(FineError::CumulativeCapExceeded);
        }

        // All checks passed — write state.
        let fine_id = next_fine_id(&env, &loan_id, &nonce)?;
        let record = FineRecord {
            loan_id: loan_id.clone(),
            policy_id: policy_id.clone(),
            rule_version,
            amount,
            accrued_at: now,
            status: FineStatus::Active,
        };
        save_fine(&env, &fine_id, &record);
        set_loan_debt(&env, &loan_id, new_debt);

        env.events().publish(
            (symbol_short!("FINE_NEW"),),
            (fine_id.clone(), loan_id, policy_id, amount),
        );
        Ok(fine_id)
    }

    /// Admin-only: waive up to `waive_amount` of a fine. Neither the fine's
    /// recorded amount nor the loan's cumulative debt may go below zero
    /// — `WaiverExceedsDebt` is returned if `waive_amount > fine.amount`.
    pub fn waive_fine(
        env: Env,
        caller: Address,
        fine_id: BytesN<32>,
        waive_amount: i128,
    ) -> Result<(), FineError> {
        require_admin(&env, &caller)?;
        if waive_amount <= 0 {
            return Err(FineError::InvalidAmount);
        }
        let mut record: FineRecord = env
            .storage()
            .persistent()
            .get(&DataKey::Fine(fine_id.clone()))
            .ok_or(FineError::NotFound)?;
        // Waiver must not exceed the fine's current amount — no negative debt.
        if waive_amount > record.amount {
            return Err(FineError::WaiverExceedsDebt);
        }
        let new_amount = record
            .amount
            .checked_sub(waive_amount)
            .ok_or(FineError::Overflow)?;
        let current_debt = loan_debt(&env, &record.loan_id);
        let new_debt = current_debt
            .checked_sub(waive_amount)
            .ok_or(FineError::Overflow)?;

        record.amount = new_amount;
        if new_amount == 0 {
            record.status = FineStatus::Waived;
        }
        save_fine(&env, &fine_id, &record);
        set_loan_debt(&env, &record.loan_id, new_debt);

        env.events().publish(
            (symbol_short!("FINE_WAV"),),
            (fine_id, waive_amount, new_amount),
        );
        Ok(())
    }

    pub fn get_fine(env: Env, fine_id: BytesN<32>) -> Result<FineRecord, FineError> {
        env.storage()
            .persistent()
            .get(&DataKey::Fine(fine_id))
            .ok_or(FineError::NotFound)
    }

    /// Returns the total outstanding debt for `loan_id` across all active fines.
    pub fn cumulative_debt(env: Env, loan_id: BytesN<32>) -> i128 {
        loan_debt(&env, &loan_id)
    }
}

#[cfg(test)]
mod tests;
