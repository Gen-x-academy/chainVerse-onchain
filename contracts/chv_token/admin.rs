use soroban_sdk::{Env, Address};
use crate::storage::get_admin;

pub fn require_admin(env: &Env) {
    let admin = get_admin(env);
    admin.require_auth();
}
