use soroban_sdk::{Address};

#[derive(Clone)]
pub struct Certificate {
    pub wallet: Address,
    pub course_id: u64,
    pub issued_at: u64,
}