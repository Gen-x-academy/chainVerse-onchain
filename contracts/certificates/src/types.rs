use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    pub wallet: Address,
    pub course_id: u64,
    pub issued_at: u64,
}
