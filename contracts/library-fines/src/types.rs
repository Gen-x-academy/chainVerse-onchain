//! Data-minimization boundary for on-chain library fine records.
//!
//! ## Classification
//! - **Allowed on-chain:** content hashes (`BytesN<32>`), pseudonymous
//!   `Address` values, entry kinds, signed deltas, timestamps.
//! - **Prohibited on-chain:** patron names, emails, fine reasons in plain
//!   text, book titles, or any field that identifies a person or exposes
//!   behavioral detail. Off-chain detail is referenced only by its hash
//!   (`meta_hash` / `reason_hash`) where commitment is needed.

use soroban_sdk::{contracttype, Address, BytesN};

/// The kind of ledger entry. Every entry kind has a defined sign for `delta`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    /// A new fine assessed. `delta` is positive (patron owes more).
    Assessment,
    /// A full or partial waiver. `delta` is negative (patron owes less).
    Waiver,
    /// A confirmed Stellar payment. `delta` is negative (patron owes less).
    Payment,
    /// An admin-authorised refund of a prior confirmed payment. `delta` is
    /// positive (outstanding balance restored).
    Refund,
    /// An admin-authorised reversal of a prior confirmed payment. `delta` is
    /// positive (outstanding balance restored).
    Reversal,
}

/// An immutable ledger entry. Once appended, it is never modified or removed.
///
/// `meta_hash` commits off-chain detail (fine reason, receipt evidence, waiver
/// justification) without storing PII or raw content on-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LedgerEntry {
    /// Pseudonymous patron reference — never a name or email.
    pub patron_ref: BytesN<32>,
    /// Stable external reference unique across the entire ledger.
    pub ref_id: BytesN<32>,
    pub kind: EntryKind,
    /// Signed balance delta: positive increases what the patron owes,
    /// negative decreases it.
    pub delta: i128,
    /// Present only for Payment, Refund, and Reversal entries.
    pub asset: Option<Address>,
    pub recorded_at: u64,
    /// Hash of off-chain metadata (reason, evidence). Never the metadata itself.
    pub meta_hash: BytesN<32>,
}

/// Monotonic state machine for a single payment attempt (#983).
///
/// Allowed transitions:
/// - Pending  → Confirmed  (admin verifies Stellar tx succeeded)
/// - Pending  → Failed     (admin verifies Stellar tx expired/failed)
/// - Confirmed → Refunded  (admin issues refund)
/// - Confirmed → Reversed  (admin issues authorized correction)
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum SettlementState {
    Pending,
    Confirmed,
    Failed,
    Refunded,
    Reversed,
}

/// Tracks a single payment attempt with exact asset and amount binding (#982).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Settlement {
    /// The patron whose balance this payment is settling.
    pub patron_ref: BytesN<32>,
    /// Address that called `initiate_payment`.
    pub payer: Address,
    /// The SEP-41 asset configured for this settlement.
    pub asset: Address,
    /// Exact amount bound at initiation time.
    pub amount: i128,
    pub state: SettlementState,
    pub initiated_at: u64,
}
