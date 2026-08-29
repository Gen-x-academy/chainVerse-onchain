//! Versioned storage keys and TTL policy for the library-rights domain (#925).
//!
//! | Variant | Ownership | Lifetime tier | Growth |
//! |---|---|---|---|
//! | `SchemaVersion` | set once at bootstrap | instance, no TTL | bounded (1) |
//! | `Role(role)` | governance bootstrap/rotation | GOVERNANCE | bounded (4 roles) |
//! | `Work(work_id)` | policy manager | CATALOG | unbounded (1/work) |
//! | `Policy(name)` | policy manager | CATALOG | unbounded (1/policy) |
//! | `Classification(kind)` | policy manager | CATALOG | bounded (1/kind) |
//! | `ClassificationCount(kind)` | contract logic | CATALOG | bounded (1/kind) |
//! | `ClassificationHistory(kind, idx)` | contract logic | CATALOG | unbounded (1/commit) |
//! | `ProvenanceCount(work_id)` | contract logic | CATALOG | unbounded (1/work) |
//! | `Provenance(work_id, idx)` | contract logic | CATALOG | unbounded (1/record) |
//! | `Entry(id)` | policy manager | CATALOG | unbounded (1/entry) |
//! | `EntryVersion(id, v)` | contract logic | CATALOG | unbounded (1/version) |
//! | `EntryVersionCount(id)` | contract logic | CATALOG | unbounded (1/entry) |
//! | `ChildCount(parent)` | contract logic | CATALOG | unbounded (1/parent) |
//! | `ChildIndex(parent, i)` | contract logic | CATALOG | unbounded (1/child) |
//! | `License(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Loan(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Hold(work_id, holder)` | contract logic | ACTIVE | unbounded (1/work/holder) |
//! | `Balance(holder)` | contract logic | ACTIVE | unbounded (1/holder) |
//!
//! Migration: a `SchemaVersion` mismatch on read is the signal to run a
//! migration path before trusting a decoded value; no variant here is
//! removed or reshaped without bumping [`SCHEMA_VERSION`].
//!
//! `Role`, `Work`, `Classification*`, and `Provenance*` are wired up to
//! actual contract logic (#926, #927, #931, #934). `Policy`, `License`,
//! `Loan`, `Hold`, and `Balance` are reserved key shapes for the
//! application-level issues that build on this foundation.
//! `Role`, `Work`, and the `Entry*`/`Child*` catalog keys are wired up to
//! actual contract logic (#926, #927, #928, #929, #932, #933). `Policy`,
//! `License`, `Loan`, `Hold`, and `Balance` are reserved key shapes for
//! the application-level issues that build on this foundation.

use soroban_sdk::{contracttype, Address, BytesN, Symbol};

use crate::types::ClassificationKind;

/// Bumped whenever a `DataKey` variant's shape changes in a
/// backwards-incompatible way. Stored once at contract bootstrap.
pub const SCHEMA_VERSION: u32 = 2;

/// Governance/role config: rarely written, must not lapse silently.
/// ~30 days min / ~180 days max (assuming ~5s ledgers).
pub const GOVERNANCE_MIN_TTL: u32 = 518_400;
pub const GOVERNANCE_MAX_TTL: u32 = 3_110_400;

/// Catalog data (works, policies, classification commitments, provenance
/// records): long-lived, low write frequency.
/// Catalog data (works, policies, registry entries, version snapshots,
/// child indexes): long-lived, low write frequency.
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
    /// Authorized to return loans on behalf of borrowers.
    Librarian,
}

/// Versioned storage keys for the library-rights contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    SchemaVersion,
    Role(Role),
    Work(BytesN<32>),
    ContentStatus(BytesN<32>),
    Quarantine(BytesN<32>),
    Policy(Symbol),
    /// Current classification commitment for a kind (Taxonomy/Audience).
    Classification(ClassificationKind),
    /// Number of committed history entries per kind (1-based index space).
    ClassificationCount(ClassificationKind),
    /// Append-only classification history, indexed from 1.
    ClassificationHistory(ClassificationKind, u64),
    /// Number of attested provenance records per work (1-based index space).
    ProvenanceCount(BytesN<32>),
    /// Append-only provenance history per work, indexed from 1.
    Provenance(BytesN<32>, u64),
    /// Current (latest) catalog entry for a registry id (#928, #929).
    Entry(BytesN<32>),
    /// Immutable per-version snapshot of an entry (#932, #933).
    EntryVersion(BytesN<32>, u32),
    /// Number of versions recorded for an entry.
    EntryVersionCount(BytesN<32>),
    /// Number of children indexed under a parent entry (#929).
    ChildCount(BytesN<32>),
    /// 0-based child-id index under a parent entry (#929).
    ChildIndex(BytesN<32>, u32),
    License(BytesN<32>, Address),
    /// An active loan.
    ///
    /// `DataKey::Loan(work_id, borrower)`
    Loan(BytesN<32>, Address),
    /// An active hold.
    ///
    /// `DataKey::Hold(work_id, holder)`
    Hold(BytesN<32>, Address),
    /// A course reserve.
    ///
    /// `DataKey::Reserve(work_id, course_id)`
    Reserve(BytesN<32>, BytesN<32>),
    /// The address of the course registry contract.
    ///
    /// `DataKey::CourseRegistry`
    CourseRegistry,
}   Hold(BytesN<32>, Address),
    Balance(Address),
}
    MembershipAttestation(BytesN<32>),
    MembershipCurrent(Address),
    MembershipCount,
}
    /// Tracks active loan counts per patron per policy: (patron, policy_id) -> count
    PatronPolicyActiveLoans(Address, Symbol),
    /// Tracks allowlisted keepers that can trigger auto-renew evaluations
    Keeper(Address),
    /// Tracks processed request nonces to ensure idempotency: (caller, nonce) -> processed
    ProcessedNonce(Address, BytesN<32>),
    /// Tracks the total number of holds for a work (to maintain queue positions)
    WorkHoldCount(BytesN<32>),
}
