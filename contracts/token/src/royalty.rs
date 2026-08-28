use soroban_sdk::{contracttype, Address, Env};
use crate::{DataKey, TokenError};

#[contracttype]
#[derive(Clone)]
pub struct RoyaltyConfig {
    pub recipient: Address,
    pub bps: u32,
}

pub fn set_royalty(
    env: &Env,
    admin: Address,
    recipient: Address,
    bps: u32,
) -> Result<(), TokenError> {
    let configured_admin: Address = env.storage().instance()
        .get(&DataKey::Admin)
        .ok_or(TokenError::NotInitialized)?;
    if configured_admin != admin {
        return Err(TokenError::Unauthorized);
    }
    admin.require_auth();
    if bps > 10_000 {
        return Err(TokenError::RoyaltyBpsTooHigh);
    }
    env.storage().instance().set(&DataKey::Royalty, &RoyaltyConfig { recipient, bps });
    Ok(())
}

pub fn get_royalty(env: &Env) -> Option<RoyaltyConfig> {
    env.storage().instance().get(&DataKey::Royalty)
}
