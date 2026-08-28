//! One-time, four-role governance bootstrap (#926).
//!
//! Supersedes #924's placeholder single-admin `initialize`/`get_admin` --
//! the crate has never been deployed, so this is a pre-release evolution
//! of the foundation, not a migration of live state.

use soroban_sdk::{symbol_short, Address, Env};

use crate::errors::ContractError;
use crate::keys::{DataKey, Role, GOVERNANCE_MAX_TTL, GOVERNANCE_MIN_TTL, SCHEMA_VERSION};

const ROLES: [Role; 5] = [
    Role::Admin,
    Role::Treasury,
    Role::PolicyManager,
    Role::Emergency,
    Role::Librarian,
];

/// Assigns all five governance roles in a single, single-use call. Every
/// role address must independently authorize its own assignment; any two
/// roles sharing the same address are rejected.
pub fn bootstrap(
    env: &Env,
    admin: Address,
    treasury: Address,
    policy_manager: Address,
    emergency: Address,
    librarian: Address,
) -> Result<(), ContractError> {
    if env.storage().persistent().has(&DataKey::Role(Role::Admin)) {
        return Err(ContractError::AlreadyInitialized);
    }

    let addrs = [
        &admin,
        &treasury,
        &policy_manager,
        &emergency,
        &librarian,
    ];

    // Reject any two roles sharing the same address *before* requiring
    // any signatures -- this also avoids calling `require_auth()` twice
    // for the same underlying address within one invocation, which the
    // test environment's mocked-auth bookkeeping does not tolerate.
    for i in 0..addrs.len() {
        for j in (i + 1)..addrs.len() {
            if addrs[i] == addrs[j] {
                return Err(ContractError::DuplicateRole);
            }
        }
    }

    // Every role must independently authorize its own assignment -- this
    // also rules out a caller silently assigning a role to an address
    // that never consented.
    for a in addrs.iter() {
        a.require_auth();
    }

    for (role, addr) in ROLES.iter().zip(addrs.iter()) {
        let key = DataKey::Role(*role);
        env.storage().persistent().set(&key, *addr);
        env.storage()
            .persistent()
            .extend_ttl(&key, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);
    }

    env.storage()
        .instance()
        .set(&DataKey::SchemaVersion, &SCHEMA_VERSION);

    env.events().publish(
        (symbol_short!("BOOTSTRP"),),
        (admin, treasury, policy_manager, emergency, librarian),
    );

    Ok(())
}

/// Returns the address currently holding `role`, renewing its TTL.
pub fn get_role(env: &Env, role: Role) -> Result<Address, ContractError> {
    let key = DataKey::Role(role);
    let addr = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(ContractError::NotInitialized)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, GOVERNANCE_MIN_TTL, GOVERNANCE_MAX_TTL);
    Ok(addr)
}

/// Requires that `caller` both holds `role` and has authorized this
/// invocation. Used by role-gated operations elsewhere in the contract.
pub fn require_role(env: &Env, role: Role, caller: &Address) -> Result<(), ContractError> {
    let holder = get_role(env, role)?;
    if holder != *caller {
        return Err(ContractError::NotAdmin);
    }
    caller.require_auth();
    Ok(())
}