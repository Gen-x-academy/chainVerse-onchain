#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

pub const MAX_MEMBERS: u32 = 32;
pub const MAX_RESOURCES: u32 = 32;
pub const MAX_VERSIONS: u32 = 16;
pub const MAX_PAGE: u32 = 50;
pub const MAX_RETENTION: u64 = 6_220_800;
pub const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    NotFound = 4,
    StaleVersion = 5,
    RequestConflict = 6,
    AlreadyExists = 7,
    InvalidTransition = 8,
    ResourcesFull = 9,
    VersionsFull = 10,
    MembersFull = 11,
    Expired = 12,
    InvalidRetention = 13,
    TimestampOverflow = 14,
    VersionOverflow = 15,
    InvalidPage = 16,
    UnsupportedSchema = 17,
    AccessDenied = 18,
    InvalidIntegrity = 19,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Config(BytesN<32>, BytesN<32>),
    Role(BytesN<32>, BytesN<32>, Address),
    Resource(BytesN<32>, BytesN<32>, BytesN<32>),
    ResourceIndex(BytesN<32>, BytesN<32>),
    ResourceVersion(BytesN<32>, BytesN<32>, BytesN<32>, u32),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryRole {
    Member,
    Reviewer,
    Curator,
    Admin,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    File,
    Dataset,
    Link,
    Citation,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LicenseKind {
    Restricted,
    Internal,
    CcBy,
    Mit,
    Apache2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessPolicy {
    OwnerOnly,
    ProjectMembers,
    Reviewers,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceState {
    Proposed,
    Approved,
    Deprecated,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceAction {
    Grant,
    Propose,
    Approve,
    Deprecate,
    Version,
    Policy,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryConfig {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub admin: Address,
    pub state_version: u32,
    pub member_count: u32,
    pub resource_count: u32,
    pub created_at: u64,
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDraft {
    pub resource_id: BytesN<32>,
    pub kind: ResourceKind,
    pub content_hash: BytesN<32>,
    pub integrity_hash: BytesN<32>,
    pub size_bytes: u64,
    pub license: LicenseKind,
    pub license_ref_hash: BytesN<32>,
    pub policy: AccessPolicy,
    pub retention_seconds: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceVersionInput {
    pub resource_id: BytesN<32>,
    pub content_hash: BytesN<32>,
    pub integrity_hash: BytesN<32>,
    pub size_bytes: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRecord {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub resource_id: BytesN<32>,
    pub owner: Address,
    pub kind: ResourceKind,
    pub content_hash: BytesN<32>,
    pub integrity_hash: BytesN<32>,
    pub size_bytes: u64,
    pub license: LicenseKind,
    pub license_ref_hash: BytesN<32>,
    pub policy: AccessPolicy,
    pub state: ResourceState,
    pub version: u32,
    pub expires_at: u64,
    pub updated_at: u64,
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceVersionRecord {
    pub resource_id: BytesN<32>,
    pub version: u32,
    pub content_hash: BytesN<32>,
    pub integrity_hash: BytesN<32>,
    pub size_bytes: u64,
    pub recorded_at: u64,
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
struct RequestRecord {
    payload_hash: BytesN<32>,
    result: BytesN<32>,
    recorded_at: u64,
}

#[contract]
pub struct CapstoneResourceLibraryContract;

#[contractimpl]
impl CapstoneResourceLibraryContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        if env.storage().persistent().has(&config_key) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        let config = LibraryConfig {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            admin: admin.clone(),
            state_version: 0,
            member_count: 1,
            resource_count: 0,
            created_at: env.ledger().timestamp(),
            schema_version: SCHEMA_VERSION,
        };
        env.storage().persistent().set(&config_key, &config);
        Self::extend(&env, &config_key);
        let role_key = Self::role_key(&tenant_id, &project_id, &admin);
        env.storage().persistent().set(&role_key, &LibraryRole::Admin);
        Self::extend(&env, &role_key);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        member: Address,
        role: LibraryRole,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &admin, LibraryRole::Admin)?;
        let admin_hash = Self::address_hash(&env, &admin);
        let member_hash = Self::address_hash(&env, &member);
        let role_bytes = Self::role_code(role).to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Grant).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                admin_hash.as_slice(),
                member_hash.as_slice(),
                &role_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &request_id, &payload, &member_hash)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        let member_key = Self::role_key(&tenant_id, &project_id, &member);
        if env.storage().persistent().has(&member_key) {
            return Err(ContractError::AlreadyExists);
        }
        let member_count = config
            .member_count
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        if member_count > MAX_MEMBERS {
            return Err(ContractError::MembersFull);
        }
        config.member_count = member_count;
        env.storage().persistent().set(&member_key, &role);
        Self::extend(&env, &member_key);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &request_id, &payload, &member_hash);
        env.events().publish(
            (symbol_short!("RSROLE"),),
            (
                member_hash,
                ResourceAction::Grant,
                config.state_version,
                Self::role_code(role),
            ),
        );
        Ok(member_hash)
    }

    pub fn propose_resource(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        draft: ResourceDraft,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            LibraryRole::Curator,
        )?;
        let actor_hash = Self::address_hash(&env, &actor);
        let id_bytes = draft.resource_id.to_array();
        let content_bytes = draft.content_hash.to_array();
        let integrity_bytes = draft.integrity_hash.to_array();
        let license_bytes = draft.license_ref_hash.to_array();
        let kind_bytes = Self::kind_code(draft.kind).to_be_bytes();
        let license_code_bytes = Self::license_code(draft.license).to_be_bytes();
        let policy_bytes = Self::policy_code(draft.policy).to_be_bytes();
        let size_bytes = draft.size_bytes.to_be_bytes();
        let retention_bytes = draft.retention_seconds.to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Propose).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &content_bytes,
                &integrity_bytes,
                &license_bytes,
                &kind_bytes,
                &license_code_bytes,
                &policy_bytes,
                &size_bytes,
                &retention_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) = Self::replay(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &draft.resource_id,
        )? {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if draft.size_bytes == 0
            || draft.integrity_hash == Self::zero_hash(&env)
            || draft.content_hash == Self::zero_hash(&env)
        {
            return Err(ContractError::InvalidIntegrity);
        }
        if draft.retention_seconds == 0 || draft.retention_seconds > MAX_RETENTION {
            return Err(ContractError::InvalidRetention);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(draft.retention_seconds)
            .ok_or(ContractError::TimestampOverflow)?;
        let key = Self::resource_key(&tenant_id, &project_id, &draft.resource_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = Self::resource_index_key(&tenant_id, &project_id);
        let mut index = Self::load_index(&env, &index_key);
        if index.len() >= MAX_RESOURCES {
            return Err(ContractError::ResourcesFull);
        }
        index.push_back(draft.resource_id.clone());
        Self::store_index(&env, &index_key, &index);
        let resource = ResourceRecord {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            resource_id: draft.resource_id.clone(),
            owner: actor,
            kind: draft.kind,
            content_hash: draft.content_hash.clone(),
            integrity_hash: draft.integrity_hash.clone(),
            size_bytes: draft.size_bytes,
            license: draft.license,
            license_ref_hash: draft.license_ref_hash,
            policy: draft.policy,
            state: ResourceState::Proposed,
            version: 1,
            expires_at,
            updated_at: now,
            schema_version: SCHEMA_VERSION,
        };
        Self::store_resource(&env, &key, &resource);
        let version_key = Self::resource_version_key(
            &tenant_id,
            &project_id,
            &draft.resource_id,
            1,
        );
        let version_record = ResourceVersionRecord {
            resource_id: draft.resource_id.clone(),
            version: 1,
            content_hash: draft.content_hash.clone(),
            integrity_hash: draft.integrity_hash.clone(),
            size_bytes: draft.size_bytes,
            recorded_at: now,
            schema_version: SCHEMA_VERSION,
        };
        env.storage().persistent().set(&version_key, &version_record);
        Self::extend(&env, &version_key);
        let resource_count = config
            .resource_count
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        config.resource_count = resource_count;
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &draft.resource_id,
        );
        env.events().publish(
            (symbol_short!("RSCP"),),
            (
                draft.resource_id,
                ResourceAction::Propose,
                1u32,
                draft.content_hash,
            ),
        );
        Ok(resource.resource_id)
    }

    pub fn approve_resource(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &admin, LibraryRole::Admin)?;
        let mut resource = Self::load_resource(&env, &tenant_id, &project_id, &resource_id)?;
        let actor_hash = Self::address_hash(&env, &admin);
        let id_bytes = resource_id.to_array();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Approve).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if resource.state != ResourceState::Proposed {
            return Err(ContractError::InvalidTransition);
        }
        resource.state = ResourceState::Approved;
        resource.updated_at = env.ledger().timestamp();
        let key = Self::resource_key(&tenant_id, &project_id, &resource_id);
        Self::store_resource(&env, &key, &resource);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id);
        env.events().publish(
            (symbol_short!("RSCA"),),
            (
                resource_id,
                ResourceAction::Approve,
                config.state_version,
                resource.content_hash,
            ),
        );
        Ok(resource.resource_id)
    }

    pub fn deprecate_resource(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &admin, LibraryRole::Admin)?;
        let mut resource = Self::load_resource(&env, &tenant_id, &project_id, &resource_id)?;
        let actor_hash = Self::address_hash(&env, &admin);
        let id_bytes = resource_id.to_array();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Deprecate).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if resource.state != ResourceState::Approved {
            return Err(ContractError::InvalidTransition);
        }
        resource.state = ResourceState::Deprecated;
        resource.updated_at = env.ledger().timestamp();
        let key = Self::resource_key(&tenant_id, &project_id, &resource_id);
        Self::store_resource(&env, &key, &resource);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id);
        env.events().publish(
            (symbol_short!("RSCD"),),
            (
                resource_id,
                ResourceAction::Deprecate,
                config.state_version,
                resource.content_hash,
            ),
        );
        Ok(resource.resource_id)
    }

    pub fn add_version(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        input: ResourceVersionInput,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &actor, LibraryRole::Curator)?;
        let mut resource = Self::load_resource(&env, &tenant_id, &project_id, &input.resource_id)?;
        let actor_hash = Self::address_hash(&env, &actor);
        let id_bytes = input.resource_id.to_array();
        let content_bytes = input.content_hash.to_array();
        let integrity_bytes = input.integrity_hash.to_array();
        let size_bytes = input.size_bytes.to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Version).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &content_bytes,
                &integrity_bytes,
                &size_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) = Self::replay(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &input.content_hash,
        )? {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if resource.state == ResourceState::Deprecated {
            return Err(ContractError::InvalidTransition);
        }
        if input.size_bytes == 0 || input.integrity_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidIntegrity);
        }
        if resource.version >= MAX_VERSIONS {
            return Err(ContractError::VersionsFull);
        }
        let version = resource
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let key = Self::resource_version_key(
            &tenant_id,
            &project_id,
            &input.resource_id,
            version,
        );
        let record = ResourceVersionRecord {
            resource_id: input.resource_id.clone(),
            version,
            content_hash: input.content_hash.clone(),
            integrity_hash: input.integrity_hash.clone(),
            size_bytes: input.size_bytes,
            recorded_at: env.ledger().timestamp(),
            schema_version: SCHEMA_VERSION,
        };
        env.storage().persistent().set(&key, &record);
        Self::extend(&env, &key);
        resource.version = version;
        resource.content_hash = input.content_hash.clone();
        resource.integrity_hash = input.integrity_hash;
        resource.size_bytes = input.size_bytes;
        resource.updated_at = env.ledger().timestamp();
        let resource_key = Self::resource_key(&tenant_id, &project_id, &input.resource_id);
        Self::store_resource(&env, &resource_key, &resource);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &input.content_hash,
        );
        env.events().publish(
            (symbol_short!("RSCV"),),
            (
                input.resource_id,
                ResourceAction::Version,
                version,
                input.content_hash,
            ),
        );
        Ok(input.content_hash)
    }

    pub fn set_policy(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
        policy: AccessPolicy,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &actor, LibraryRole::Curator)?;
        let mut resource = Self::load_resource(&env, &tenant_id, &project_id, &resource_id)?;
        let actor_hash = Self::address_hash(&env, &actor);
        let id_bytes = resource_id.to_array();
        let policy_bytes = Self::policy_code(policy).to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ResourceAction::Policy).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &policy_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if resource.state == ResourceState::Deprecated {
            return Err(ContractError::InvalidTransition);
        }
        resource.policy = policy;
        resource.updated_at = env.ledger().timestamp();
        let key = Self::resource_key(&tenant_id, &project_id, &resource_id);
        Self::store_resource(&env, &key, &resource);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &resource_id);
        env.events().publish(
            (symbol_short!("RSCPOL"),),
            (
                resource_id,
                ResourceAction::Policy,
                config.state_version,
                Self::policy_code(policy),
            ),
        );
        Ok(resource.resource_id)
    }

    pub fn get_resource(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
    ) -> Result<ResourceRecord, ContractError> {
        Self::require_role(&env, &tenant_id, &project_id, &actor, LibraryRole::Member)?;
        Self::load_resource(&env, &tenant_id, &project_id, &resource_id)
    }

    pub fn get_version(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
        version: u32,
    ) -> Result<ResourceVersionRecord, ContractError> {
        Self::require_role(&env, &tenant_id, &project_id, &actor, LibraryRole::Member)?;
        let key = Self::resource_version_key(&tenant_id, &project_id, &resource_id, version);
        let record = env
            .storage()
            .persistent()
            .get::<_, ResourceVersionRecord>(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        if record.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        Ok(record)
    }

    pub fn resources_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_role(&env, &tenant_id, &project_id, &actor, LibraryRole::Member)?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let key = Self::resource_index_key(&tenant_id, &project_id);
        let index = Self::load_index(&env, &key);
        let mut page = Vec::new(&env);
        let mut position = offset;
        while position < index.len() && page.len() < limit {
            if let Some(resource_id) = index.get(position) {
                page.push_back(resource_id);
            }
            position += 1;
        }
        Self::extend(&env, &key);
        Ok(page)
    }

    pub fn can_access(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        resource_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let stored_role = Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            LibraryRole::Member,
        )?;
        let resource = Self::load_resource(&env, &tenant_id, &project_id, &resource_id)?;
        if resource.state != ResourceState::Approved {
            return Ok(false);
        }
        match resource.policy {
            AccessPolicy::OwnerOnly => Ok(resource.owner == actor),
            AccessPolicy::ProjectMembers => Ok(true),
            AccessPolicy::Reviewers => Ok(Self::role_allows(
                stored_role,
                LibraryRole::Reviewer,
            )),
        }
    }

    fn config_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> DataKey {
        DataKey::Config(tenant_id.clone(), project_id.clone())
    }

    fn role_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>, actor: &Address) -> DataKey {
        DataKey::Role(tenant_id.clone(), project_id.clone(), actor.clone())
    }

    fn resource_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        resource_id: &BytesN<32>,
    ) -> DataKey {
        DataKey::Resource(tenant_id.clone(), project_id.clone(), resource_id.clone())
    }

    fn resource_index_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> DataKey {
        DataKey::ResourceIndex(tenant_id.clone(), project_id.clone())
    }

    fn resource_version_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        resource_id: &BytesN<32>,
        version: u32,
    ) -> DataKey {
        DataKey::ResourceVersion(
            tenant_id.clone(),
            project_id.clone(),
            resource_id.clone(),
            version,
        )
    }

    fn request_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
    ) -> DataKey {
        DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone())
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_config(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<LibraryConfig, ContractError> {
        let key = Self::config_key(tenant_id, project_id);
        let config = env
            .storage()
            .persistent()
            .get::<_, LibraryConfig>(&key)
            .ok_or(ContractError::NotInitialized)?;
        Self::extend(env, &key);
        if config.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        Ok(config)
    }

    fn require_role(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        actor: &Address,
        minimum: LibraryRole,
    ) -> Result<LibraryRole, ContractError> {
        actor.require_auth();
        let key = Self::role_key(tenant_id, project_id, actor);
        let stored = env
            .storage()
            .persistent()
            .get::<_, LibraryRole>(&key)
            .ok_or(ContractError::NotAuthorized)?;
        Self::extend(env, &key);
        if !Self::role_allows(stored, minimum) {
            return Err(ContractError::NotAuthorized);
        }
        Ok(stored)
    }

    fn role_allows(stored: LibraryRole, minimum: LibraryRole) -> bool {
        match minimum {
            LibraryRole::Member => true,
            LibraryRole::Reviewer => {
                stored == LibraryRole::Reviewer
                    || stored == LibraryRole::Curator
                    || stored == LibraryRole::Admin
            }
            LibraryRole::Curator => stored == LibraryRole::Curator || stored == LibraryRole::Admin,
            LibraryRole::Admin => stored == LibraryRole::Admin,
        }
    }

    fn role_code(role: LibraryRole) -> u32 {
        match role {
            LibraryRole::Member => 1,
            LibraryRole::Reviewer => 2,
            LibraryRole::Curator => 3,
            LibraryRole::Admin => 4,
        }
    }

    fn kind_code(kind: ResourceKind) -> u32 {
        match kind {
            ResourceKind::File => 1,
            ResourceKind::Dataset => 2,
            ResourceKind::Link => 3,
            ResourceKind::Citation => 4,
        }
    }

    fn license_code(license: LicenseKind) -> u32 {
        match license {
            LicenseKind::Restricted => 1,
            LicenseKind::Internal => 2,
            LicenseKind::CcBy => 3,
            LicenseKind::Mit => 4,
            LicenseKind::Apache2 => 5,
        }
    }

    fn policy_code(policy: AccessPolicy) -> u32 {
        match policy {
            AccessPolicy::OwnerOnly => 1,
            AccessPolicy::ProjectMembers => 2,
            AccessPolicy::Reviewers => 3,
        }
    }

    fn action_code(action: ResourceAction) -> u32 {
        match action {
            ResourceAction::Grant => 1,
            ResourceAction::Propose => 2,
            ResourceAction::Approve => 3,
            ResourceAction::Deprecate => 4,
            ResourceAction::Version => 5,
            ResourceAction::Policy => 6,
        }
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn address_hash(env: &Env, address: &Address) -> BytesN<32> {
        env.crypto().sha256(&address.to_xdr(env))
    }

    fn hash_values(env: &Env, values: &[&[u8]]) -> BytesN<32> {
        let mut data = Bytes::new(env);
        for value in values {
            data.append(&Bytes::from_slice(env, *value));
        }
        env.crypto().sha256(&data)
    }

    fn ensure_version(config: &LibraryConfig, expected_version: u32) -> Result<(), ContractError> {
        if config.state_version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        Ok(())
    }

    fn bump_version(
        env: &Env,
        config_key: &DataKey,
        config: &mut LibraryConfig,
    ) -> Result<(), ContractError> {
        config.state_version = config
            .state_version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        env.storage().persistent().set(config_key, config);
        Self::extend(env, config_key);
        Ok(())
    }

    fn replay(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result: &BytesN<32>,
    ) -> Result<Option<BytesN<32>>, ContractError> {
        let key = Self::request_key(tenant_id, project_id, request_id);
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RequestRecord>(&key)
        {
            Self::extend(env, &key);
            if existing.payload_hash != *payload_hash || existing.result != *result {
                return Err(ContractError::RequestConflict);
            }
            return Ok(Some(existing.result));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result: &BytesN<32>,
    ) {
        let key = Self::request_key(tenant_id, project_id, request_id);
        let record = RequestRecord {
            payload_hash: payload_hash.clone(),
            result: result.clone(),
            recorded_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &record);
        Self::extend(env, &key);
    }

    fn load_resource(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        resource_id: &BytesN<32>,
    ) -> Result<ResourceRecord, ContractError> {
        let key = Self::resource_key(tenant_id, project_id, resource_id);
        let resource = env
            .storage()
            .persistent()
            .get::<_, ResourceRecord>(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(env, &key);
        if resource.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        if env.ledger().timestamp() > resource.expires_at {
            return Err(ContractError::Expired);
        }
        Ok(resource)
    }

    fn store_resource(env: &Env, key: &DataKey, resource: &ResourceRecord) {
        env.storage().persistent().set(key, resource);
        Self::extend(env, key);
    }

    fn load_index(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get::<_, Vec<BytesN<32>>>(key)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn store_index(env: &Env, key: &DataKey, index: &Vec<BytesN<32>>) {
        env.storage().persistent().set(key, index);
        Self::extend(env, key);
    }
}

#[cfg(test)]
mod tests;
