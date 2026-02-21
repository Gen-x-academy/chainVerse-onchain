use soroban_sdk::{contracttype};

#[contracttype]
pub enum DataKey {
    Admin,
    BackendPubKey,
}