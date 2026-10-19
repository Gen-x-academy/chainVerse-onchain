#![no_std]

//! Scoped role and permission registry for mentorship workflows.
//!
//! Only addresses, opaque 32-byte scope identifiers, role grants, and the
//! permission matrix are stored on-chain. Session content and personal data
//! remain off-chain.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

fn global_scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    Unauthorized = 4,
    PermissionDenied = 5,
    InvalidScope = 6,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Student = 1,
    Tutor = 2,
    Mentor = 3,
    Guardian = 4,
    OrganizationStaff = 5,
    Moderator = 6,
    FinanceStaff = 7,
    Administrator = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    ViewEngagement = 1,
    CreateEngagement = 2,
    UpdateEngagement = 3,
    ManageAvailability = 4,
    SubmitSessionEvidence = 5,
    ManageGuardianConsent = 6,
    ViewPaymentStatus = 7,
    InitiateSettlement = 8,
    ApproveSettlement = 9,
    RaiseSafetyConcern = 10,
    ReviewSafetyCase = 11,
    ModerateParticipant = 12,
    ManageOrganizationRoster = 13,
    ManageRoles = 14,
    AuditAccess = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, Role),
    Permission(Role, Permission),
}

#[contract]
pub struct MentorshipAccessControlContract;

#[contractimpl]
impl MentorshipAccessControlContract {
    /// Initialize once; the supplied administrator must authorize deployment setup.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::grant_internal(&env, &admin, &global_scope(&env), Role::Administrator);
        Self::initialize_permissions(&env);
        Ok(())
    }

    /// Grant a role within a mentorship or organization scope. Only the
    /// administrator may assign roles; the zero scope is reserved for
    /// Administrator so other roles can never become accidentally global.
    pub fn grant_role(
        env: Env,
        admin: Address,
        account: Address,
        scope: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_scope(&env, &scope, role)?;
        Self::grant_internal(&env, &account, &scope, role);
        Ok(())
    }

    /// Revoke a role immediately. The false tombstone and event preserve an
    /// auditable state transition without storing private participant data.
    pub fn revoke_role(
        env: Env,
        admin: Address,
        account: Address,
        scope: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::validate_scope(&env, &scope, role)?;
        let key = DataKey::Role(account.clone(), scope.clone(), role);
        env.storage().persistent().set(&key, &false);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("MROLE_RVK"),), (account, scope, role));
        Ok(())
    }

    /// Read-only role check. Administrator grants apply to every scope.
    pub fn has_role(env: Env, account: Address, scope: BytesN<32>, role: Role) -> bool {
        let key = DataKey::Role(account.clone(), scope.clone(), role);
        if Self::active_role(&env, &key) {
            return true;
        }
        if role == Role::Administrator {
            return Self::active_role(
                &env,
                &DataKey::Role(account, global_scope(&env), Role::Administrator),
            );
        }
        false
    }

    /// Require the caller to hold the requested role in this exact scope.
    pub fn require_role(
        env: Env,
        account: Address,
        scope: BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        account.require_auth();
        if Self::has_role(env, account, scope, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    /// Update one matrix entry. Only the contract administrator can change
    /// policy; changes are published for off-chain monitoring.
    pub fn set_permission(
        env: Env,
        admin: Address,
        role: Role,
        permission: Permission,
        allowed: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::Permission(role, permission), &allowed);
        env.events().publish(
            (symbol_short!("MPERM_SET"),),
            (role, permission, allowed),
        );
        Ok(())
    }

    /// Return whether the role is allowed to perform the permission.
    pub fn can(env: Env, role: Role, permission: Permission) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Permission(role, permission))
            .unwrap_or(false)
    }

    /// Authenticate the account, verify its scoped role, then check the
    /// seeded or administrator-configured permission matrix.
    pub fn authorize(
        env: Env,
        account: Address,
        scope: BytesN<32>,
        role: Role,
        permission: Permission,
    ) -> Result<(), ContractError> {
        account.require_auth();
        if !Self::has_role(env.clone(), account, scope, role) {
            return Err(ContractError::Unauthorized);
        }
        if !Self::can(env, role, permission) {
            return Err(ContractError::PermissionDenied);
        }
        Ok(())
    }

    pub fn global_scope(env: Env) -> BytesN<32> {
        global_scope(&env)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &admin != caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn validate_scope(env: &Env, scope: &BytesN<32>, role: Role) -> Result<(), ContractError> {
        let is_global = *scope == global_scope(env);
        if is_global != (role == Role::Administrator) {
            return Err(ContractError::InvalidScope);
        }
        Ok(())
    }

    fn grant_internal(env: &Env, account: &Address, scope: &BytesN<32>, role: Role) {
        let key = DataKey::Role(account.clone(), scope.clone(), role);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("MROLE_GRT"),),
            (account.clone(), scope.clone(), role),
        );
    }

    fn active_role(env: &Env, key: &DataKey) -> bool {
        let active: bool = env.storage().persistent().get(key).unwrap_or(false);
        if active {
            env.storage()
                .persistent()
                .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        active
    }

    fn initialize_permissions(env: &Env) {
        use Permission::*;
        use Role::*;

        let permissions: &[(Role, Permission)] = &[
            (Student, ViewEngagement),
            (Student, CreateEngagement),
            (Student, UpdateEngagement),
            (Student, SubmitSessionEvidence),
            (Student, RaiseSafetyConcern),
            (Tutor, ViewEngagement),
            (Tutor, UpdateEngagement),
            (Tutor, ManageAvailability),
            (Tutor, SubmitSessionEvidence),
            (Tutor, ViewPaymentStatus),
            (Tutor, RaiseSafetyConcern),
            (Mentor, ViewEngagement),
            (Mentor, UpdateEngagement),
            (Mentor, ManageAvailability),
            (Mentor, SubmitSessionEvidence),
            (Mentor, ViewPaymentStatus),
            (Mentor, RaiseSafetyConcern),
            (Guardian, ViewEngagement),
            (Guardian, ManageGuardianConsent),
            (Guardian, ViewPaymentStatus),
            (Guardian, RaiseSafetyConcern),
            (OrganizationStaff, ViewEngagement),
            (OrganizationStaff, ManageOrganizationRoster),
            (Moderator, ReviewSafetyCase),
            (Moderator, ModerateParticipant),
            (FinanceStaff, ViewPaymentStatus),
            (FinanceStaff, InitiateSettlement),
            (Administrator, ApproveSettlement),
            (Administrator, ManageRoles),
            (Administrator, AuditAccess),
        ];

        for &(role, permission) in permissions {
            env.storage()
                .instance()
                .set(&DataKey::Permission(role, permission), &true);
        }
    }
}

#[cfg(test)]
mod tests;