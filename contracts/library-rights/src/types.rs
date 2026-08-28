use soroban_sdk::{contracttype, Address, BytesN, Symbol};

/// A registered work's on-chain record. Only a content hash and pseudonymous
/// custodian are stored; raw content and personal metadata remain off-chain.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct WorkRecord {
    pub work_hash: BytesN<32>,
    pub custodian: Address,
}

/// Scope used to resolve a borrowing policy. `collection` is optional so one
/// policy can apply to every work in a format or to one collection only.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyScope {
    pub institution: Address,
    pub role: Symbol,
    pub format: Symbol,
    pub collection: Option<BytesN<32>>,
}

/// A complete borrowing-policy snapshot. Durations are seconds and fine is a
/// non-negative amount in the institution's configured accounting unit.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct BorrowingPolicy {
    pub scope: PolicyScope,
    pub loan_duration_secs: u64,
    pub max_concurrent_loans: u32,
    pub renewal_limit: u32,
    pub hold_duration_secs: u64,
    pub fine_per_day: i128,
    pub version: u32,
    pub active: bool,
}

/// An append-only policy version. The policy body is never overwritten; a new
/// version is written when the same scope changes.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyVersion {
    pub policy_id: BytesN<32>,
    pub version: u32,
    pub policy: BorrowingPolicy,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LicenseRecord {
    pub work_id: BytesN<32>,
    pub institution: Address,
    pub expires_at: u64,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RenditionRecord {
    pub work_id: BytesN<32>,
    pub format: Symbol,
    pub active: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SeatRecord {
    pub institution: Address,
    pub available: bool,
}

/// A loan captures the exact policy version used at checkout. Later policy
/// changes therefore cannot silently alter this obligation.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LoanRecord {
    pub loan_id: BytesN<32>,
    pub borrower: Address,
    pub institution: Address,
    pub work_id: BytesN<32>,
    pub license_id: BytesN<32>,
    pub rendition_id: BytesN<32>,
    pub seat_id: BytesN<32>,
    pub policy_id: BytesN<32>,
    pub policy_version: u32,
    pub checked_out_at: u64,
    pub due_at: u64,
    pub active: bool,
}
