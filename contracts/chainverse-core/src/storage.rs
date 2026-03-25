use soroban_sdk::{contracttype, Address, Vec};

#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub admin: Address,
    pub protocol_fee: u32, // basis points, e.g. 100 = 1%
    pub supported_tokens: Vec<Address>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config,
}
