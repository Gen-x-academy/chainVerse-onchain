#![no_std]

//! Scholarship feature module registry and role registry.
//!
//! Scope of this pass (issue #1057, alongside ADR 0002 at
//! `docs/adr/0002-scholarships-on-chain-boundaries.md`): "register the
//! feature" by giving the scholarship contracts already built
//! (`scholarship-core`, `scholarship-programs`, `scholarship-eligibility`,
//! `scholarship-applications`) a single, on-chain-discoverable place to
//! resolve "which deployed contract handles module X," and give the
//! epic's actors (student, sponsor, reviewer, finance, administrator) a
//! real, testable role mechanism so "role-scoped routes are reachable,
//! and unauthorized actors receive consistent errors" has an actual
//! implementation ahead of full sponsor-org membership (#1059/#1060).
//!
//! This contract does not proxy or wrap calls to the other four — it is
//! purely a directory (module name → address) plus a coarse role map
//! (address → role). Callers resolve a module's address here, then call
//! it directly; `require_role`/`has_role` are meant to be checked before
//! that call, either by the caller's own off-chain orchestration or by a
//! future on-chain integration in the target contract itself.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ModuleNotFound = 4,
    /// #1057 — the generic, consistent error every role-gated caller gets
    /// back when the account lacks the required role.
    Unauthorized = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Module(Symbol),
    Role(Address, Role),
}

/// #1057 — coarse actor roles named in the epic's acceptance criteria.
/// Deliberately flat (no per-program scoping) until sponsor-org
/// membership (#1059/#1060) exists to provide that.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Student,
    Sponsor,
    Reviewer,
    Finance,
    Administrator,
}

#[contract]
pub struct ScholarshipRegistryContract;

#[contractimpl]
impl ScholarshipRegistryContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    // ── module registry ──────────────────────────────────────────────────

    /// Admin-only: register (or update) the deployed address for a named
    /// module, e.g. `Symbol::new(&env, "applications")` →
    /// `scholarship-applications`'s contract address. Unlike the versioned
    /// records in the sibling contracts, a module registration *can* be
    /// updated in place (pointing a name at a new deployment, e.g. after
    /// an upgrade) — the registry's job is "what's current," not a
    /// history of every deployment.
    pub fn register_module(
        env: Env,
        admin: Address,
        name: Symbol,
        address: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Module(name.clone());
        env.storage().persistent().set(&key, &address);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("MODREG"),), (name, address));
        Ok(())
    }

    pub fn get_module(env: Env, name: Symbol) -> Result<Address, ContractError> {
        let key = DataKey::Module(name);
        let address = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ModuleNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(address)
    }

    pub fn is_module_registered(env: Env, name: Symbol) -> bool {
        env.storage().persistent().has(&DataKey::Module(name))
    }

    // ── role registry ─────────────────────────────────────────────────────

    /// Admin-only: grant `role` to `account`.
    pub fn grant_role(
        env: Env,
        admin: Address,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Role(account.clone(), role);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events()
            .publish((soroban_sdk::symbol_short!("ROLEGRT"),), (account, role));
        Ok(())
    }

    /// Admin-only: revoke `role` from `account`. Access is lost
    /// immediately — the next `has_role`/`require_role` call reflects it,
    /// there is no delay or grace period.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Role(account.clone(), role), &false);

        env.events()
            .publish((soroban_sdk::symbol_short!("ROLERVK"),), (account, role));
        Ok(())
    }

    pub fn has_role(env: Env, account: Address, role: Role) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Role(account, role))
            .unwrap_or(false)
    }

    /// Convenience for other contracts/off-chain callers: returns a
    /// consistent `Unauthorized` error when `account` lacks `role`,
    /// rather than each caller inventing its own error for the same
    /// check. Does not call `require_auth` itself — that's the caller's
    /// responsibility for whatever action they're gating.
    pub fn require_role(env: Env, account: Address, role: Role) -> Result<(), ContractError> {
        if Self::has_role(env, account, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
