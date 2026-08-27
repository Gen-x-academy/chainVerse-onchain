#![no_std]

//! # Library Rights Contract (Foundation)
//!
//! Scaffolding for the E-Library on-chain feature (issue #924). This crate is
//! intentionally decoupled from the existing `escrow` contract -- library
//! rights (works, licenses, loans, holds) are a distinct domain from
//! escrowed payments and must not import or depend on escrow types/state.
//!
//! This foundation issue only establishes the deployable shell: a versioned
//! ABI, typed errors, and a single-use `initialize`. Storage key design
//! (works/licenses/policies/loans/holds/balances/governance) lands in #925,
//! the multi-role governance model lands in #926, and the data-minimization
//! privacy boundary lands in #927 -- all on top of this crate.
//!
//! ## Impact summary
//! - **ABI:** adds a new deployable contract, `LibraryRightsContract`, with
//!   `initialize(admin)` and `version()`. No existing ABI is changed.
//! - **Storage:** instance storage only, a single `DataKey::Admin` entry.
//!   No persistent storage yet (introduced in #925).
//! - **Events:** none published by this foundation; bootstrap/role events
//!   are introduced in #926.
//! - **Privacy:** stores a single `Address` (the admin) -- no user-linkable
//!   or content-linkable data. Full classification lands in #927.
//! - **Deployment:** new, independently deployable contract; does not
//!   replace or migrate any existing contract.
//! - **Migration:** none -- this is a new contract with no prior on-chain
//!   state.

use soroban_sdk::{contract, contracterror, contractimpl, Address, Env, String};

/// Semantic version of this contract's ABI. Bump on any breaking change to
/// the exported function signatures or their externally-observable behavior.
const CONTRACT_VERSION: &str = "0.1.0";

/// Typed errors for the library-rights contract.
///
/// Kept local to this crate rather than re-using `shared::ContractError`:
/// every existing workspace contract (`course_registry`, `staking`, `token`,
/// `payout-automation`, `escrow-vault`, ...) defines its own local error
/// enum despite `docs/contracts.md` describing a shared-enum convention, so
/// this follows the convention actually in force across the codebase.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// `initialize` was called after the contract was already initialized.
    AlreadyInitialized = 1,
    /// An admin-gated call was made before `initialize` ever succeeded.
    NotInitialized = 2,
    /// The caller failed authorization as the stored admin.
    NotAdmin = 3,
}

/// Instance storage keys.
///
/// Only `Admin` exists at this foundation stage. Persistent, versioned keys
/// for library domain data (works, licenses, policies, loans, holds,
/// balances, governance) are defined in #925.
#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
}

#[contract]
pub struct LibraryRightsContract;

#[contractimpl]
impl LibraryRightsContract {
    /// One-time initialization. Sets `admin` as the contract's sole
    /// administrator. Fails if called more than once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }

        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Returns the stored admin address. Fails if the contract has not
    /// been initialized yet.
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    /// Returns this contract's ABI version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod tests;
