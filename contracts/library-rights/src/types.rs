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
/// never a name, email, raw content, reading position, or staff notes.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    /// Hash of the off-chain work content. Never the content itself.
    pub work_hash: BytesN<32>,
    /// Pseudonymous on-chain address of the work's current custodian.
    pub custodian: Address,
}

/// The on-chain record of a single loan.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LoanRecord {
    /// The ID of the work being loaned.
    pub work_id: BytesN<32>,
    /// The address of the borrower.
    pub borrower: Address,
    /// The timestamp when the loan expires.
    pub expiry: u64,
}

/// The on-chain record of a single hold.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct HoldRecord {
    /// The ID of the work being held.
    pub work_id: BytesN<32>,
    /// The address of the person who placed the hold.
    pub holder: Address,
    /// The timestamp when the hold expires.
    pub expiry: u64,
}

/// The on-chain record of a course reserve.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ReserveRecord {
    /// The ID of the work being reserved.
    pub work_id: BytesN<32>,
    /// The ID of the course for which the work is reserved.
    pub course_id: BytesN<32>,
    /// The number of seats reserved for the course.
    pub seats: u32,
    /// The timestamp when the reserve expires.
    pub expiry: u64,
}