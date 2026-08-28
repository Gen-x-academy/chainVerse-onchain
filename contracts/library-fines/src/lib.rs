#![no_std]

//! # Library Fines Contract
//!
//! Implements issues #980–#983 of the E-Library On-chain suite.
//!
//! ## Issue summary
//! - **#980 (ledger):** append-only charge ledger keyed by stable `ref_id`
//!   references; cursor-bounded queries; balance derivable from entry deltas.
//! - **#981 (waivers):** bounded full/partial waiver actions with committed
//!   `reason_hash`; librarian or admin role required; cannot exceed outstanding
//!   balance; assessment history preserved.
//! - **#982 (assets):** SEP-41 asset settlement path; only configured assets
//!   accepted; exact asset and amount bound at initiation; idempotent
//!   (duplicate `settlement_id` rejected); balance updated atomically on
//!   confirm; receipt emits the ledger `ref_id`.
//! - **#983 (reconciliation):** monotonic Pending → Confirmed/Failed and
//!   Confirmed → Refunded/Reversed state machine; only admin can drive
//!   transitions; balance credited exactly once per settlement.
//!
//! ## ABI
//! `initialize`, `add_supported_asset`, `assess`, `waive`,
//! `initiate_payment`, `confirm_payment`, `fail_payment`,
//! `refund_payment`, `reverse_payment`,
//! `get_balance`, `get_entry`, `get_entries`, `get_settlement`, `version`.
//!
//! ## Storage
//! See [`keys::DataKey`] for the full key table and TTL tiers.
//!
//! ## Events
//! `ASSESSED`, `WAIVED`, `PAY_INIT`, `PAY_CONF`, `PAY_FAIL`,
//! `PAY_RFND`, `PAY_REV`.
//!
//! ## Privacy
//! `patron_ref` is a pseudonymous hash — no names, emails, or reading
//! history land on-chain. Off-chain detail is committed via `meta_hash`
//! or `reason_hash`.
//!
//! ## Deployment
//! New, independently deployable contract; no existing contract is replaced.
//!
//! ## Migration
//! No prior on-chain state exists. Future schema changes bump the version
//! string and extend [`keys::DataKey`].

mod errors;
mod events;
mod keys;
mod types;

#[cfg(test)]
mod tests;

pub use errors::ContractError;
pub use keys::DataKey;
pub use types::{EntryKind, LedgerEntry, Settlement, SettlementState};

use keys::{
    DataKey as DK, ACTIVE_MAX_TTL, ACTIVE_MIN_TTL, GOVERNANCE_MAX_TTL, GOVERNANCE_MIN_TTL,
    LEDGER_MAX_TTL, LEDGER_MIN_TTL, MAX_PAGE_SIZE,
};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

const CONTRACT_VERSION: &str = "0.1.0";

#[contract]
pub struct LibraryFinesContract;

#[contractimpl]
impl LibraryFinesContract {
    /// One-time setup. Only `admin` must authorize.
    pub fn initialize(
        env: Env,
        admin: Address,
        librarian: Address,
    ) -> Result<(), ContractError> {
        if env.storage().persistent().has(&DK::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().persistent().set(&DK::Admin, &admin);
        env.storage()
            .persistent()
            .extend_ttl(&DK::Admin, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);

        env.storage().persistent().set(&DK::Librarian, &librarian);
        env.storage()
            .persistent()
            .extend_ttl(&DK::Librarian, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);

        Ok(())
    }

    // ------------------------------------------------------------------ helpers

    fn read_admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    fn read_librarian(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Librarian)
            .ok_or(ContractError::NotInitialized)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = Self::read_admin(env)?;
        if *caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    /// Accepts either the `Librarian` or the `Admin` role.
    fn require_librarian_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = Self::read_admin(env)?;
        let librarian = Self::read_librarian(env)?;
        if *caller != admin && *caller != librarian {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn read_balance(env: &Env, patron_ref: &BytesN<32>) -> i128 {
        env.storage()
            .persistent()
            .get(&DK::Balance(patron_ref.clone()))
            .unwrap_or(0i128)
    }

    fn write_balance(env: &Env, patron_ref: &BytesN<32>, balance: i128) {
        let key = DK::Balance(patron_ref.clone());
        env.storage().persistent().set(&key, &balance);
        env.storage()
            .persistent()
            .extend_ttl(&key, LEDGER_MIN_TTL, LEDGER_MAX_TTL);
    }

    fn entry_count(env: &Env, patron_ref: &BytesN<32>) -> u32 {
        env.storage()
            .persistent()
            .get(&DK::EntryCount(patron_ref.clone()))
            .unwrap_or(0u32)
    }

    /// Appends `entry` to the ledger and marks its `ref_id` as used.
    fn append_entry(env: &Env, entry: &LedgerEntry) {
        let count_key = DK::EntryCount(entry.patron_ref.clone());
        let seq: u32 = env
            .storage()
            .persistent()
            .get(&count_key)
            .unwrap_or(0u32);

        let entry_key = DK::Entry(entry.patron_ref.clone(), seq);
        env.storage().persistent().set(&entry_key, entry);
        env.storage()
            .persistent()
            .extend_ttl(&entry_key, LEDGER_MIN_TTL, LEDGER_MAX_TTL);

        env.storage().persistent().set(&count_key, &(seq + 1));
        env.storage()
            .persistent()
            .extend_ttl(&count_key, LEDGER_MIN_TTL, LEDGER_MAX_TTL);

        let ref_key = DK::RefExists(entry.ref_id.clone());
        env.storage().persistent().set(&ref_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&ref_key, LEDGER_MIN_TTL, LEDGER_MAX_TTL);
    }

    // ----------------------------------------------------------------- actions

    /// Registers a SEP-41 asset as accepted for fine settlements. Admin only.
    pub fn add_supported_asset(
        env: Env,
        caller: Address,
        asset: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        let key = DK::SupportedAsset(asset);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);
        Ok(())
    }

    /// (#980) Appends an Assessment entry to the ledger.
    ///
    /// Increases the patron's outstanding balance by `amount`. Duplicate
    /// `ref_id` values are rejected globally across the entire ledger.
    pub fn assess(
        env: Env,
        caller: Address,
        patron_ref: BytesN<32>,
        ref_id: BytesN<32>,
        amount: i128,
        meta_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_librarian_or_admin(&env, &caller)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if env.storage().persistent().has(&DK::RefExists(ref_id.clone())) {
            return Err(ContractError::DuplicateReference);
        }

        let balance = Self::read_balance(&env, &patron_ref);
        let new_balance = balance + amount;

        Self::append_entry(
            &env,
            &LedgerEntry {
                patron_ref: patron_ref.clone(),
                ref_id: ref_id.clone(),
                kind: EntryKind::Assessment,
                delta: amount,
                asset: None,
                recorded_at: env.ledger().timestamp(),
                meta_hash,
            },
        );
        Self::write_balance(&env, &patron_ref, new_balance);
        events::assessment_recorded(&env, patron_ref, ref_id, amount, new_balance);
        Ok(())
    }

    /// (#981) Appends a Waiver entry, reducing the patron's outstanding balance.
    ///
    /// `amount` must be ≤ the current outstanding balance (cannot create
    /// negative debt). The original Assessment entries are preserved —
    /// only a new Waiver entry is appended. `reason_hash` commits the
    /// off-chain justification without storing PII on-chain.
    pub fn waive(
        env: Env,
        caller: Address,
        patron_ref: BytesN<32>,
        ref_id: BytesN<32>,
        amount: i128,
        reason_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_librarian_or_admin(&env, &caller)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if env.storage().persistent().has(&DK::RefExists(ref_id.clone())) {
            return Err(ContractError::DuplicateReference);
        }

        let balance = Self::read_balance(&env, &patron_ref);
        if amount > balance {
            return Err(ContractError::WaiverExceedsBalance);
        }
        let new_balance = balance - amount;

        Self::append_entry(
            &env,
            &LedgerEntry {
                patron_ref: patron_ref.clone(),
                ref_id: ref_id.clone(),
                kind: EntryKind::Waiver,
                delta: -amount,
                asset: None,
                recorded_at: env.ledger().timestamp(),
                meta_hash: reason_hash.clone(),
            },
        );
        Self::write_balance(&env, &patron_ref, new_balance);
        events::waiver_granted(&env, caller, patron_ref, ref_id, amount, reason_hash, new_balance);
        Ok(())
    }

    /// (#982, #983) Registers a payment attempt in Pending state.
    ///
    /// Binds the exact SEP-41 `asset` and `amount` at initiation time.
    /// Unsupported assets are rejected. Duplicate `settlement_id` values
    /// are rejected, ensuring idempotency: the same payment cannot be
    /// applied twice even if the caller retries.
    pub fn initiate_payment(
        env: Env,
        caller: Address,
        patron_ref: BytesN<32>,
        settlement_id: BytesN<32>,
        asset: Address,
        amount: i128,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        Self::read_admin(&env)?;

        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if !env.storage().persistent().has(&DK::SupportedAsset(asset.clone())) {
            return Err(ContractError::UnsupportedAsset);
        }
        let sett_key = DK::Settlement(settlement_id.clone());
        if env.storage().persistent().has(&sett_key) {
            return Err(ContractError::DuplicateSettlement);
        }

        let settlement = Settlement {
            patron_ref: patron_ref.clone(),
            payer: caller,
            asset: asset.clone(),
            amount,
            state: SettlementState::Pending,
            initiated_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&sett_key, &settlement);
        env.storage()
            .persistent()
            .extend_ttl(&sett_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);

        events::payment_initiated(&env, patron_ref, settlement_id, asset, amount);
        Ok(())
    }

    /// (#982, #983) Confirms a Pending settlement and credits the balance.
    ///
    /// Monotonic: only Pending → Confirmed. `amount` must not exceed the
    /// patron's outstanding balance so the balance cannot go negative.
    /// The emitted receipt identifies the new ledger entry via `ref_id`.
    /// Calling confirm a second time on the same settlement is rejected,
    /// preventing double-credit.
    pub fn confirm_payment(
        env: Env,
        caller: Address,
        settlement_id: BytesN<32>,
        ref_id: BytesN<32>,
        meta_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let sett_key = DK::Settlement(settlement_id.clone());
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&sett_key)
            .ok_or(ContractError::SettlementNotFound)?;

        if settlement.state != SettlementState::Pending {
            return Err(ContractError::InvalidStateTransition);
        }
        if env.storage().persistent().has(&DK::RefExists(ref_id.clone())) {
            return Err(ContractError::DuplicateReference);
        }

        let balance = Self::read_balance(&env, &settlement.patron_ref);
        if settlement.amount > balance {
            return Err(ContractError::PaymentExceedsBalance);
        }
        let new_balance = balance - settlement.amount;

        Self::append_entry(
            &env,
            &LedgerEntry {
                patron_ref: settlement.patron_ref.clone(),
                ref_id: ref_id.clone(),
                kind: EntryKind::Payment,
                delta: -settlement.amount,
                asset: Some(settlement.asset.clone()),
                recorded_at: env.ledger().timestamp(),
                meta_hash,
            },
        );
        Self::write_balance(&env, &settlement.patron_ref, new_balance);

        settlement.state = SettlementState::Confirmed;
        env.storage().persistent().set(&sett_key, &settlement);
        env.storage()
            .persistent()
            .extend_ttl(&sett_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);

        events::payment_confirmed(&env, settlement.patron_ref, settlement_id, ref_id, new_balance);
        Ok(())
    }

    /// (#983) Marks a Pending settlement as Failed. Balance is not changed.
    ///
    /// Monotonic: only Pending → Failed.
    pub fn fail_payment(
        env: Env,
        caller: Address,
        settlement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let sett_key = DK::Settlement(settlement_id.clone());
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&sett_key)
            .ok_or(ContractError::SettlementNotFound)?;

        if settlement.state != SettlementState::Pending {
            return Err(ContractError::InvalidStateTransition);
        }

        settlement.state = SettlementState::Failed;
        env.storage().persistent().set(&sett_key, &settlement);
        env.storage()
            .persistent()
            .extend_ttl(&sett_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);

        events::payment_failed(&env, settlement.patron_ref, settlement_id);
        Ok(())
    }

    /// (#983) Refunds a Confirmed settlement, restoring the patron's balance.
    ///
    /// Monotonic: only Confirmed → Refunded.
    pub fn refund_payment(
        env: Env,
        caller: Address,
        settlement_id: BytesN<32>,
        ref_id: BytesN<32>,
        meta_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let sett_key = DK::Settlement(settlement_id.clone());
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&sett_key)
            .ok_or(ContractError::SettlementNotFound)?;

        if settlement.state != SettlementState::Confirmed {
            return Err(ContractError::InvalidStateTransition);
        }
        if env.storage().persistent().has(&DK::RefExists(ref_id.clone())) {
            return Err(ContractError::DuplicateReference);
        }

        let balance = Self::read_balance(&env, &settlement.patron_ref);
        let new_balance = balance + settlement.amount;

        Self::append_entry(
            &env,
            &LedgerEntry {
                patron_ref: settlement.patron_ref.clone(),
                ref_id: ref_id.clone(),
                kind: EntryKind::Refund,
                delta: settlement.amount,
                asset: Some(settlement.asset.clone()),
                recorded_at: env.ledger().timestamp(),
                meta_hash,
            },
        );
        Self::write_balance(&env, &settlement.patron_ref, new_balance);

        settlement.state = SettlementState::Refunded;
        env.storage().persistent().set(&sett_key, &settlement);
        env.storage()
            .persistent()
            .extend_ttl(&sett_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);

        events::payment_refunded(&env, settlement.patron_ref, settlement_id, ref_id, new_balance);
        Ok(())
    }

    /// (#983) Reverses a Confirmed settlement via authorized correction,
    /// restoring the patron's balance.
    ///
    /// Monotonic: only Confirmed → Reversed. Semantically distinct from
    /// Refund: a Reversal is an admin-initiated correction (e.g., payment
    /// credited in error), not a patron-requested refund.
    pub fn reverse_payment(
        env: Env,
        caller: Address,
        settlement_id: BytesN<32>,
        ref_id: BytesN<32>,
        meta_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let sett_key = DK::Settlement(settlement_id.clone());
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&sett_key)
            .ok_or(ContractError::SettlementNotFound)?;

        if settlement.state != SettlementState::Confirmed {
            return Err(ContractError::InvalidStateTransition);
        }
        if env.storage().persistent().has(&DK::RefExists(ref_id.clone())) {
            return Err(ContractError::DuplicateReference);
        }

        let balance = Self::read_balance(&env, &settlement.patron_ref);
        let new_balance = balance + settlement.amount;

        Self::append_entry(
            &env,
            &LedgerEntry {
                patron_ref: settlement.patron_ref.clone(),
                ref_id: ref_id.clone(),
                kind: EntryKind::Reversal,
                delta: settlement.amount,
                asset: Some(settlement.asset.clone()),
                recorded_at: env.ledger().timestamp(),
                meta_hash,
            },
        );
        Self::write_balance(&env, &settlement.patron_ref, new_balance);

        settlement.state = SettlementState::Reversed;
        env.storage().persistent().set(&sett_key, &settlement);
        env.storage()
            .persistent()
            .extend_ttl(&sett_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);

        events::payment_reversed(&env, settlement.patron_ref, settlement_id, ref_id, new_balance);
        Ok(())
    }

    // ------------------------------------------------------------------ queries

    /// Returns the current outstanding balance for `patron_ref`.
    /// Equals the sum of all `delta` values in the patron's ledger entries.
    pub fn get_balance(env: Env, patron_ref: BytesN<32>) -> i128 {
        Self::read_balance(&env, &patron_ref)
    }

    /// Returns a single ledger entry by `(patron_ref, seq)`.
    pub fn get_entry(
        env: Env,
        patron_ref: BytesN<32>,
        seq: u32,
    ) -> Result<LedgerEntry, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Entry(patron_ref, seq))
            .ok_or(ContractError::EntryNotFound)
    }

    /// Cursor-bounded query returning up to `limit` entries starting at `cursor`.
    ///
    /// `limit` is silently capped at `MAX_PAGE_SIZE` (50). Returns an empty
    /// `Vec` when `cursor` is at or beyond the total entry count.
    pub fn get_entries(
        env: Env,
        patron_ref: BytesN<32>,
        cursor: u32,
        limit: u32,
    ) -> Vec<LedgerEntry> {
        let count = Self::entry_count(&env, &patron_ref);
        let page_limit = if limit > MAX_PAGE_SIZE { MAX_PAGE_SIZE } else { limit };
        let end = cursor.saturating_add(page_limit);

        let mut result: Vec<LedgerEntry> = Vec::new(&env);
        let mut seq = cursor;
        while seq < count && seq < end {
            let key = DK::Entry(patron_ref.clone(), seq);
            let maybe: Option<LedgerEntry> = env.storage().persistent().get(&key);
            if let Some(entry) = maybe {
                result.push_back(entry);
            }
            seq += 1;
        }
        result
    }

    /// Returns the settlement record for `settlement_id`.
    pub fn get_settlement(
        env: Env,
        settlement_id: BytesN<32>,
    ) -> Result<Settlement, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Settlement(settlement_id))
            .ok_or(ContractError::SettlementNotFound)
    }

    /// Returns this contract's ABI version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}
