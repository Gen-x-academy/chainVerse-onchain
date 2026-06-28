use soroban_sdk::{contracttype, Address, BytesN};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    pub recipient: Address,
    pub course_id: BytesN<32>,
    pub token_id: u64,
    pub soul_bound: bool,
}
