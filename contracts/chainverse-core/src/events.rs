use soroban_sdk::{contracttype, Address};

#[contracttype]
pub struct CoursePurchasedEvent {
    pub wallet: Address,
    pub course_id: u32,
    pub asset_used: Address,
    pub timestamp: u64,
}