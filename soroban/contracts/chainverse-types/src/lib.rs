//! ChainVerse Academy – shared Soroban types and interface definitions.
//!
//! Defines the frozen public types consumed by the payment contract and any
//! downstream crates (purchase execution, accounting, indexers).  Nothing in
//! this crate contains business logic.
#![no_std]

use soroban_sdk::{contracttype, Address, Symbol};

// ─── Storage constants ────────────────────────────────────────────────────────

/// Minimum TTL (in ledgers) for persistent storage entries.
pub const MIN_TTL: u32 = 4_096;

/// Maximum TTL (in ledgers) for persistent storage entries.
pub const MAX_TTL: u32 = 100_000;

/// Maximum allowed platform fee in basis points (20 %).
pub const MAX_FEE_BASIS_POINTS: u32 = 2_000;

/// Denominator used in fee calculations (100 % == 10 000 bps).
pub const FEE_DENOMINATOR: u32 = 10_000;

/// Deployed contract version string.
pub const CONTRACT_VERSION: &str = "1.0.0";

// ─── Event topic constants ────────────────────────────────────────────────────

/// Topic emitted when a payment is recorded.
pub const EVENT_PAYMENT_RECORDED: &str = "PYMT_RCD";
/// Topic emitted when a refund is issued.
pub const EVENT_REFUND_ISSUED: &str = "RFND_ISS";
/// Topic emitted when the platform fee is updated.
pub const EVENT_FEE_SET: &str = "FEE_SET";
/// Topic emitted when an instructor withdrawal is processed.
pub const EVENT_WITHDRAWAL_PROCESSED: &str = "WTHDW";
/// Topic emitted when a course payment configuration is written.
pub const EVENT_COURSE_CONFIGURED: &str = "CRSE_CFG";
/// Topic emitted when a supported-asset configuration is written.
pub const EVENT_ASSET_CONFIGURED: &str = "ASSET_CFG";
/// Topic emitted when the treasury address is updated.
pub const EVENT_TREASURY_SET: &str = "TRES_SET";
/// Topic emitted when the administrator address is updated.
pub const EVENT_ADMIN_SET: &str = "ADMIN_SET";

// ─── Storage key enum ────────────────────────────────────────────────────────

/// Typed storage keys used across the payment contract.
///
/// | Key                          | Storage kind | Purpose                           |
/// |------------------------------|--------------|-----------------------------------|
/// | `Admin`                      | Instance     | Contract administrator address    |
/// | `Treasury`                   | Instance     | Platform treasury address         |
/// | `FeePercent`                 | Instance     | Platform fee in basis points      |
/// | `RefundWindowSeconds`        | Instance     | Refund deadline in seconds        |
/// | `AssetConfig(Address)`       | Persistent   | Per-asset supported flag          |
/// | `CourseConfig(Symbol)`       | Persistent   | Per-course payment settings       |
/// | `Enrollment(Address,Symbol)` | Persistent   | Student enrollment record         |
/// | `PaymentRecord(Address,Symbol)` | Persistent | Full payment receipt             |
/// | `InstructorBalance(Address)` | Persistent   | Instructor claimable balance      |
/// | `PaymentIdOwner(Symbol)`     | Persistent   | Reservation map for payment IDs   |
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DataKey {
    /// Contract admin address (instance storage).
    Admin,
    /// Platform treasury address (instance storage).
    Treasury,
    /// Platform fee in basis points (instance storage).
    FeePercent,
    /// Configurable refund window in seconds (instance storage).
    RefundWindowSeconds,
    /// Supported-asset configuration keyed by asset address (persistent).
    AssetConfig(Address),
    /// Course payment settings keyed by course ID symbol (persistent).
    CourseConfig(Symbol),
    /// Enrollment flag keyed by (student, course_id) (persistent).
    Enrollment(Address, Symbol),
    /// Full payment receipt keyed by (student, course_id) (persistent).
    PaymentRecord(Address, Symbol),
    /// Claimable instructor balance keyed by instructor address (persistent).
    InstructorBalance(Address),
    /// Business-level payment-ID reservation keyed by the payment ID itself
    /// (persistent). Maps a reserved 32-byte ID to the (student, course_id)
    /// pair that owns it, guaranteeing global uniqueness across purchases.
    PaymentIdOwner(Symbol),
}

// ─── Domain types ─────────────────────────────────────────────────────────────

/// Full payment receipt stored after a successful course purchase.
///
/// The split allocation required by ADR-001 is persisted alongside the gross
/// payment: `fee_amount + instructor_amount == amount` always holds.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PaymentRecord {
    /// Student who made the purchase.
    pub student: Address,
    /// Identifier of the purchased course.
    pub course_id: Symbol,
    /// Gross amount transferred by the student.
    pub amount: i128,
    /// Stellar Asset Contract address used for payment.
    pub asset: Address,
    /// Ledger timestamp at the time of payment.
    pub paid_at: u64,
    /// Business-level idempotency key (up to 32 bytes).
    pub payment_id: Symbol,
    /// Platform fee retained from the gross amount (truncated integer split).
    pub fee_amount: i128,
    /// Net proceeds credited to the instructor (`amount - fee_amount`).
    pub instructor_amount: i128,
}

/// Course payment configuration stored by the administrator.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CourseConfig {
    /// Unique course identifier.
    pub course_id: Symbol,
    /// Gross price in the smallest unit of the payment asset.
    pub price: i128,
    /// Stellar Asset Contract address accepted for this course.
    pub asset: Address,
    /// On-chain instructor address that receives net proceeds.
    pub instructor: Address,
    /// Platform fee override for this course in basis points.
    /// A value of `0` means the global fee applies.
    pub fee_bps: u32,
    /// Whether the course is currently open for purchase.
    pub active: bool,
}

/// Supported-asset configuration stored by the administrator.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AssetConfig {
    /// Stellar Asset Contract address of the supported asset.
    pub asset: Address,
    /// Whether this asset is currently accepted for payment.
    pub enabled: bool,
}
