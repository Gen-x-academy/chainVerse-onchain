#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_SNAPSHOTS: u32 = 64;
const MAX_AUDIT_ENTRIES: u32 = 96;
const MIN_WINDOW_SECONDS: u32 = 60;
const MAX_WINDOW_SECONDS: u32 = 2_592_000;
const MIN_GRANT_TTL_SECONDS: u32 = 60;
const MAX_GRANT_TTL_SECONDS: u32 = 604_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    NotRegistrar = 4,
    NotAuthorized = 5,
    ProjectNotFound = 6,
    ProjectAlreadyRegistered = 7,
    ZeroHash = 8,
    SnapshotNotFound = 9,
    SnapshotAlreadyExists = 10,
    SnapshotLimitReached = 11,
    ExportRateLimited = 12,
    GrantNotFound = 13,
    GrantAlreadyExists = 14,
    GrantExpired = 15,
    InvalidPolicy = 16,
    AuditLimitReached = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Participant = 1,
    Coordinator = 2,
    Registrar = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotKind {
    AcceptedTerms = 1,
    Amendments = 2,
    Approvals = 3,
    Membership = 4,
    MilestoneSummary = 5,
    FinancialSummary = 6,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditKind {
    Requested = 1,
    Downloaded = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportPolicy {
    pub max_exports_per_window: u32,
    pub window_seconds: u32,
    pub grant_ttl_seconds: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRecord {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub terms_hash: BytesN<32>,
    pub registered_at: u64,
    pub snapshot_ids: Vec<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotRef {
    pub id: BytesN<32>,
    pub kind: SnapshotKind,
    pub hash: BytesN<32>,
    pub recorded_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportGrant {
    pub grant_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub requester: Address,
    pub created_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportQuota {
    pub window_started_at: u64,
    pub used: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub at: u64,
    pub actor: Address,
    pub grant_id: BytesN<32>,
    pub kind: AuditKind,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportManifest {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub terms_hash: BytesN<32>,
    pub registered_at: u64,
    pub snapshots: Vec<SnapshotRef>,
    pub snapshot_count: u32,
    pub generated_at: u64,
    pub generated_for: Address,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Policy,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Project(BytesN<32>, BytesN<32>),
    Snapshot(BytesN<32>, BytesN<32>, BytesN<32>),
    Grant(BytesN<32>),
    Quota(Address),
    Audit(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneGovernanceExportContract;

#[contractimpl]
impl CapstoneGovernanceExportContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        max_exports_per_window: u32,
        window_seconds: u32,
        grant_ttl_seconds: u32,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        let policy = ExportPolicy {
            max_exports_per_window,
            window_seconds,
            grant_ttl_seconds,
        };
        Self::validate_policy(&policy)?;
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Policy, &policy);
        Ok(())
    }

    pub fn admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn policy(env: Env) -> Result<ExportPolicy, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Policy)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn version(_env: Env) -> u32 {
        SCHEMA_VERSION
    }

    pub fn set_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        role: Role,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(account.clone(), tenant.clone(), project.clone(), role.clone());
        env.storage().persistent().set(&key, &active);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("GVROLE"),),
            (tenant, project, account, role, active),
        );
        Ok(())
    }

    pub fn register_project(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        terms_hash: BytesN<32>,
    ) -> Result<ProjectRecord, ContractError> {
        caller.require_auth();
        Self::require_registrar(&env, &caller, &tenant, &project)?;
        if terms_hash == Self::zero_hash(&env) {
            return Err(ContractError::ZeroHash);
        }
        let key = DataKey::Project(tenant.clone(), project.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProjectAlreadyRegistered);
        }
        let record = ProjectRecord {
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            terms_hash: terms_hash.clone(),
            registered_at: env.ledger().timestamp(),
            snapshot_ids: Vec::new(&env),
        };
        env.storage().persistent().set(&key, &record);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("GVREG"),),
            (tenant, project, terms_hash),
        );
        Ok(record)
    }

    pub fn record_snapshot(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        snapshot_id: BytesN<32>,
        kind: SnapshotKind,
        hash: BytesN<32>,
    ) -> Result<SnapshotRef, ContractError> {
        caller.require_auth();
        Self::require_registrar(&env, &caller, &tenant, &project)?;
        let zero = Self::zero_hash(&env);
        if snapshot_id == zero || hash == zero {
            return Err(ContractError::ZeroHash);
        }
        let project_key = DataKey::Project(tenant.clone(), project.clone());
        let mut record = Self::load_project(&env, &project_key)?;
        let snapshot_key = DataKey::Snapshot(
            tenant.clone(),
            project.clone(),
            snapshot_id.clone(),
        );
        if env.storage().persistent().has(&snapshot_key) {
            return Err(ContractError::SnapshotAlreadyExists);
        }
        if record.snapshot_ids.len() >= MAX_SNAPSHOTS {
            return Err(ContractError::SnapshotLimitReached);
        }
        let snapshot = SnapshotRef {
            id: snapshot_id.clone(),
            kind: kind.clone(),
            hash: hash.clone(),
            recorded_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&snapshot_key, &snapshot);
        record.snapshot_ids.push_back(snapshot_id.clone());
        env.storage().persistent().set(&project_key, &record);
        Self::extend_persistent(&env, &snapshot_key);
        Self::extend_persistent(&env, &project_key);
        env.events().publish(
            (symbol_short!("GVSNAP"),),
            (tenant, project, snapshot_id, kind, hash),
        );
        Ok(snapshot)
    }

    pub fn request_export(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        grant_id: BytesN<32>,
    ) -> Result<ExportGrant, ContractError> {
        caller.require_auth();
        let project_key = DataKey::Project(tenant.clone(), project.clone());
        Self::load_project(&env, &project_key)?;
        Self::require_export_role(&env, &caller, &tenant, &project)?;
        if grant_id == Self::zero_hash(&env) {
            return Err(ContractError::ZeroHash);
        }
        let now = env.ledger().timestamp();
        let grant_key = DataKey::Grant(grant_id.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, ExportGrant>(&grant_key)
        {
            Self::extend_persistent(&env, &grant_key);
            if existing.tenant_id == tenant
                && existing.project_id == project
                && existing.requester == caller
            {
                return Ok(existing);
            }
            return Err(ContractError::GrantAlreadyExists);
        }
        let policy: ExportPolicy = env
            .storage()
            .instance()
            .get(&DataKey::Policy)
            .ok_or(ContractError::NotInitialized)?;
        let quota_key = DataKey::Quota(caller.clone());
        let mut quota: ExportQuota =
            env.storage()
                .persistent()
                .get(&quota_key)
                .unwrap_or(ExportQuota {
                    window_started_at: now,
                    used: 0,
                });
        let window_elapsed = now
            .checked_sub(quota.window_started_at)
            .map(|age| age >= u64::from(policy.window_seconds))
            .unwrap_or(true);
        if window_elapsed {
            quota = ExportQuota {
                window_started_at: now,
                used: 0,
            };
        }
        if quota.used >= policy.max_exports_per_window {
            return Err(ContractError::ExportRateLimited);
        }
        quota.used = quota
            .used
            .checked_add(1)
            .ok_or(ContractError::ExportRateLimited)?;
        let expires_at = now
            .checked_add(u64::from(policy.grant_ttl_seconds))
            .ok_or(ContractError::InvalidPolicy)?;
        let grant = ExportGrant {
            grant_id: grant_id.clone(),
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            requester: caller.clone(),
            created_at: now,
            expires_at,
        };
        env.storage().persistent().set(&grant_key, &grant);
        env.storage().persistent().set(&quota_key, &quota);
        Self::extend_persistent(&env, &grant_key);
        Self::extend_persistent(&env, &quota_key);
        Self::push_audit(
            &env,
            &tenant,
            &project,
            &AuditEntry {
                at: now,
                actor: caller.clone(),
                grant_id: grant_id.clone(),
                kind: AuditKind::Requested,
            },
        )?;
        env.events().publish(
            (symbol_short!("GVEXREQ"),),
            (grant_id, tenant, project, caller, expires_at),
        );
        Ok(grant)
    }

    pub fn get_export(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
    ) -> Result<ExportManifest, ContractError> {
        caller.require_auth();
        let grant_key = DataKey::Grant(grant_id.clone());
        let grant: ExportGrant = env
            .storage()
            .persistent()
            .get(&grant_key)
            .ok_or(ContractError::GrantNotFound)?;
        Self::extend_persistent(&env, &grant_key);
        if grant.requester != caller {
            return Err(ContractError::NotAuthorized);
        }
        let now = env.ledger().timestamp();
        if now > grant.expires_at {
            return Err(ContractError::GrantExpired);
        }
        let project_key = DataKey::Project(
            grant.tenant_id.clone(),
            grant.project_id.clone(),
        );
        let record = Self::load_project(&env, &project_key)?;
        let mut snapshots = Vec::new(&env);
        for i in 0..record.snapshot_ids.len() {
            if let Some(id) = record.snapshot_ids.get(i) {
                let snapshot_key = DataKey::Snapshot(
                    grant.tenant_id.clone(),
                    grant.project_id.clone(),
                    id.clone(),
                );
                let snapshot: SnapshotRef = env
                    .storage()
                    .persistent()
                    .get(&snapshot_key)
                    .ok_or(ContractError::SnapshotNotFound)?;
                Self::extend_persistent(&env, &snapshot_key);
                snapshots.push_back(snapshot);
            }
        }
        let snapshot_count = snapshots.len();
        Self::push_audit(
            &env,
            &grant.tenant_id,
            &grant.project_id,
            &AuditEntry {
                at: now,
                actor: caller.clone(),
                grant_id: grant_id.clone(),
                kind: AuditKind::Downloaded,
            },
        )?;
        env.events().publish(
            (symbol_short!("GVEXDL"),),
            (grant_id, grant.tenant_id, grant.project_id, caller.clone()),
        );
        Ok(ExportManifest {
            tenant_id: record.tenant_id,
            project_id: record.project_id,
            terms_hash: record.terms_hash,
            registered_at: record.registered_at,
            snapshots,
            snapshot_count,
            generated_at: now,
            generated_for: caller,
        })
    }

    pub fn get_grant(
        env: Env,
        caller: Address,
        grant_id: BytesN<32>,
    ) -> Result<ExportGrant, ContractError> {
        caller.require_auth();
        let key = DataKey::Grant(grant_id);
        let grant: ExportGrant = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::GrantNotFound)?;
        Self::extend_persistent(&env, &key);
        if grant.requester != caller {
            return Err(ContractError::NotAuthorized);
        }
        Ok(grant)
    }

    pub fn get_audit(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
    ) -> Result<Vec<AuditEntry>, ContractError> {
        caller.require_auth();
        Self::require_export_role(&env, &caller, &tenant, &project)?;
        let key = DataKey::Audit(tenant, project);
        let entries: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if env.storage().persistent().has(&key) {
            Self::extend_persistent(&env, &key);
        }
        Ok(entries)
    }

    pub fn get_project(
        env: Env,
        tenant: BytesN<32>,
        project: BytesN<32>,
    ) -> Result<ProjectRecord, ContractError> {
        let key = DataKey::Project(tenant, project);
        Self::load_project(&env, &key)
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

    fn require_registrar(
        env: &Env,
        caller: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller == admin {
            return Ok(());
        }
        let key = DataKey::Role(
            caller.clone(),
            tenant.clone(),
            project.clone(),
            Role::Registrar,
        );
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !active {
            return Err(ContractError::NotRegistrar);
        }
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn require_export_role(
        env: &Env,
        caller: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let participant = DataKey::Role(
            caller.clone(),
            tenant.clone(),
            project.clone(),
            Role::Participant,
        );
        let coordinator = DataKey::Role(
            caller.clone(),
            tenant.clone(),
            project.clone(),
            Role::Coordinator,
        );
        let is_participant: bool = env
            .storage()
            .persistent()
            .get(&participant)
            .unwrap_or(false);
        let is_coordinator: bool = env
            .storage()
            .persistent()
            .get(&coordinator)
            .unwrap_or(false);
        if !is_participant && !is_coordinator {
            return Err(ContractError::NotAuthorized);
        }
        if env.storage().persistent().has(&participant) {
            Self::extend_persistent(env, &participant);
        }
        if env.storage().persistent().has(&coordinator) {
            Self::extend_persistent(env, &coordinator);
        }
        Ok(())
    }

    fn validate_policy(policy: &ExportPolicy) -> Result<(), ContractError> {
        if policy.max_exports_per_window == 0
            || policy.window_seconds < MIN_WINDOW_SECONDS
            || policy.window_seconds > MAX_WINDOW_SECONDS
            || policy.grant_ttl_seconds < MIN_GRANT_TTL_SECONDS
            || policy.grant_ttl_seconds > MAX_GRANT_TTL_SECONDS
        {
            return Err(ContractError::InvalidPolicy);
        }
        Ok(())
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn load_project(env: &Env, key: &DataKey) -> Result<ProjectRecord, ContractError> {
        let record: ProjectRecord = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::ProjectNotFound)?;
        Self::extend_persistent(env, key);
        Ok(record)
    }

    fn push_audit(
        env: &Env,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
        entry: &AuditEntry,
    ) -> Result<(), ContractError> {
        let key = DataKey::Audit(tenant.clone(), project.clone());
        let mut entries: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if entries.len() >= MAX_AUDIT_ENTRIES {
            return Err(ContractError::AuditLimitReached);
        }
        entries.push_back(entry.clone());
        env.storage().persistent().set(&key, &entries);
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
