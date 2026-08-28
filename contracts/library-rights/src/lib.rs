#![no_std]

//! # Library Rights Contract
//!
//! On-chain foundation for the E-Library feature. Kept decoupled from the
//! `escrow` contract -- library rights (works, licenses, loans, holds)
//! are a distinct domain from escrowed payments and do not import or
//! depend on escrow types/state.
//!
//! ## Issue history
//! - **#924 (foundation):** deployable shell, versioned ABI, typed
//!   errors.
//! - **#925 (storage):** versioned [`keys::DataKey`]/[`Role`] scheme and
//!   per-domain TTL tiers.
//! - **#926 (governance):** one-time four-role bootstrap (`Admin`,
//!   `Treasury`, `PolicyManager`, `Emergency`) in [`governance`]. This
//!   replaces #924's placeholder single-admin `initialize`/`get_admin`
//!   -- the crate has never been deployed, so this is a pre-release
//!   evolution, not a migration of live state.
//! - **#927 (privacy):** [`WorkRecord`] holds only a content hash and a
//!   pseudonymous custodian address -- no names, emails, raw content,
//!   reading position, or staff notes ever land on-chain.
//!
//! ## Impact summary
//! - **ABI:** `bootstrap(admin, treasury, policy_manager, emergency)`,
//!   `get_role(role)`, `put_work(caller, work_id, work_hash, custodian)`,
//!   `get_work(work_id)`, `version()`.
//! - **Storage:** persistent, versioned keys per [`keys::DataKey`], each
//!   TTL-tiered by domain and renewed on every read/write that touches
//!   it. `SchemaVersion` lives in instance storage.
//! - **Events:** `BOOTSTRP` published once, on successful bootstrap.
//! - **Privacy:** see [`types`] -- hash + pseudonymous address only.
//! - **Deployment:** new, independently deployable contract; no existing
//!   contract is replaced.
//! - **Migration:** none yet -- no prior on-chain state exists. Future
//!   schema changes bump [`keys::SCHEMA_VERSION`].

mod errors;
mod governance;
mod keys;
mod types;

pub use errors::ContractError;
pub use keys::{DataKey, Role};
pub use types::WorkRecord;

use course_registry::CourseRegistryContractClient;
use keys::{DataKey as DK, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, String};

const CONTRACT_VERSION: &str = "1.0.0";

#[contract]
pub struct LibraryRightsContract;

#[contractimpl]
impl LibraryRightsContract {
    /// One-time bootstrap: assigns all four governance roles. Each
    /// address must independently authorize its own assignment;
    /// duplicate addresses across roles are rejected. Fails if the
    /// contract has already been bootstrapped.
    pub fn bootstrap(
        env: Env,
        admin: Address,
        treasury: Address,
        policy_manager: Address,
        emergency: Address,
        librarian: Address,
    ) -> Result<(), ContractError> {
        governance::bootstrap(&env, admin, treasury, policy_manager, emergency, librarian)
    }

    /// Returns the address currently holding `role`.
    pub fn get_role(env: Env, role: Role) -> Result<Address, ContractError> {
        governance::get_role(&env, role)
    }

    /// Registers a work's content hash and custodian. Restricted to the
    /// `PolicyManager` role.
    pub fn put_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        work_hash: BytesN<32>,
        custodian: Address,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let key = DK::Work(work_id);
        let record = WorkRecord {
            work_hash,
            custodian,
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        Ok(())
    }

    /// Returns the stored record for `work_id`, renewing its TTL.
    pub fn get_work(env: Env, work_id: BytesN<32>) -> Result<WorkRecord, ContractError> {
        let key = DK::Work(work_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::WorkNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);
        Ok(record)
    }

    /// Borrows a work, creating a new loan record.
    pub fn borrow_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        borrower: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let work_key = DK::Work(work_id.clone());
        if !env.storage().persistent().has(&work_key) {
            return Err(ContractError::WorkNotFound);
        }

        let loan_key = DK::Loan(work_id.clone(), borrower.clone());
        if env.storage().persistent().has(&loan_key) {
            // TODO: Add a specific error for this case
            return Err(ContractError::Unknown);
        }

        let expiry = env.ledger().timestamp() + (30 * 24 * 60 * 60); // 30 days
        let loan = LoanRecord {
            work_id: work_id.clone(),
            borrower: borrower.clone(),
            expiry,
        };
        env.storage().persistent().set(&loan_key, &loan);
        env.storage()
            .persistent()
            .extend_ttl(&loan_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

        env.events()
            .publish((symbol_short!("BORROW"), caller), (work_id, borrower, expiry));

        Ok(())
    }

    /// Invalidates an active loan, freeing one seat. The caller must
    /// be the borrower or an authorized librarian. Repeated calls
    /// have no effect.
    pub fn return_work(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        borrower: Address,
    ) -> Result<(), ContractError> {
        let librarian = governance::get_role(&env, Role::Librarian)?;
        if caller != borrower && caller != librarian {
            return Err(ContractError::Unauthorized);
        }

        let key = DK::Loan(work_id.clone(), borrower.clone());
        if env.storage().persistent().has(&key) {
            env.storage().persistent().remove(&key);
            env.events().publish(
                (symbol_short!("RETURN"), caller),
                (work_id, borrower),
            );
        }
        Ok(())
    }

    /// Places a hold on a work, adding the caller to the hold queue.
    pub fn place_hold(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        holder: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let work_key = DK::Work(work_id.clone());
        if !env.storage().persistent().has(&work_key) {
            return Err(ContractError::WorkNotFound);
        }

        let hold_key = DK::Hold(work_id.clone(), holder.clone());
        if env.storage().persistent().has(&hold_key) {
            // TODO: Add a specific error for this case
            return Err(ContractError::Unknown);
        }

        let expiry = env.ledger().timestamp() + (7 * 24 * 60 * 60); // 7 days
        let hold = HoldRecord {
            work_id: work_id.clone(),
            holder: holder.clone(),
            expiry,
        };
        env.storage().persistent().set(&hold_key, &hold);
        env.storage()
            .persistent()
            .extend_ttl(&hold_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

        env.events()
            .publish((symbol_short!("HOLD"), caller), (work_id, holder, expiry));

        Ok(())
    }

    /// Claims a hold, borrowing the work and removing the hold from the queue.
    pub fn claim_hold(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        holder: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let hold_key = DK::Hold(work_id.clone(), holder.clone());
        let hold: HoldRecord = env.storage().persistent().get(&hold_key).ok_or(ContractError::HoldNotFound)?;

        if env.ledger().timestamp() > hold.expiry {
            return Err(ContractError::HoldExpired);
        }

        if caller != hold.holder {
            return Err(ContractError::Unauthorized);
        }

        Self::borrow_work(env.clone(), caller.clone(), work_id.clone(), holder.clone())?;

        env.storage().persistent().remove(&hold_key);

        env.events()
            .publish((symbol_short!("CLAIM"), caller), (work_id, holder));

        Ok(())
    }

    /// Creates a new course reserve for a work.
    pub fn create_reserve(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        course_id: BytesN<32>,
        seats: u32,
        expiry: u64,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;

        let work_key = DK::Work(work_id.clone());
        if !env.storage().persistent().has(&work_key) {
            return Err(ContractError::WorkNotFound);
        }

        let reserve_key = DK::Reserve(work_id.clone(), course_id.clone());
        if env.storage().persistent().has(&reserve_key) {
            return Err(ContractError::ReserveExists);
        }

        let reserve = ReserveRecord {
            work_id: work_id.clone(),
            course_id: course_id.clone(),
            seats,
            expiry,
        };
        env.storage().persistent().set(&reserve_key, &reserve);
        env.storage()
            .persistent()
            .extend_ttl(&reserve_key, CATALOG_MIN_TTL, CATALOG_MAX_TTL);

        env.events().publish(
            (symbol_short!("RESERVE"), caller),
            (work_id, course_id, seats, expiry),
        );

        Ok(())
    }

    /// Sets the address of the course registry contract.
    pub fn set_course_registry(
        env: Env,
        caller: Address,
        course_registry_id: Address,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::Admin, &caller)?;
        env.storage()
            .instance()
            .set(&DK::CourseRegistry, &course_registry_id);
        Ok(())
    }

    /// Borrows a work from a course reserve.
    pub fn borrow_from_reserve(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        course_id: BytesN<32>,
        borrower: Address,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let course_registry_id: Address = env
            .storage()
            .instance()
            .get(&DK::CourseRegistry)
            .ok_or(ContractError::CourseRegistryNotSet)?;
        let course_registry = CourseRegistryContractClient::new(&env, &course_registry_id);

        if !course_registry.is_enrolled(&course_id, &borrower) {
            return Err(ContractError::NotEnrolled);
        }

        let reserve_key = DK::Reserve(work_id.clone(), course_id.clone());
        let mut reserve: ReserveRecord = env
            .storage()
            .persistent()
            .get(&reserve_key)
            .ok_or(ContractError::ReserveNotFound)?;

        if reserve.seats == 0 {
            return Err(ContractError::NoSeatsAvailable);
        }

        Self::borrow_work(env.clone(), caller.clone(), work_id.clone(), borrower.clone())?;

        reserve.seats -= 1;
        env.storage().persistent().set(&reserve_key, &reserve);

        env.events().publish(
            (symbol_short!("BRW_RES"), caller),
            (work_id, course_id, borrower),
        );

        Ok(())
    }

    /// Returns a work to a course reserve.
    pub fn return_to_reserve(
        env: Env,
        caller: Address,
        work_id: BytesN<32>,
        course_id: BytesN<32>,
        borrower: Address,
    ) -> Result<(), ContractError> {
        Self::return_work(env.clone(), caller.clone(), work_id.clone(), borrower.clone())?;

        let reserve_key = DK::Reserve(work_id.clone(), course_id.clone());
        let mut reserve: ReserveRecord = env
            .storage()
            .persistent()
            .get(&reserve_key)
            .ok_or(ContractError::ReserveNotFound)?;

        reserve.seats += 1;
        env.storage().persistent().set(&reserve_key, &reserve);

        env.events().publish(
            (symbol_short!("RTN_RES"), caller),
            (work_id, course_id, borrower),
        );

        Ok(())
    }

    /// Returns this contract's ABI version string.
    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod tests;