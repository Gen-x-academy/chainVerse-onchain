use soroban_sdk::{Address, Env};
use crate::errors::Error;
use crate::storage::get_admin;

pub fn require_admin(env: &Env) -> Result<Address, Error> {
    let admin = get_admin(env).ok_or(Error::Unauthorized)?;
    admin.require_auth();
    Ok(admin)
}
