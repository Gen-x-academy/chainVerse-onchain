use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    Pending,
    Completed,
    Cancelled,
    Disputed,
}

#[contracttype]
#[derive(Clone)]
pub struct Escrow {
    pub buyer: Address,
    pub seller: Address,
    pub token: Address,
    pub amount: i128,
    pub status: EscrowStatus,
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
