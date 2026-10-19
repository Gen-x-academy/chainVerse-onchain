use soroban_sdk::{contracttype, Address, BytesN};

/// Types of conflicts of interest that can be registered.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictType {
    OrganizationConflict = 0,
    ReportingConflict = 1,
    FinancialConflict = 2,
    DeclaredConflict = 3,
}

/// Result of a conflict-of-interest preflight check.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictCheckResult {
    /// True if the check passed (no conflicts found).
    pub passed: bool,
    /// Reason hash if conflicts were found (hashes detailed conflict info).
    pub reason_hash: BytesN<32>,
    /// Timestamp of the check.
    pub checked_at: u64,
}
