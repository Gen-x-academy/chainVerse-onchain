use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    /// Escrow created but not yet funded by the buyer.
    Created,
    /// Buyer has deposited tokens; awaiting release, dispute, or refund.
    Funded,
    /// Funds fully released to the seller.
    Completed,
    /// Funds returned to the buyer after expiry (or cancellation).
    Cancelled,
    /// Dispute opened; release blocked until resolved.
    Disputed,
}

#[contracttype]
#[derive(Clone)]
pub struct Escrow {
    pub buyer: Address,
    pub seller: Address,
    pub token: Address,
    /// Remaining locked amount (reduced by partial releases).
    pub amount: i128,
    /// Original deposit amount, preserved across partial and full releases (#862).
    pub original_amount: i128,
    pub status: EscrowStatus,
    /// Unix timestamp after which the buyer may reclaim funds (#709).
    pub expiration: u64,
}

/// A single fee-collection record persisted on every successful escrow release.
#[contracttype]
#[derive(Clone)]
pub struct FeeRecord {
    pub escrow_id: u64,
    pub token: Address,
    pub amount: i128,
    pub timestamp: u64,
}
