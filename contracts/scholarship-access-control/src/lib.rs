#![no_std]
#![allow(clippy::too_many_arguments)]

//! Scholarship access-control contract.
//!
//! Enforces the seven actor roles — Student, Sponsor, Reviewer, Verifier,
//! Finance, Auditor, and Administrator — on every scholarship resource, with
//! program-scoped role bindings so cross-tenant access is structurally
//! impossible.
//!
//! ## Design
//!
//! - Roles are **program-scoped**: `Role(account, program_id, role) → bool`.
//!   A Sponsor on program A has no standing on program B unless explicitly
//!   granted. This is the structural cross-tenant isolation.
//! - **Global roles** (Administrator, Auditor) are stored under the
//!   all-zero `[0u8; 32]` scope so a single `has_role` call covers both
//!   global and scoped checks.
//! - Every `require_role` call also calls `account.require_auth()`, so
//!   on-chain callers cannot pretend to be someone else.
//! - A **resource permission table** maps `(resource_kind, role)` pairs to
//!   allowed operations. Callers check `can(resource, role, op)` before
//!   acting; this contract does the look-up deterministically.
//! - **Bounded storage**: at most `MAX_GRANTS_PER_PROGRAM` role grants may
//!   be active per program; a global ceiling `MAX_TOTAL_GRANTS` guards
//!   instance-counter overflow.
//! - TTL policy: role grants are kept for `RECORD_MAX_TTL`; permission
//!   table entries are kept permanently in instance storage.
//!
//! ## Ownership / privacy notes
//!
//! This contract stores only `Address` (public keys) and `BytesN<32>`
//! program identifiers — no names, grades, or other personal data.
//! Revocation is immediate; revoked grants are tombstoned (set to `false`)
//! so the audit trail remains in the event log without retaining PII.
//!
//! ## Migration notes
//!
//! Role assignments are persistent records keyed by
//! `(account, program_id, role)`. A contract upgrade that adds a new `Role`
//! variant does not affect existing keys; old grants remain valid. Removing
//! a variant would require a migration script that revokes outstanding
//! grants for that role before the upgrade.
//!
//! ## Adversarial contract tests
//!
//! See `tests/` for cross-tenant isolation tests, unauthorized role-grant
//! attempts, and revocation timing attacks.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;

/// ~1 year at 5-second ledgers.
const RECORD_MIN_TTL: u32 = 3_110_400;
/// ~2 years.
const RECORD_MAX_TTL: u32 = 6_220_800;

/// Maximum role grants tracked per program (bounds per-program index Vec).
const MAX_GRANTS_PER_PROGRAM: u32 = 256;

/// Absolute ceiling on total grants across all programs (guards instance counter).
const MAX_TOTAL_GRANTS: u64 = 50_000;

/// The all-zero scope is used for global role bindings (Administrator,
/// Auditor), which apply across all programs.
fn global_scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    /// Caller is not the contract administrator.
    NotAdmin = 3,
    /// The account does not hold the required role on this resource.
    Unauthorized = 4,
    /// The requested operation is not permitted for this role on this resource.
    OperationNotPermitted = 5,
    /// Per-program role-grant ceiling reached.
    GrantLimitExceeded = 6,
    /// Global grant ceiling reached.
    TotalGrantLimitExceeded = 7,
    /// Arithmetic overflow in a counter (practically unreachable).
    Overflow = 8,
    /// The requested resource kind is unknown to this build.
    UnknownResource = 9,
    /// The requested operation is unknown to this build.
    UnknownOperation = 10,
}

// ---------------------------------------------------------------------------
// Role enum
// ---------------------------------------------------------------------------

/// The seven actor roles named in the scholarship epic.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Student = 1,
    Sponsor = 2,
    Reviewer = 3,
    Verifier = 4,
    Finance = 5,
    Auditor = 6,
    Administrator = 7,
}

// ---------------------------------------------------------------------------
// Resource kinds and operations
// ---------------------------------------------------------------------------

/// Scholarship resources that are subject to role-based access control.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resource {
    Program = 1,
    Application = 2,
    Document = 3,
    Decision = 4,
    Disbursement = 5,
    Report = 6,
    Identity = 7,
    Communication = 8,
}

/// Operations that can be performed on a resource.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Read = 1,
    Write = 2,
    Approve = 3,
    Reject = 4,
    Disburse = 5,
    Audit = 6,
    Revoke = 7,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract admin.
    Admin,
    /// `Role(account, program_scope, role) → bool`.
    /// `program_scope` is the all-zero key for global roles.
    Role(Address, BytesN<32>, Role),
    /// Permission table entry: `(resource, role, operation) → bool`.
    Permission(Resource, Role, Operation),
    /// Bounded index: `ProgramGrantCount(program_scope) → u32`.
    ProgramGrantCount(BytesN<32>),
    /// Total grant counter stored in instance storage (survives upgrades).
    TotalGrantCount,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ScholarshipAccessControlContract;

#[contractimpl]
impl ScholarshipAccessControlContract {
    /// One-time initialization. The deployer becomes the first Administrator.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        // Bootstrap the admin's global Administrator role.
        Self::_grant_internal(
            &env,
            &admin,
            &global_scope(&env),
            Role::Administrator,
        )?;
        // Seed the default permission table.
        Self::init_permissions(&env);
        Ok(())
    }

    // ── admin guard ───────────────────────────────────────────────────────

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

    // ── role management ───────────────────────────────────────────────────

    /// Admin-only: grant `role` to `account` scoped to `program_id`.
    /// For global roles (Administrator, Auditor) pass the all-zero
    /// `global_scope()` as `program_id`.
    pub fn grant_role(
        env: Env,
        admin: Address,
        account: Address,
        program_id: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::_grant_internal(&env, &account, &program_id, role)
    }

    fn _grant_internal(
        env: &Env,
        account: &Address,
        program_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        // Check per-program ceiling.
        let prog_key = DataKey::ProgramGrantCount(program_id.clone());
        let prog_count: u32 = env
            .storage()
            .persistent()
            .get(&prog_key)
            .unwrap_or(0u32);
        // Only count new grants, not updates to existing ones.
        let role_key = DataKey::Role(account.clone(), program_id.clone(), role);
        let already_granted: bool = env
            .storage()
            .persistent()
            .get(&role_key)
            .unwrap_or(false);
        if !already_granted {
            if prog_count >= MAX_GRANTS_PER_PROGRAM {
                return Err(ContractError::GrantLimitExceeded);
            }
            // Check global ceiling.
            let total: u64 = env
                .storage()
                .instance()
                .get(&DataKey::TotalGrantCount)
                .unwrap_or(0u64);
            if total >= MAX_TOTAL_GRANTS {
                return Err(ContractError::TotalGrantLimitExceeded);
            }
            env.storage()
                .instance()
                .set(&DataKey::TotalGrantCount, &(total.checked_add(1).ok_or(ContractError::Overflow)?));
            env.storage()
                .persistent()
                .set(&prog_key, &(prog_count.checked_add(1).ok_or(ContractError::Overflow)?));
            env.storage()
                .persistent()
                .extend_ttl(&prog_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }

        env.storage().persistent().set(&role_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&role_key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (symbol_short!("ROLEGRT"),),
            (account.clone(), program_id.clone(), role),
        );
        Ok(())
    }

    /// Admin-only: revoke `role` from `account` for `program_id`.
    /// The storage entry is tombstoned (set to `false`) rather than deleted
    /// so the event log remains the authoritative audit trail.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        account: Address,
        program_id: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Role(account.clone(), program_id.clone(), role);
        // Only decrement the counter if the grant was active.
        let was_active: bool = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(false);
        env.storage().persistent().set(&key, &false);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        if was_active {
            let prog_key = DataKey::ProgramGrantCount(program_id.clone());
            let prog_count: u32 = env
                .storage()
                .persistent()
                .get(&prog_key)
                .unwrap_or(0u32);
            if prog_count > 0 {
                env.storage()
                    .persistent()
                    .set(&prog_key, &(prog_count - 1));
            }
            let total: u64 = env
                .storage()
                .instance()
                .get(&DataKey::TotalGrantCount)
                .unwrap_or(0u64);
            if total > 0 {
                env.storage()
                    .instance()
                    .set(&DataKey::TotalGrantCount, &(total - 1));
            }
        }

        env.events().publish(
            (symbol_short!("ROLERVK"),),
            (account, program_id, role),
        );
        Ok(())
    }

    /// True iff `account` holds `role` on `program_id` *or* globally.
    /// Global scope is checked automatically for every call.
    pub fn has_role(
        env: Env,
        account: Address,
        program_id: BytesN<32>,
        role: Role,
    ) -> bool {
        // Check program-scoped grant.
        let scoped: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Role(account.clone(), program_id, role))
            .unwrap_or(false);
        if scoped {
            return true;
        }
        // Fallback: check global grant (all-zero scope).
        env.storage()
            .persistent()
            .get(&DataKey::Role(account, global_scope(&env), role))
            .unwrap_or(false)
    }

    /// Require `account` to hold `role` on `program_id` and call
    /// `account.require_auth()`. Returns `Unauthorized` on failure.
    /// This is the primary entry-point for other contracts.
    pub fn require_role(
        env: Env,
        account: Address,
        program_id: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        account.require_auth();
        if Self::has_role(env, account, program_id, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    // ── permission table ──────────────────────────────────────────────────

    /// Admin-only: explicitly allow or deny a `(resource, role, operation)`
    /// triple. Stored in instance storage (low-cost, survives upgrades).
    pub fn set_permission(
        env: Env,
        admin: Address,
        resource: Resource,
        role: Role,
        operation: Operation,
        allowed: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Permission(resource, role, operation), &allowed);
        env.events().publish(
            (symbol_short!("PERMSET"),),
            (resource, role, operation, allowed),
        );
        Ok(())
    }

    /// True iff `role` may perform `operation` on `resource`.
    pub fn can(
        env: Env,
        resource: Resource,
        role: Role,
        operation: Operation,
    ) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Permission(resource, role, operation))
            .unwrap_or(false)
    }

    /// Combined check: `account` has `role` on `program_id` AND `role` may
    /// perform `operation` on `resource`. Calls `account.require_auth()`.
    pub fn authorize(
        env: Env,
        account: Address,
        program_id: BytesN<32>,
        role: Role,
        resource: Resource,
        operation: Operation,
    ) -> Result<(), ContractError> {
        account.require_auth();
        if !Self::has_role(env.clone(), account.clone(), program_id, role) {
            return Err(ContractError::Unauthorized);
        }
        if !Self::can(env, resource, role, operation) {
            return Err(ContractError::OperationNotPermitted);
        }
        Ok(())
    }

    /// Returns the global scope identifier (all-zero BytesN<32>).
    pub fn global_scope(env: Env) -> BytesN<32> {
        global_scope(&env)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    // ── default permission table ──────────────────────────────────────────

    /// Seed a sensible default permission table. Called once from
    /// `initialize`. Individual entries may be overridden with
    /// `set_permission`.
    fn init_permissions(env: &Env) {
        use Operation::*;
        use Resource::*;
        use Role::*;

        // Format: (resource, role, operation) → allowed
        let permissions: &[(Resource, Role, Operation, bool)] = &[
            // Student
            (Application, Student, Read, true),
            (Application, Student, Write, true),
            (Document, Student, Write, true),
            (Document, Student, Read, true),
            (Identity, Student, Write, true),
            (Identity, Student, Read, true),
            (Communication, Student, Read, true),
            // Sponsor
            (Program, Sponsor, Read, true),
            (Program, Sponsor, Write, true),
            (Application, Sponsor, Read, true),
            (Disbursement, Sponsor, Read, true),
            (Report, Sponsor, Read, true),
            (Communication, Sponsor, Write, true),
            (Communication, Sponsor, Read, true),
            // Reviewer
            (Application, Reviewer, Read, true),
            (Document, Reviewer, Read, true),
            (Decision, Reviewer, Read, true),
            (Decision, Reviewer, Write, true),
            // Verifier
            (Identity, Verifier, Read, true),
            (Identity, Verifier, Write, true),
            (Identity, Verifier, Approve, true),
            (Identity, Verifier, Revoke, true),
            // Finance
            (Disbursement, Finance, Read, true),
            (Disbursement, Finance, Write, true),
            (Disbursement, Finance, Disburse, true),
            (Report, Finance, Read, true),
            (Report, Finance, Write, true),
            // Auditor
            (Application, Auditor, Read, true),
            (Decision, Auditor, Read, true),
            (Disbursement, Auditor, Audit, true),
            (Report, Auditor, Read, true),
            (Identity, Auditor, Audit, true),
            // Administrator
            (Program, Administrator, Read, true),
            (Program, Administrator, Write, true),
            (Program, Administrator, Approve, true),
            (Program, Administrator, Reject, true),
            (Application, Administrator, Read, true),
            (Application, Administrator, Approve, true),
            (Application, Administrator, Reject, true),
            (Document, Administrator, Read, true),
            (Document, Administrator, Revoke, true),
            (Decision, Administrator, Read, true),
            (Decision, Administrator, Approve, true),
            (Decision, Administrator, Reject, true),
            (Disbursement, Administrator, Disburse, true),
            (Disbursement, Administrator, Revoke, true),
            (Report, Administrator, Read, true),
            (Report, Administrator, Write, true),
            (Identity, Administrator, Read, true),
            (Identity, Administrator, Revoke, true),
            (Communication, Administrator, Read, true),
            (Communication, Administrator, Write, true),
        ];

        for &(resource, role, op, allowed) in permissions {
            env.storage()
                .instance()
                .set(&DataKey::Permission(resource, role, op), &allowed);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
