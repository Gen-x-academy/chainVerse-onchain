//! Versioned storage keys and TTL policy for the library-rights domain (#925).
//!
//! | Variant | Ownership | Lifetime tier | Growth |
//! |---|---|---|---|
//! | `SchemaVersion` | set once at bootstrap | instance, no TTL | bounded (1) |
//! | `Role(role)` | governance bootstrap/rotation | GOVERNANCE | bounded (4 roles) |
//! | `Work(work_id)` | policy manager | CATALOG | unbounded (1/work) |
//! | `Policy(name)` | policy manager | CATALOG | unbounded (1/policy) |
//! | `License(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Loan(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Hold(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Balance(holder)` | contract logic | ACTIVE | unbounded (1/holder) |
//!
//! Migration: a `SchemaVersion` mismatch on read is the signal to run a
//! migration path before trusting a decoded value; no variant here is
//! removed or reshaped without bumping [`SCHEMA_VERSION`].
//!
//! Only `Role`, `Work`, and their supporting TTL tiers are wired up to
//! actual contract logic so far (#926, #927). `Policy`, `License`, `Loan`,
//! `Hold`, and `Balance` are reserved key shapes for the application-level
//! issues that build on this foundation.

use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// Bumped whenever a `DataKey` variant's shape changes in a
/// backwards-incompatible way. Stored once at contract bootstrap.
pub const SCHEMA_VERSION: u32 = 1;

/// Governance/role config: rarely written, must not lapse silently.
/// ~30 days min / ~180 days max (assuming ~5s ledgers).
pub const GOVERNANCE_MIN_TTL: u32 = 518_400;
pub const GOVERNANCE_MAX_TTL: u32 = 3_110_400;

/// Catalog data (works, policies): long-lived, low write frequency.
/// ~30 days min / ~365 days max.
pub const CATALOG_MIN_TTL: u32 = 518_400;
pub const CATALOG_MAX_TTL: u32 = 6_220_800;

/// Active-state data (licenses, loans, holds, balances): shorter-lived,
/// expected to be renewed on every interaction.
/// ~1 day min / ~30 days max.
///
/// Not yet consumed by any contract function -- `License`, `Loan`,
/// `Hold`, and `Balance` are reserved key shapes (see the table above)
/// wired up by later application-level issues, so these constants are
/// unused for now.
#[allow(dead_code)]
pub const ACTIVE_MIN_TTL: u32 = 17_280;
#[allow(dead_code)]
pub const ACTIVE_MAX_TTL: u32 = 518_400;

/// The four governance roles bootstrapped in #926.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Treasury,
    PolicyManager,
    Emergency,
}

/// Versioned storage keys for the library-rights contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    SchemaVersion,
    Role(Role),
    Work(BytesN<32>),
    Policy(Symbol),
    License(BytesN<32>, Address),
    Loan(BytesN<32>, Address),
    Hold(BytesN<32>, Address),
    Balance(Address),
    /// Tracks active loan counts per patron per policy: (patron, policy_id) -> count
    PatronPolicyActiveLoans(Address, Symbol),
    /// Tracks allowlisted keepers that can trigger auto-renew evaluations
    Keeper(Address),
    /// Tracks processed request nonces to ensure idempotency: (caller, nonce) -> processed
    ProcessedNonce(Address, BytesN<32>),
    /// Tracks the total number of holds for a work (to maintain queue positions)
    WorkHoldCount(BytesN<32>),
}