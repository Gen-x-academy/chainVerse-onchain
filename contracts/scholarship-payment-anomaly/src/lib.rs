#![no_std]

mod errors;
mod storage;
mod types;
#[cfg(test)]
mod test;

pub use errors::ContractError;
pub use types::{AlertStatus, AlertSummary, AnomalyCategory, PaymentAlert};

use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, String};

use storage::{
    add_paused_amount, alert_exists, assert_not_paused, bump_instance, get_alert,
    get_total_alerts, get_total_paused_amount, increment_total_alerts, require_admin,
    require_detector, require_resolver, save_alert, set_admin, set_detector, set_paused,
    set_resolver, sub_paused_amount,
};
use types::{
    AlertStatus as AS, AlertSummary as ASu, AnomalyCategory as AC, PaymentAlert as PA,
};

const MAX_PAYMENT_REF_BYTES: u32 = 64;
const MAX_DESCRIPTION_BYTES: u32 = 512;
const MAX_EVIDENCE_REF_BYTES: u32 = 256;
const MAX_NOTES_BYTES: u32 = 1_024;

/// Default pause window: 72 hours in seconds.
const DEFAULT_PAUSE_WINDOW_SECS: u64 = 259_200;

#[contract]
pub struct ScholarshipPaymentAnomaly;

#[contractimpl]
impl ScholarshipPaymentAnomaly {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract.
    ///
    /// * `detector`  — trusted service that raises anomaly alerts.
    /// * `resolver`  — trusted human who clears or confirms alerts.
    ///
    /// # Safe-pause design
    /// This contract never holds funds.  It acts as a coordination layer:
    /// when an alert is raised with `pause_payment = true`, the calling
    /// disbursement contract or off-chain service is responsible for halting
    /// the payment until the alert is resolved.  The `is_payment_paused`
    /// function provides the canonical on-chain pause state.
    pub fn init(
        env: Env,
        admin: Address,
        detector: Address,
        resolver: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        if storage::get_admin(&env).is_some() {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        set_admin(&env, &admin);
        set_detector(&env, &detector);
        set_resolver(&env, &resolver);
        set_paused(&env, false);

        env.events()
            .publish((symbol_short!("PA_INIT"),), (admin, detector, resolver));
        Ok(())
    }

    // ── Alert lifecycle ───────────────────────────────────────────────────────

    /// Raise a payment anomaly alert.
    ///
    /// * `pause_payment`  — if `true`, marks the payment as paused.
    ///   Downstream systems must call `is_payment_paused` before disbursing.
    /// * `pause_duration_secs` — seconds from now for which the pause is
    ///   valid.  0 means use the default (72 h).  The pause is not enforced
    ///   on-chain after expiry — the resolver must explicitly clear it or
    ///   the disbursement system must respect the expiry timestamp.
    /// * `wallet_change_velocity` — number of wallet address changes observed
    ///   in the look-back window (0 = N/A).
    /// * `retry_count` — how many times this payment has failed and been
    ///   retried (0 = N/A).
    ///
    /// # Evidence requirement
    /// `evidence_ref` must point to an off-chain bundle that substantiates
    /// each claim in `description`.  An alert without an evidence reference
    /// is stored with `evidence_ref = ""` but resolvers should treat it as
    /// lower confidence.
    pub fn raise_alert(
        env: Env,
        caller: Address,
        payment_ref: String,
        category: AnomalyCategory,
        description: String,
        evidence_ref: String,
        amount: i128,
        destination: Address,
        pause_payment: bool,
        pause_duration_secs: u64,
        wallet_change_velocity: u32,
        retry_count: u32,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_detector(&env, &caller)?;

        // Bounds
        if payment_ref.len() > MAX_PAYMENT_REF_BYTES {
            return Err(ContractError::PaymentRefTooLong);
        }
        if description.len() > MAX_DESCRIPTION_BYTES {
            return Err(ContractError::DescriptionTooLong);
        }
        if evidence_ref.len() > MAX_EVIDENCE_REF_BYTES {
            return Err(ContractError::EvidenceRefTooLong);
        }
        if amount < 0 {
            return Err(ContractError::InvalidAmount);
        }
        if alert_exists(&env, &payment_ref) {
            return Err(ContractError::AlertAlreadyExists);
        }

        let now = env.ledger().timestamp();
        let pause_expires_at = if pause_payment {
            let window = if pause_duration_secs == 0 {
                DEFAULT_PAUSE_WINDOW_SECS
            } else {
                pause_duration_secs
            };
            now.saturating_add(window)
        } else {
            0
        };

        let alert = PA {
            payment_ref: payment_ref.clone(),
            category: category.clone(),
            status: AS::PausedPendingReview,
            description,
            evidence_ref,
            amount,
            destination: destination.clone(),
            payment_paused: pause_payment,
            pause_expires_at,
            raised_by: caller.clone(),
            raised_at: now,
            resolved_by: None,
            resolution_notes: String::from_str(&env, ""),
            resolved_at: 0,
            wallet_change_velocity,
            retry_count,
        };

        if pause_payment {
            add_paused_amount(&env, amount)?;
        }
        save_alert(&env, &alert);
        increment_total_alerts(&env);

        env.events().publish(
            (symbol_short!("PA_RAIS"),),
            (
                payment_ref,
                category,
                amount,
                destination,
                pause_payment,
                pause_expires_at,
                caller,
                now,
            ),
        );
        Ok(())
    }

    /// Resolve an alert — either clear or confirm it.
    ///
    /// Only the designated resolver may call this.  Resolution is audited
    /// via the `PA_RESV` event.
    ///
    /// * `resolution` — `Cleared` lets the payment proceed; `Confirmed`
    ///   keeps the block in place.
    /// * `notes` — mandatory explanation for the resolution (max 1 024 bytes).
    ///   Must reference the off-chain investigation report for `Confirmed`
    ///   resolutions.
    pub fn resolve_alert(
        env: Env,
        caller: Address,
        payment_ref: String,
        resolution: AlertStatus,
        notes: String,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        assert_not_paused(&env)?;
        require_resolver(&env, &caller)?;

        if notes.len() > MAX_NOTES_BYTES {
            return Err(ContractError::NotesTooLong);
        }

        let mut alert = get_alert(&env, &payment_ref)
            .ok_or(ContractError::AlertNotFound)?;

        // Terminal states cannot be re-resolved
        if alert.status == AS::Confirmed || alert.status == AS::Cleared {
            return Err(ContractError::AlertAlreadyResolved);
        }

        // Only Cleared and Confirmed are valid resolution outcomes
        let valid_resolution =
            matches!(resolution, AS::Cleared | AS::Confirmed);
        if !valid_resolution {
            return Err(ContractError::Unauthorized);
        }

        // If we're clearing a paused payment, release the paused amount
        if resolution == AS::Cleared && alert.payment_paused {
            sub_paused_amount(&env, alert.amount)?;
            alert.payment_paused = false;
        }

        let now = env.ledger().timestamp();
        alert.status = resolution.clone();
        alert.resolved_by = Some(caller.clone());
        alert.resolution_notes = notes;
        alert.resolved_at = now;
        save_alert(&env, &alert);

        env.events().publish(
            (symbol_short!("PA_RESV"),),
            (payment_ref, resolution, caller, now),
        );
        Ok(())
    }

    // ── Pause-state query ─────────────────────────────────────────────────────

    /// Check whether a payment is currently paused.
    ///
    /// Returns `true` if:
    /// - An alert exists for `payment_ref`
    /// - The alert has `payment_paused = true`
    /// - The alert is not resolved (`status == PausedPendingReview`)
    /// - The pause has not expired (or `pause_expires_at == 0`)
    ///
    /// Disbursement contracts **must** call this before executing a payment.
    pub fn is_payment_paused(env: Env, payment_ref: String) -> bool {
        bump_instance(&env);
        match get_alert(&env, &payment_ref) {
            None => false,
            Some(alert) => {
                if !alert.payment_paused {
                    return false;
                }
                if alert.status != AS::PausedPendingReview {
                    return false;
                }
                if alert.pause_expires_at > 0
                    && env.ledger().timestamp() > alert.pause_expires_at
                {
                    return false;
                }
                true
            }
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Compact public summary of an alert.
    pub fn get_alert_summary(
        env: Env,
        payment_ref: String,
    ) -> Result<AlertSummary, ContractError> {
        bump_instance(&env);
        let alert = get_alert(&env, &payment_ref).ok_or(ContractError::AlertNotFound)?;
        Ok(ASu {
            payment_ref: alert.payment_ref,
            category: alert.category,
            status: alert.status,
            description: alert.description,
            amount: alert.amount,
            payment_paused: alert.payment_paused,
            raised_at: alert.raised_at,
            resolved_at: alert.resolved_at,
        })
    }

    /// Full alert record for authorised reviewers.
    pub fn get_alert_full(
        env: Env,
        payment_ref: String,
    ) -> Result<PaymentAlert, ContractError> {
        bump_instance(&env);
        get_alert(&env, &payment_ref).ok_or(ContractError::AlertNotFound)
    }

    /// Total number of alerts ever raised.
    pub fn total_alerts(env: Env) -> u64 {
        bump_instance(&env);
        get_total_alerts(&env)
    }

    /// Total amount currently paused across all active alerts.
    pub fn total_paused_amount(env: Env) -> i128 {
        bump_instance(&env);
        get_total_paused_amount(&env)
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
        env.events().publish((symbol_short!("PA_PAUS"),), caller);
        Ok(())
    }

    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_paused(&env, false);
        env.events().publish((symbol_short!("PA_UPAU"),), caller);
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
            .publish((symbol_short!("PA_SDET"),), (caller, new_detector));
        Ok(())
    }

    pub fn set_resolver(
        env: Env,
        caller: Address,
        new_resolver: Address,
    ) -> Result<(), ContractError> {
        bump_instance(&env);
        require_admin(&env, &caller)?;
        set_resolver(&env, &new_resolver);
        env.events()
            .publish((symbol_short!("PA_SRES"),), (caller, new_resolver));
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
            .publish((symbol_short!("PA_UPGD"),), new_wasm_hash);
        Ok(())
    }
}
