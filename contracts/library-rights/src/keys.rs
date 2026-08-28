use soroban_sdk::{contracttype, Address, BytesN};

pub const SCHEMA_VERSION: u32 = 2;
pub const GOVERNANCE_MIN_TTL: u32 = 518_400;
pub const GOVERNANCE_MAX_TTL: u32 = 3_110_400;
pub const CATALOG_MIN_TTL: u32 = 518_400;
pub const CATALOG_MAX_TTL: u32 = 6_220_800;
pub const ACTIVE_MIN_TTL: u32 = 17_280;
pub const ACTIVE_MAX_TTL: u32 = 518_400;

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Treasury,
    PolicyManager,
    Emergency,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    SchemaVersion,
    Role(Role),
    Work(BytesN<32>),
    /// Kept as the stable policy-scope index for compatibility.
    Policy(BytesN<32>),
    PolicyVersion(BytesN<32>, u32),
    PolicyCounter,
    License(BytesN<32>),
    Rendition(BytesN<32>),
    Seat(BytesN<32>),
    Loan(BytesN<32>),
    BorrowerLoanCount(Address, Address),
    LoanCounter,
    Hold(BytesN<32>, Address),
    Balance(Address),
}
