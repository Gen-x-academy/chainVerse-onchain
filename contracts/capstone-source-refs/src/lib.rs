#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_CONNECTIONS: u32 = 16;
const MAX_REFS: u32 = 64;
const MAX_HOOKS: u32 = 16;
const MAX_PAGE: u32 = 64;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    VersionConflict = 4,
    VersionOverflow = 5,
    RequestConflict = 6,
    NotFound = 7,
    AlreadyExists = 8,
    InvalidInput = 9,
    InvalidState = 10,
    ConnectionRevoked = 11,
    ProviderMismatch = 12,
    DeliveryReplay = 13,
    CollectionFull = 14,
    InvalidPage = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Maintainer = 2,
    Integrator = 3,
    Viewer = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    GitHub = 1,
    GitLab = 2,
    Bitbucket = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Active = 1,
    Revoked = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefKind {
    Branch = 1,
    Commit = 2,
    Release = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefState {
    Proposed = 1,
    Verified = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebhookState {
    Active = 1,
    Disabled = 2,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    RoleCount(BytesN<32>),
    Version(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Connection(BytesN<32>, BytesN<32>, BytesN<32>),
    ConnectionIndex(BytesN<32>, BytesN<32>),
    Ref(BytesN<32>, BytesN<32>, BytesN<32>),
    RefIndex(BytesN<32>, BytesN<32>),
    Hook(BytesN<32>, BytesN<32>, BytesN<32>),
    HookIndex(BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRecord {
    pub input_hash: BytesN<32>,
    pub result_id: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionRecord {
    pub id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub provider: Provider,
    pub credential_hash: BytesN<32>,
    pub scopes_mask: u32,
    pub state: ConnectionState,
    pub created_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRefRecord {
    pub id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub connection_id: BytesN<32>,
    pub provider: Provider,
    pub kind: RefKind,
    pub ref_hash: BytesN<32>,
    pub proof_hash: Option<BytesN<32>>,
    pub state: RefState,
    pub created_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebhookRecord {
    pub id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub connection_id: BytesN<32>,
    pub provider: Provider,
    pub hook_hash: BytesN<32>,
    pub last_delivery_seq: u32,
    pub state: WebhookState,
    pub created_at: u64,
    pub version: u32,
}

struct RequestToken {
    replay: Option<RequestRecord>,
    next_version: u32,
}

#[contract]
pub struct CapstoneSourceRefsContract;

#[contractimpl]
impl CapstoneSourceRefsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("SRINIT"),), (admin,));
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let role_key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        if !env.storage().persistent().has(&role_key) {
            let count_key = DataKey::RoleCount(tenant_id.clone());
            let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
            if count >= MAX_ROLES {
                return Err(ContractError::CollectionFull);
            }
            let next = count.checked_add(1).ok_or(ContractError::VersionOverflow)?;
            Self::write_u32(&env, &count_key, next);
        }
        Self::write_bool(&env, &role_key, true);
        env.events()
            .publish((symbol_short!("ROLEGR"),), (tenant_id, account, role as u32));
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::NotFound);
        }
        Self::write_bool(&env, &key, false);
        env.events()
            .publish((symbol_short!("ROLERV"),), (tenant_id, account, role as u32));
        Ok(())
    }

    pub fn connect_provider(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        connection_id: BytesN<32>,
        provider: Provider,
        credential_hash: BytesN<32>,
        scopes_mask: u32,
    ) -> Result<u32, ContractError> {
        Self::require_any_role(
            &env,
            &actor,
            &tenant_id,
            &[Role::Maintainer, Role::Integrator],
        )?;
        if scopes_mask == 0 || !Self::nonzero(&credential_hash) {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Connection(
            tenant_id.clone(),
            project_id.clone(),
            connection_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::ConnectionIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_ids(&env, &index_key);
        if index.len() >= MAX_CONNECTIONS {
            return Err(ContractError::CollectionFull);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&connection_id.to_array());
        extra.extend_from_array(&(provider as u32).to_be_bytes());
        extra.extend_from_array(&credential_hash.to_array());
        extra.extend_from_array(&scopes_mask.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP071_CONNECT",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let connection = ConnectionRecord {
            id: connection_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            provider,
            credential_hash,
            scopes_mask,
            state: ConnectionState::Active,
            created_at: env.ledger().timestamp(),
            version: token.next_version,
        };
        let mut updated_index = index;
        updated_index.push_back(connection_id.clone());
        Self::write_connection(&env, &key, &connection);
        Self::write_ids(&env, &index_key, &updated_index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &connection_id,
        );
        env.events().publish(
            (symbol_short!("SRCONN"),),
            (
                tenant_id,
                project_id,
                connection_id,
                provider as u32,
                scopes_mask,
            ),
        );
        Ok(token.next_version)
    }

    pub fn revoke_connection(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        connection_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Maintainer)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&connection_id.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP071_REVOKE",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let key = DataKey::Connection(
            tenant_id.clone(),
            project_id.clone(),
            connection_id.clone(),
        );
        let mut connection: ConnectionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if connection.state != ConnectionState::Active {
            return Err(ContractError::InvalidState);
        }
        connection.state = ConnectionState::Revoked;
        Self::write_connection(&env, &key, &connection);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &connection_id,
        );
        env.events().publish(
            (symbol_short!("SRRVK"),),
            (
                tenant_id,
                project_id,
                connection_id,
                ConnectionState::Revoked as u32,
            ),
        );
        Ok(token.next_version)
    }

    pub fn register_ref(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        ref_id: BytesN<32>,
        connection_id: BytesN<32>,
        provider: Provider,
        kind: RefKind,
        ref_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Maintainer)?;
        if !Self::nonzero(&ref_hash) {
            return Err(ContractError::InvalidInput);
        }
        let ref_key = DataKey::Ref(tenant_id.clone(), project_id.clone(), ref_id.clone());
        if env.storage().persistent().has(&ref_key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::RefIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_ids(&env, &index_key);
        if index.len() >= MAX_REFS {
            return Err(ContractError::CollectionFull);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&ref_id.to_array());
        extra.extend_from_array(&connection_id.to_array());
        extra.extend_from_array(&(provider as u32).to_be_bytes());
        extra.extend_from_array(&(kind as u32).to_be_bytes());
        extra.extend_from_array(&ref_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP071_REF",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let connection_key = DataKey::Connection(
            tenant_id.clone(),
            project_id.clone(),
            connection_id.clone(),
        );
        let connection: ConnectionRecord = env
            .storage()
            .persistent()
            .get(&connection_key)
            .ok_or(ContractError::NotFound)?;
        if connection.state != ConnectionState::Active {
            return Err(ContractError::ConnectionRevoked);
        }
        if connection.provider != provider {
            return Err(ContractError::ProviderMismatch);
        }
        let record = SourceRefRecord {
            id: ref_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            connection_id,
            provider,
            kind,
            ref_hash,
            proof_hash: None,
            state: RefState::Proposed,
            created_at: env.ledger().timestamp(),
            version: token.next_version,
        };
        let mut updated_index = index;
        updated_index.push_back(ref_id.clone());
        Self::write_ref(&env, &ref_key, &record);
        Self::write_ids(&env, &index_key, &updated_index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &ref_id,
        );
        env.events().publish(
            (symbol_short!("SRREF"),),
            (tenant_id, project_id, ref_id, provider as u32, kind as u32),
        );
        Ok(token.next_version)
    }

    pub fn verify_ref(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        ref_id: BytesN<32>,
        proof_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Integrator)?;
        if !Self::nonzero(&proof_hash) {
            return Err(ContractError::InvalidInput);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&ref_id.to_array());
        extra.extend_from_array(&proof_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP071_VERIFY",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let key = DataKey::Ref(tenant_id.clone(), project_id.clone(), ref_id.clone());
        let mut reference: SourceRefRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if reference.state != RefState::Proposed {
            return Err(ContractError::InvalidState);
        }
        let connection_key = DataKey::Connection(
            tenant_id.clone(),
            project_id.clone(),
            reference.connection_id.clone(),
        );
        let connection: ConnectionRecord = env
            .storage()
            .persistent()
            .get(&connection_key)
            .ok_or(ContractError::NotFound)?;
        if connection.state != ConnectionState::Active {
            return Err(ContractError::ConnectionRevoked);
        }
        reference.proof_hash = Some(proof_hash);
        reference.state = RefState::Verified;
        Self::write_ref(&env, &key, &reference);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &ref_id,
        );
        env.events().publish(
            (symbol_short!("SRVRFY"),),
            (
                tenant_id,
                project_id,
                ref_id,
                RefState::Verified as u32,
            ),
        );
        Ok(token.next_version)
    }

    pub fn record_webhook(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        hook_id: BytesN<32>,
        connection_id: BytesN<32>,
        hook_hash: BytesN<32>,
        delivery_seq: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Integrator)?;
        if !Self::nonzero(&hook_hash) {
            return Err(ContractError::InvalidInput);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&hook_id.to_array());
        extra.extend_from_array(&connection_id.to_array());
        extra.extend_from_array(&hook_hash.to_array());
        extra.extend_from_array(&delivery_seq.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP071_HOOK",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let connection_key = DataKey::Connection(
            tenant_id.clone(),
            project_id.clone(),
            connection_id.clone(),
        );
        let connection: ConnectionRecord = env
            .storage()
            .persistent()
            .get(&connection_key)
            .ok_or(ContractError::NotFound)?;
        if connection.state != ConnectionState::Active {
            return Err(ContractError::ConnectionRevoked);
        }
        let key = DataKey::Hook(tenant_id.clone(), project_id.clone(), hook_id.clone());
        let existing: Option<WebhookRecord> = env.storage().persistent().get(&key);
        if existing.is_none() && delivery_seq == 0 {
            return Err(ContractError::DeliveryReplay);
        }
        let mut hook = match existing {
            Some(record) => record,
            None => {
                let index_key = DataKey::HookIndex(tenant_id.clone(), project_id.clone());
                let index = Self::load_ids(&env, &index_key);
                if index.len() >= MAX_HOOKS {
                    return Err(ContractError::CollectionFull);
                }
                let mut updated_index = index;
                updated_index.push_back(hook_id.clone());
                Self::write_ids(&env, &index_key, &updated_index);
                WebhookRecord {
                    id: hook_id.clone(),
                    tenant_id: tenant_id.clone(),
                    project_id: project_id.clone(),
                    connection_id: connection_id.clone(),
                    provider: connection.provider,
                    hook_hash: hook_hash.clone(),
                    last_delivery_seq: 0,
                    state: WebhookState::Active,
                    created_at: env.ledger().timestamp(),
                    version: token.next_version,
                }
            }
        };
        if hook.connection_id != connection_id || hook.provider != connection.provider {
            return Err(ContractError::ProviderMismatch);
        }
        if delivery_seq <= hook.last_delivery_seq {
            return Err(ContractError::DeliveryReplay);
        }
        hook.hook_hash = hook_hash;
        hook.last_delivery_seq = delivery_seq;
        Self::write_hook(&env, &key, &hook);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &hook_id,
        );
        env.events().publish(
            (symbol_short!("SRHOOK"),),
            (tenant_id, project_id, hook_id, delivery_seq),
        );
        Ok(token.next_version)
    }

    pub fn get_connection(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        connection_id: BytesN<32>,
    ) -> Result<ConnectionRecord, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Connection(tenant_id, project_id, connection_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    pub fn get_ref(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        ref_id: BytesN<32>,
    ) -> Result<SourceRefRecord, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Ref(tenant_id, project_id, ref_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    pub fn get_webhook(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        hook_id: BytesN<32>,
    ) -> Result<WebhookRecord, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Hook(tenant_id, project_id, hook_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(record)
    }

    pub fn list_refs(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
        provider_filter: Option<Provider>,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let key = DataKey::RefIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_ids(&env, &key);
        if from > index.len() {
            return Err(ContractError::InvalidPage);
        }
        let end = from.checked_add(limit).ok_or(ContractError::VersionOverflow)?;
        let last = if end > index.len() { index.len() } else { end };
        let mut page = Vec::new(&env);
        let mut cursor = from;
        while cursor < last {
            if let Some(id) = index.get(cursor) {
                let ref_key = DataKey::Ref(tenant_id.clone(), project_id.clone(), id.clone());
                let reference: SourceRefRecord = env
                    .storage()
                    .persistent()
                    .get(&ref_key)
                    .ok_or(ContractError::NotFound)?;
                env.storage()
                    .persistent()
                    .extend_ttl(&ref_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
                let include = match provider_filter.as_ref() {
                    Some(provider) => reference.provider == *provider,
                    None => true,
                };
                if include {
                    page.push_back(id);
                }
            }
            cursor += 1;
        }
        Ok(page)
    }

    pub fn project_version(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::load_version(&env, &tenant_id, &project_id)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn has_role(env: &Env, tenant_id: &BytesN<32>, account: &Address, role: Role) -> bool {
        let key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        let active = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&key)
            .unwrap_or(false);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        active
    }

    fn require_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        actor.require_auth();
        if Self::has_role(env, tenant_id, actor, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn require_any_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        roles: &[Role],
    ) -> Result<(), ContractError> {
        actor.require_auth();
        for role in roles.iter() {
            if Self::has_role(env, tenant_id, actor, *role) {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    fn require_viewer(
        env: &Env,
        viewer: &Address,
        tenant_id: &BytesN<32>,
    ) -> Result<(), ContractError> {
        viewer.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *viewer == admin {
            return Ok(());
        }
        let roles = [Role::Maintainer, Role::Integrator, Role::Viewer];
        for role in roles.iter() {
            if Self::has_role(env, tenant_id, viewer, *role) {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    fn begin_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        expected_version: u32,
        input_hash: &BytesN<32>,
    ) -> Result<RequestToken, ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(record) = env.storage().persistent().get::<DataKey, RequestRecord>(&key) {
            if &record.input_hash != input_hash {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            return Ok(RequestToken {
                replay: Some(record),
                next_version: 0,
            });
        }
        let current = Self::load_version(env, tenant_id, project_id);
        if expected_version != current {
            return Err(ContractError::VersionConflict);
        }
        let next_version = current
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        Ok(RequestToken {
            replay: None,
            next_version,
        })
    }

    fn finish_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        token: &RequestToken,
        input_hash: &BytesN<32>,
        result_id: &BytesN<32>,
    ) {
        let request_key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let record = RequestRecord {
            input_hash: input_hash.clone(),
            result_id: result_id.clone(),
            version: token.next_version,
        };
        Self::write_record(env, &request_key, &record);
        let version_key = DataKey::Version(tenant_id.clone(), project_id.clone());
        Self::write_u32(env, &version_key, token.next_version);
    }

    fn touch_request(env: &Env, tenant_id: &BytesN<32>, project_id: &BytesN<32>, request_id: &BytesN<32>) {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_version(env: &Env, tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> u32 {
        let key = DataKey::Version(tenant_id.clone(), project_id.clone());
        let version: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        version
    }

    fn load_ids(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        let ids = env
            .storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env));
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        ids
    }

    fn hash_input(
        env: &Env,
        domain: &[u8],
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        expected_version: u32,
        extra: &Bytes,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, domain));
        input.extend_from_array(&tenant_id.to_array());
        input.extend_from_array(&project_id.to_array());
        input.extend_from_array(&request_id.to_array());
        input.extend_from_array(&expected_version.to_be_bytes());
        input.append(extra);
        env.crypto().sha256(&input)
    }

    fn nonzero(hash: &BytesN<32>) -> bool {
        hash.to_array().iter().any(|byte| *byte != 0)
    }

    fn write_bool(env: &Env, key: &DataKey, value: bool) {
        env.storage().persistent().set(key, &value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_u32(env: &Env, key: &DataKey, value: u32) {
        env.storage().persistent().set(key, &value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_ids(env: &Env, key: &DataKey, value: &Vec<BytesN<32>>) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_record(env: &Env, key: &DataKey, value: &RequestRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_connection(env: &Env, key: &DataKey, value: &ConnectionRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_ref(env: &Env, key: &DataKey, value: &SourceRefRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_hook(env: &Env, key: &DataKey, value: &WebhookRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
