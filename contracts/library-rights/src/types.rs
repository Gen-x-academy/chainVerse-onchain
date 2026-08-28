//! Data-minimization boundary for on-chain library records (#927).
//!
//! ## Classification
//! - **Public/allowed on-chain:** content hashes (`BytesN<32>`),
//!   pseudonymous `Address` values, coarse status enums/timestamps.
//! - **Prohibited on-chain:** names, emails, raw book/content bytes,
//!   reading position/progress, or staff notes -- or any other field
//!   that identifies a person or exposes content/behavioral detail.
//!   That data belongs in the off-chain backend (chainVerse-backend
//!   #979) and is referenced here only by its hash where verification
//!   is needed.
//!
//! [`WorkRecord`] is intentionally minimal: it holds nothing beyond a
//! content hash and a pseudonymous custodian address. Any future field
//! added to a type in this module must be checked against the
//! prohibited list above before being merged.

use soroban_sdk::{contracttype, Address, BytesN};

/// A registered work's on-chain record.
///
/// Contains only a content hash and a pseudonymous custodian address --
/// never a name, email, raw content, reading position, or staff note.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    /// Hash of the off-chain work content. Never the content itself.
    pub work_hash: BytesN<32>,
    /// Pseudonymous on-chain address of the work's current custodian.
    pub custodian: Address,
}
