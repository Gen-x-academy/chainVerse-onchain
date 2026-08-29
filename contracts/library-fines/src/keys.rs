//! Versioned storage keys and TTL policy for the library-fines domain.
//!
//! | Variant | Tier | Growth |
//! |---|---|---|
//! | `Admin` | GOVERNANCE | 1 |
//! | `Librarian` | GOVERNANCE | 1 |
//! | `SupportedAsset(asset)` | GOVERNANCE | unbounded (1/asset) |
//! | `EntryCount(patron_ref)` | LEDGER | unbounded (1/patron) |
//! | `Entry(patron_ref, seq)` | LEDGER | unbounded (1/entry) |
//! | `RefExists(ref_id)` | LEDGER | unbounded (1/ref) |
//! | `Balance(patron_ref)` | LEDGER | unbounded (1/patron) |
//! | `Settlement(settlement_id)` | ACTIVE | unbounded (1/settlement) |

use soroban_sdk::{contracttype, Address, BytesN};

/// ~30 days min / ~180 days max (assuming ~5s ledgers).
pub const GOVERNANCE_MIN_TTL: u32 = 518_400;
pub const GOVERNANCE_MAX_TTL: u32 = 3_110_400;

/// Ledger entries are long-lived and append-only.
/// ~30 days min / ~365 days max.
pub const LEDGER_MIN_TTL: u32 = 518_400;
pub const LEDGER_MAX_TTL: u32 = 6_220_800;

/// Settlement state is renewed on every transition.
/// ~1 day min / ~30 days max.
pub const ACTIVE_MIN_TTL: u32 = 17_280;
pub const ACTIVE_MAX_TTL: u32 = 518_400;

/// Hard upper bound on cursor-paginated result sets.
pub const MAX_PAGE_SIZE: u32 = 50;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Librarian,
    SupportedAsset(Address),
    /// Total entry count for `patron_ref`; doubles as the next sequence number.
    EntryCount(BytesN<32>),
    /// Individual ledger entry at `(patron_ref, seq)`.
    Entry(BytesN<32>, u32),
    /// Dedup guard: `ref_id` has already been recorded in the ledger.
    RefExists(BytesN<32>),
    /// Current outstanding balance for `patron_ref`.
    Balance(BytesN<32>),
    /// Settlement lifecycle state keyed by `settlement_id`.
    Settlement(BytesN<32>),
}
