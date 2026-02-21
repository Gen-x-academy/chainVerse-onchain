use soroban_sdk::{Env, Address};
use crate::storage::DataKey;
use crate::errors::Error;

pub fn require_admin(env: &Env) -> Result<Address, Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::Unauthorized)?;

    admin.require_auth();
    Ok(admin)
}