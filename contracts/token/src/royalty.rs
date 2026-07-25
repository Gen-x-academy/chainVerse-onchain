use soroban_sdk::{contracttype, Address, Env};
use crate::error::TokenError;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Royalty(soroban_sdk::BytesN<32>),
}

#[contracttype]
#[derive(Clone)]
pub struct RoyaltyConfig {
    pub recipient: Address,
    pub bps: u32,
}

/// Sets the royalty configuration for a token. Only callable by the admin.
pub fn set_royalty(
    env: &Env,
    caller: Address,
    token_id: soroban_sdk::BytesN<32>,
    recipient: Address,
    bps: u32,
) -> Result<(), TokenError> {
    let admin: Address = env.storage().instance()
        .get(&DataKey::Admin)
        .ok_or(TokenError::AdminNotSet)?;
    if admin != caller {
        return Err(TokenError::Unauthorized);
    }
    caller.require_auth();
    env.storage().persistent().set(
        &DataKey::Royalty(token_id),
        &RoyaltyConfig { recipient, bps },
    );
    Ok(())
}

/// Returns the royalty configuration for a token.
pub fn get_royalty(env: &Env, token_id: soroban_sdk::BytesN<32>) -> Option<RoyaltyConfig> {
    env.storage().persistent().get(&DataKey::Royalty(token_id))
}
