#![no_std]

extern crate soroban_sdk;

use soroban_sdk::{contracterror, contracttype, Address, Symbol};

/// Payment record storing transaction details
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PaymentRecord {
    pub student: Address,
    pub course_id: Symbol,
    pub amount: i128,
    pub asset: Address,
    pub paid_at: u64,
    pub payment_id: Symbol,
}

/// Course configuration storing pricing information
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CourseConfig {
    pub course_id: Symbol,
    pub price: i128,
    pub asset: Address,
    pub active: bool,
}

/// Asset configuration for supported tokens
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetConfig {
    pub asset: Address,
    pub enabled: bool,
}

/// Storage keys for contract state
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DataKey {
    Admin,
    Token,
    FeePercent,
    RefundWindowSeconds,
    CourseConfig(Symbol),
    AssetConfig(Address),
    Enrollment(Address, Symbol),
    PaymentRecord(Address, Symbol),
    InstructorBalance(Address),
}

/// Contract errors with stable discriminants
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotAdmin = 2,
    NotInitialized = 3,
    InvalidFee = 4,
    CourseNotFound = 5,
    CourseInactive = 6,
    AlreadyEnrolled = 7,
    NotEnrolled = 8,
    PaymentFailed = 9,
    RefundWindowExpired = 10,
    InsufficientBalance = 11,
    TransferFailed = 12,
    InvalidAmount = 13,
    InvalidToken = 14,
    UnauthorizedCaller = 15,
    AssetNotEnabled = 16,
    CourseAlreadyConfigured = 17,
}

/// Event topics for indexing
pub const EVENT_PAYMENT_RECORDED: &str = "PYMT_RCD";
pub const EVENT_REFUND_ISSUED: &str = "RFND_ISS";
pub const EVENT_FEE_SET: &str = "FEE_SET";
pub const EVENT_WITHDRAWAL_PROCESSED: &str = "WTHDW";
pub const EVENT_COURSE_CONFIGURED: &str = "CRSE_CFG";
pub const EVENT_ASSET_CONFIGURED: &str = "ASSET_CFG";

/// Storage constants
pub const MIN_TTL: u32 = 4096;
pub const MAX_TTL: u32 = 100_000;
pub const MAX_FEE_BASIS_POINTS: u32 = 2000;
pub const FEE_DENOMINATOR: u32 = 10000;

/// Contract version
pub const CONTRACT_VERSION: &str = "1.0.0";
