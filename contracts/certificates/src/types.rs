use soroban_sdk::{contracttype, Address, BytesN};

#[contracttype]
#[derive(Clone)]
pub struct Certificate {
    pub wallet: Address,
    pub course_id: u32,
    pub metadata_hash: BytesN<32>,
    pub timestamp: u64,
}