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

mod enrollment;
mod errors;
mod governance;
mod keys;
mod types;

pub use enrollment::{CourseRegistryClient, CourseRegistryInterface};
pub use errors::ContractError;
pub use keys::{DataKey, Role};
pub use types::{
    BorrowingPolicy, LicenseRecord, LoanRecord, PolicyScope, PolicyVersion, RenditionRecord,
    SeatRecord, WorkRecord,
};

use keys::{DataKey as DK, ACTIVE_MAX_TTL, ACTIVE_MIN_TTL, CATALOG_MAX_TTL, CATALOG_MIN_TTL};
use soroban_sdk::{
    contract, contractimpl, symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env, String, Symbol,
};

const CONTRACT_VERSION: &str = "0.5.0";

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
    ) -> Result<(), ContractError> {
        governance::bootstrap(&env, admin, treasury, policy_manager, emergency)
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

    /// Returns this contract's ABI version string.
    /// Append a new policy version for a scope. Existing versions remain
    /// immutable, while the latest version is indexed for resolution.
    pub fn append_policy(
        env: Env,
        caller: Address,
        policy_id: BytesN<32>,
        scope: PolicyScope,
        loan_duration_secs: u64,
        max_concurrent_loans: u32,
        renewal_limit: u32,
        hold_duration_secs: u64,
        fine_per_day: i128,
    ) -> Result<u32, ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        if scope.institution != caller && caller != governance::get_role(&env, Role::PolicyManager)?
        {
            return Err(ContractError::NotAdmin);
        }
        if loan_duration_secs == 0
            || max_concurrent_loans == 0
            || fine_per_day < 0
            || hold_duration_secs == 0
        {
            return Err(ContractError::InvalidPolicy);
        }
        let version = env
            .storage()
            .persistent()
            .get::<DK, u32>(&DK::Policy(policy_id.clone()))
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::InvalidPolicy)?;
        let policy = BorrowingPolicy {
            scope,
            loan_duration_secs,
            max_concurrent_loans,
            renewal_limit,
            hold_duration_secs,
            fine_per_day,
            version,
            active: true,
        };
        let record = PolicyVersion {
            policy_id: policy_id.clone(),
            version,
            policy,
            created_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DK::PolicyVersion(policy_id.clone(), version), &record);
        env.storage()
            .persistent()
            .set(&DK::Policy(policy_id), &version);
        env.storage().persistent().extend_ttl(
            &DK::PolicyVersion(record.policy_id.clone(), version),
            CATALOG_MIN_TTL,
            CATALOG_MAX_TTL,
        );
        Ok(version)
    }

    pub fn get_policy_version(
        env: Env,
        policy_id: BytesN<32>,
        version: u32,
    ) -> Result<PolicyVersion, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::PolicyVersion(policy_id, version))
            .ok_or(ContractError::PolicyVersionNotFound)
    }

    pub fn latest_policy(env: Env, policy_id: BytesN<32>) -> Result<PolicyVersion, ContractError> {
        let version = env
            .storage()
            .persistent()
            .get::<DK, u32>(&DK::Policy(policy_id.clone()))
            .ok_or(ContractError::PolicyNotFound)?;
        Self::get_policy_version(env, policy_id, version)
    }

    pub fn register_license(
        env: Env,
        caller: Address,
        license_id: BytesN<32>,
        work_id: BytesN<32>,
        institution: Address,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        if expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidTimestamp);
        }
        let key = DK::License(license_id);
        env.storage().persistent().set(
            &key,
            &LicenseRecord {
                work_id,
                institution,
                expires_at,
                active: true,
            },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);
        Ok(())
    }

    pub fn register_rendition(
        env: Env,
        caller: Address,
        rendition_id: BytesN<32>,
        work_id: BytesN<32>,
        format: Symbol,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let key = DK::Rendition(rendition_id);
        env.storage().persistent().set(
            &key,
            &RenditionRecord {
                work_id,
                format,
                active: true,
            },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);
        Ok(())
    }

    pub fn register_seat(
        env: Env,
        caller: Address,
        seat_id: BytesN<32>,
        institution: Address,
    ) -> Result<(), ContractError> {
        governance::require_role(&env, Role::PolicyManager, &caller)?;
        let key = DK::Seat(seat_id);
        env.storage().persistent().set(
            &key,
            &SeatRecord {
                institution,
                available: true,
            },
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);
        Ok(())
    }

    /// Validate every checkout condition before mutating the seat, borrower
    /// count, and loan. Soroban rolls back the complete invocation on error.
    pub fn checkout(
        env: Env,
        borrower: Address,
        institution: Address,
        course_registry: Address,
        course_id: Symbol,
        borrower_role: Symbol,
        collection: Option<BytesN<32>>,
        policy_id: BytesN<32>,
        license_id: BytesN<32>,
        rendition_id: BytesN<32>,
        seat_id: BytesN<32>,
    ) -> Result<BytesN<32>, ContractError> {
        borrower.require_auth();
        let registry = CourseRegistryClient::new(&env, &course_registry);
        if !registry.is_enrolled(&borrower, &course_id) {
            return Err(ContractError::NotEnrolled);
        }
        let policy = Self::latest_policy(env.clone(), policy_id.clone())?;
        if !policy.policy.active
            || policy.policy.scope.institution != institution
            || policy.policy.scope.role != borrower_role
            || policy.policy.scope.collection != collection
        {
            return Err(ContractError::InvalidPolicy);
        }
        let license = env
            .storage()
            .persistent()
            .get::<DK, LicenseRecord>(&DK::License(license_id.clone()))
            .ok_or(ContractError::LicenseNotFound)?;
        if !license.active
            || license.institution != institution
            || license.expires_at <= env.ledger().timestamp()
        {
            return Err(ContractError::LicenseInactive);
        }
        let rendition = env
            .storage()
            .persistent()
            .get::<DK, RenditionRecord>(&DK::Rendition(rendition_id.clone()))
            .ok_or(ContractError::RenditionNotFound)?;
        if !rendition.active
            || rendition.work_id != license.work_id
            || rendition.format != policy.policy.scope.format
        {
            return Err(ContractError::RenditionInactive);
        }
        let seat_key = DK::Seat(seat_id.clone());
        let mut seat = env
            .storage()
            .persistent()
            .get::<DK, SeatRecord>(&seat_key)
            .ok_or(ContractError::SeatNotFound)?;
        if !seat.available || seat.institution != institution {
            return Err(ContractError::SeatUnavailable);
        }
        let count_key = DK::BorrowerLoanCount(institution.clone(), borrower.clone());
        let count = env
            .storage()
            .persistent()
            .get::<DK, u32>(&count_key)
            .unwrap_or(0);
        if count >= policy.policy.max_concurrent_loans {
            return Err(ContractError::BorrowingLimitReached);
        }
        let due_at = env
            .ledger()
            .timestamp()
            .checked_add(policy.policy.loan_duration_secs)
            .ok_or(ContractError::InvalidTimestamp)?;
        let next = env
            .storage()
            .instance()
            .get::<DK, u64>(&DK::LoanCounter)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ContractError::LoanIdOverflow)?;
        env.storage().instance().set(&DK::LoanCounter, &next);
        let mut salt = Bytes::new(&env);
        salt.append(&Bytes::from_slice(&env, &next.to_be_bytes()));
        salt.append(&borrower.clone().to_xdr(&env));
        let loan_id: BytesN<32> = env.crypto().sha256(&salt).into();
        let loan = LoanRecord {
            loan_id: loan_id.clone(),
            borrower: borrower.clone(),
            institution: institution.clone(),
            work_id: license.work_id,
            license_id,
            rendition_id,
            seat_id,
            policy_id,
            policy_version: policy.version,
            checked_out_at: env.ledger().timestamp(),
            due_at,
            active: true,
        };
        seat.available = false;
        env.storage().persistent().set(&seat_key, &seat);
        env.storage().persistent().set(&count_key, &(count + 1));
        let loan_key = DK::Loan(loan_id.clone());
        env.storage().persistent().set(&loan_key, &loan);
        env.storage()
            .persistent()
            .extend_ttl(&loan_key, ACTIVE_MIN_TTL, ACTIVE_MAX_TTL);
        env.events().publish(
            (symbol_short!("CHECKOUT"),),
            (loan_id.clone(), borrower, policy.version),
        );
        Ok(loan_id)
    }

    pub fn get_loan(env: Env, loan_id: BytesN<32>) -> Result<LoanRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DK::Loan(loan_id))
            .ok_or(ContractError::LoanNotFound)
    }

    pub fn version(env: Env) -> String {
        String::from_str(&env, CONTRACT_VERSION)
    }
}

#[cfg(test)]
mod tests;
