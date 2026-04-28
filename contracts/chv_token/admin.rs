use soroban_sdk::Env;
use crate::storage::get_admin;
use crate::error::TokenError;

pub fn require_admin(env: &Env) -> Result<(), TokenError> {
    let admin = get_admin(env).ok_or(TokenError::NotInitialized)?;
    admin.require_auth();
    Ok(())
}
