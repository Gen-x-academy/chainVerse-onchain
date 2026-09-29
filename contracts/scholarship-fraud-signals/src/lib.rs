#![no_std]

mod errors;
mod storage;
mod types;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::{
    FraudSignal, FraudSignalSummary, SignalCategory, SignalStatus,
};

use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, String};

use storage::{
    assert_not_paused, bump_instance, get_signal, require_admin, require_detector,
    require_reviewer, save_signal, set_admin, set_detector, set_paused, set_reviewer,
    signal_exists,
};
use types::{
    FraudSignal as FS, FraudSignalSummary as FSS, SignalCategory as SC,
    SignalStatus as SS,
};

const MAX_APP_ID_BYTES: u32 = 64;
const MAX_DESCRIPTION_BYTES: u32 = 512;
const MAX_EVIDENCE_REF_BYTES: u32 = 256;
const MAX_NOTES_BYTES: u32 = 1_024;

#[contract]
pub struct ScholarshipFraudSignals;

#[contractimpl]
impl ScholarshipFraudSignals {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract.
    ///
    /// * `detector`  — trusted service account that may raise signals.
    /// * `reviewer`  — human reviewer who may resolve or respond to appeals.
    ///
    /// # Privacy design
    /// This contract stores **only** opaque application identifiers and
    /// advisory signals.  Biometric data, government-issued IDs, and any
    /// other sensitive personal data must never be stored here.
    /// Evidence must be referenced via content-addressed off-chain pointers.
    ///
    /// # Auto-reject prohibition
    /// Signals raised by this contract carry **no enforcement power**.
    /// No downstream contract or off-chain system may automatically reject
    /// an application based solely on the presence of a signal.  All signals
    /// must route through human review before any adverse action.
    pub fn init(
        env: Env,
        admin: Address,
        detector: Address,
        reviewer: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        set_detector(&env, &detector);
        set_reviewer(&env, &reviewer);
        set_paused(&env, false);

        env.events()
            .publish((symbol_short!("FS_INIT"),), (admin, detector, reviewer));
        Ok(())
    }

    // ── Signal lifecycle ──────────────────────────────────────────────────────

    /// Raise a new fraud signal.
    ///
    /// Only the designated detector service account may call this.
    /// If a signal already exists for `app_id`, the call returns
    /// `SignalAlreadyExists` — use `update_signal` to amend an existing one.
    ///
    /// `risk_score` must be in [0, 100].  A score of 0 is allowed (signal
    /// raised for monitoring purposes with no current evidence of risk).
    ///
    /// # No auto-reject
    /// Raising a signal never triggers an automatic rejection.  The signal
    /// status starts as `Flagged` and must be reviewed by a human before
    /// any adverse action is taken.
    pub fn raise_signal(
        env: Env,
        caller: Address,
        app_id: String,
        category: SignalCategory,
        description: String,
        evidence_ref: String,
        risk_score: u32,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_detector(&env, &caller)?;

        // Bounds checks
        if app_id.len() > MAX_APP_ID_BYTES {
            return Err(ContractError::AppIdTooLong);
        }
        if description.len() > MAX_DESCRIPTION_BYTES {
            return Err(ContractError::DescriptionTooLong);
        }
        if evidence_ref.len() > MAX_EVIDENCE_REF_BYTES {
            return Err(ContractError::EvidenceRefTooLong);
        }
        if risk_score > 100 {
            return Err(ContractError::InvalidScore);
        }

        if signal_exists(&env, &app_id) {
            return Err(ContractError::SignalAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let sig = FS {
            app_id: app_id.clone(),
            category: category.clone(),
            status: SS::Flagged,
            description,
            evidence_ref,
            risk_score,
            raised_by: caller.clone(),
            raised_at: now,
            reviewer: None,
            reviewer_notes: String::from_str(&env, ""),
            reviewed_at: 0,
            has_appeal: false,
            appeal_statement: String::from_str(&env, ""),
            appealed_at: 0,
        };
        save_signal(&env, &sig);

        env.events().publish(
            (symbol_short!("FS_RAIS"),),
            (app_id, category, risk_score, caller, now),
        );
        Ok(())
    }

    /// Update an existing signal's evidence reference and risk score.
    ///
    /// Only the detector may call this.  Status is not changed by an update —
    /// only `evidence_ref` and `risk_score` may be revised.
    pub fn update_signal(
        env: Env,
        caller: Address,
        app_id: String,
        evidence_ref: String,
        risk_score: u32,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_detector(&env, &caller)?;

        if evidence_ref.len() > MAX_EVIDENCE_REF_BYTES {
            return Err(ContractError::EvidenceRefTooLong);
        }
        if risk_score > 100 {
            return Err(ContractError::InvalidScore);
        }

        let mut sig = get_signal(&env, &app_id).ok_or(ContractError::SignalNotFound)?;
        sig.evidence_ref = evidence_ref.clone();
        sig.risk_score = risk_score;
        save_signal(&env, &sig);

        env.events().publish(
            (symbol_short!("FS_UPDT"),),
            (app_id, risk_score, caller),
        );
        Ok(())
    }

    /// Record a human reviewer's resolution of a signal.
    ///
    /// Only the designated reviewer may call this.
    /// The signal must be in `Flagged` or `UnderAppeal` status.
    /// Setting `resolved_status` to `Confirmed` means the signal is upheld;
    /// `Dismissed` clears it as a false positive.
    ///
    /// # Explainability
    /// `notes` must contain enough detail for the applicant or an independent
    /// auditor to understand why the signal was confirmed or dismissed.
    pub fn resolve_signal(
        env: Env,
        caller: Address,
        app_id: String,
        resolved_status: SignalStatus,
        notes: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_reviewer(&env, &caller)?;

        if notes.len() > MAX_NOTES_BYTES {
            return Err(ContractError::NotesTooLong);
        }

        let mut sig = get_signal(&env, &app_id).ok_or(ContractError::SignalNotFound)?;

        // Only Flagged or UnderAppeal signals are reviewable
        if sig.status != SS::Flagged && sig.status != SS::UnderAppeal {
            return Err(ContractError::SignalNotFlagged);
        }

        // Only allow valid terminal / appeal-response statuses
        let allowed = matches!(
            resolved_status,
            SS::Confirmed | SS::Dismissed | SS::AppealUpheld | SS::AppealRejected
        );
        if !allowed {
            return Err(ContractError::Unauthorized);
        }

        sig.status = resolved_status.clone();
        sig.reviewer = Some(caller.clone());
        sig.reviewer_notes = notes;
        sig.reviewed_at = env.ledger().timestamp();
        save_signal(&env, &sig);

        env.events().publish(
            (symbol_short!("FS_RESV"),),
            (app_id, resolved_status, caller, sig.reviewed_at),
        );
        Ok(())
    }

    // ── Appeal lifecycle ──────────────────────────────────────────────────────

    /// File an appeal on behalf of an applicant.
    ///
    /// The `appeal_statement` must be a reference to the off-chain statement
    /// (e.g. a CID) or a brief note — not raw applicant PII.
    ///
    /// Any party authorised by the admin (typically the applicant's
    /// case-management service) may file an appeal.  We use the admin here
    /// for simplicity; production deployments may choose to add a dedicated
    /// `case_manager` role.
    pub fn file_appeal(
        env: Env,
        caller: Address,
        app_id: String,
        appeal_statement: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        // Admin-level permission required to file an appeal to prevent
        // griefing by arbitrary actors.
        require_admin(&env, &caller)?;

        if appeal_statement.len() > MAX_NOTES_BYTES {
            return Err(ContractError::NotesTooLong);
        }

        let mut sig = get_signal(&env, &app_id).ok_or(ContractError::SignalNotFound)?;
        if sig.has_appeal {
            return Err(ContractError::AppealAlreadyFiled);
        }

        let now = env.ledger().timestamp();
        sig.has_appeal = true;
        sig.appeal_statement = appeal_statement;
        sig.appealed_at = now;
        sig.status = SS::UnderAppeal;
        save_signal(&env, &sig);

        env.events()
            .publish((symbol_short!("FS_APPL"),), (app_id, caller, now));
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Return the public summary of a signal (no raw evidence_ref or reviewer
    /// notes to limit over-exposure).
    pub fn get_signal_summary(
        env: Env,
        app_id: String,
    ) -> Result<FraudSignalSummary, ContractError> {
        bump_instance(&env);
        let sig = get_signal(&env, &app_id).ok_or(ContractError::SignalNotFound)?;
        Ok(FSS {
            app_id: sig.app_id,
            category: sig.category,
            status: sig.status,
            description: sig.description,
            risk_score: sig.risk_score,
            raised_at: sig.raised_at,
            reviewed_at: sig.reviewed_at,
            has_appeal: sig.has_appeal,
        })
    }

    /// Return the full signal record.  Should be gated off-chain to authorised
    /// reviewers only; the contract itself cannot restrict off-chain reads.
    pub fn get_signal_full(
        env: Env,
        app_id: String,
    ) -> Result<FraudSignal, ContractError> {
        bump_instance(&env);
        get_signal(&env, &app_id).ok_or(ContractError::SignalNotFound)
    }

    pub fn paused(env: Env) -> bool {
        bump_instance(&env);
        storage::is_paused(&env)
    }

    // ── Admin operations ──────────────────────────────────────────────────────

    pub fn pause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, true);
        env.events().publish((symbol_short!("FS_PAUS"),), caller);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, false);
        env.events().publish((symbol_short!("FS_UPAU"),), caller);
        Ok(())
    }

    pub fn set_detector(
        env: Env,
        caller: Address,
        new_detector: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_detector(&env, &new_detector);
        env.events()
            .publish((symbol_short!("FS_SDET"),), (caller, new_detector));
        Ok(())
    }

    pub fn set_reviewer(
        env: Env,
        caller: Address,
        new_reviewer: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_reviewer(&env, &new_reviewer);
        env.events()
            .publish((symbol_short!("FS_SRVW"),), (caller, new_reviewer));
        Ok(())
    }

    pub fn upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        require_admin(&env, &caller)?;
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        env.events()
            .publish((symbol_short!("FS_UPGD"),), new_wasm_hash);
        Ok(())
    }
}
