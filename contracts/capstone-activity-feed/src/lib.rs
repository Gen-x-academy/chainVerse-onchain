#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_FEED: u32 = 64;
const MAX_PAGE: u32 = 64;
const DEFAULT_RETENTION: u64 = 2_592_000;
const MAX_RETENTION: u64 = 31_536_000;

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
    InvalidPage = 10,
    InvalidSequence = 11,
    InvalidVisibility = 12,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Coordinator = 2,
    Member = 3,
    Observer = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedKind {
    Decision = 1,
    SourceRef = 2,
    Review = 3,
    Milestone = 4,
    Assignment = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Public = 1,
    Members = 2,
    Coordinators = 3,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    RoleCount(BytesN<32>),
    Version(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Index(BytesN<32>, BytesN<32>),
    Entry(BytesN<32>, BytesN<32>, u32),
    Read(BytesN<32>, BytesN<32>, Address),
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
pub struct FeedIndex {
    pub head_seq: u32,
    pub head_hash: BytesN<32>,
    pub retention_seconds: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedEntry {
    pub seq: u32,
    pub id: BytesN<32>,
    pub kind: FeedKind,
    pub actor_role: Role,
    pub visibility: Visibility,
    pub ref_hash: BytesN<32>,
    pub deep_link_hash: BytesN<32>,
    pub at: u64,
    pub prev_hash: BytesN<32>,
    pub entry_hash: BytesN<32>,
}

struct RequestToken {
    replay: Option<RequestRecord>,
    next_version: u32,
}

#[contract]
pub struct CapstoneActivityFeedContract;

#[contractimpl]
impl CapstoneActivityFeedContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("CFAINIT"),), (admin,));
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
                return Err(ContractError::InvalidInput);
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

    pub fn set_retention(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        retention_seconds: u64,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        if retention_seconds == 0 || retention_seconds > MAX_RETENTION {
            return Err(ContractError::InvalidInput);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&retention_seconds.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP072_RETENTION",
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
        let key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let mut index = Self::load_index(&env, &key);
        index.retention_seconds = retention_seconds;
        index.version = token.next_version;
        Self::write_index(&env, &key, &index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &BytesN::from_array(&env, &[0u8; 32]),
        );
        env.events().publish(
            (symbol_short!("CFARET"),),
            (tenant_id, project_id, retention_seconds),
        );
        Ok(token.next_version)
    }

    pub fn append_entry(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        entry_id: BytesN<32>,
        kind: FeedKind,
        ref_hash: BytesN<32>,
        deep_link_hash: BytesN<32>,
        visibility: Visibility,
    ) -> Result<u32, ContractError> {
        Self::require_any_role(
            &env,
            &actor,
            &tenant_id,
            &[Role::Coordinator, Role::Member],
        )?;
        if !Self::nonzero(&ref_hash) || !Self::nonzero(&deep_link_hash) {
            return Err(ContractError::InvalidInput);
        }
        let actor_role = if Self::has_role(&env, &tenant_id, &actor, Role::Coordinator) {
            Role::Coordinator
        } else {
            Role::Member
        };
        if visibility == Visibility::Coordinators && actor_role != Role::Coordinator {
            return Err(ContractError::InvalidVisibility);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&entry_id.to_array());
        extra.extend_from_array(&(kind as u32).to_be_bytes());
        extra.extend_from_array(&ref_hash.to_array());
        extra.extend_from_array(&deep_link_hash.to_array());
        extra.extend_from_array(&(visibility as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP072_APPEND",
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
        let index_key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let mut index = Self::load_index(&env, &index_key);
        let oldest = Self::oldest_seq(index.head_seq);
        let mut cursor = oldest;
        while cursor <= index.head_seq {
            let slot = cursor % MAX_FEED;
            if let Some(existing) = env.storage().persistent().get::<DataKey, FeedEntry>(
                &DataKey::Entry(tenant_id.clone(), project_id.clone(), slot),
            ) {
                if existing.id == entry_id {
                    return Err(ContractError::AlreadyExists);
                }
            }
            if cursor == index.head_seq {
                break;
            }
            cursor += 1;
        }
        let seq = index.head_seq
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        let mut hash_extra = Bytes::new(&env);
        hash_extra.extend_from_array(&seq.to_be_bytes());
        hash_extra.extend_from_array(&entry_id.to_array());
        hash_extra.extend_from_array(&(kind as u32).to_be_bytes());
        hash_extra.extend_from_array(&(actor_role as u32).to_be_bytes());
        hash_extra.extend_from_array(&(visibility as u32).to_be_bytes());
        hash_extra.extend_from_array(&ref_hash.to_array());
        hash_extra.extend_from_array(&deep_link_hash.to_array());
        hash_extra.extend_from_array(&now.to_be_bytes());
        hash_extra.extend_from_array(&index.head_hash.to_array());
        let entry_hash = Self::hash_input(
            &env,
            b"CAP072_HASH",
            &tenant_id,
            &project_id,
            &entry_id,
            seq,
            &hash_extra,
        );
        let entry = FeedEntry {
            seq,
            id: entry_id.clone(),
            kind,
            actor_role,
            visibility,
            ref_hash,
            deep_link_hash,
            at: now,
            prev_hash: index.head_hash.clone(),
            entry_hash: entry_hash.clone(),
        };
        let slot = seq % MAX_FEED;
        let entry_key = DataKey::Entry(tenant_id.clone(), project_id.clone(), slot);
        Self::write_entry(&env, &entry_key, &entry);
        index.head_seq = seq;
        index.head_hash = entry_hash;
        index.version = token.next_version;
        Self::write_index(&env, &index_key, &index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &entry_id,
        );
        env.events().publish(
            (symbol_short!("CFAENT"),),
            (
                tenant_id,
                project_id,
                seq,
                kind as u32,
                visibility as u32,
            ),
        );
        Ok(token.next_version)
    }

    pub fn mark_read(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        seq: u32,
    ) -> Result<u32, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&seq.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP072_READ",
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
        let index_key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let index = Self::load_index(&env, &index_key);
        if seq == 0 || seq > index.head_seq || seq < Self::oldest_seq(index.head_seq) {
            return Err(ContractError::InvalidSequence);
        }
        let key = DataKey::Read(tenant_id.clone(), project_id.clone(), viewer.clone());
        let current: u64 = env.storage().persistent().get(&key).unwrap_or(0);
        if (seq as u64) < current {
            return Err(ContractError::InvalidSequence);
        }
        Self::write_u64(&env, &key, seq as u64);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &BytesN::from_array(&env, &[0u8; 32]),
        );
        env.events()
            .publish((symbol_short!("CFAREAD"),), (tenant_id, project_id, seq));
        Ok(token.next_version)
    }

    pub fn get_entry(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        seq: u32,
    ) -> Result<FeedEntry, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let index_key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let index = Self::load_index(&env, &index_key);
        if seq < Self::oldest_seq(index.head_seq) || seq > index.head_seq {
            return Err(ContractError::NotFound);
        }
        let key = DataKey::Entry(tenant_id.clone(), project_id.clone(), seq % MAX_FEED);
        let entry: FeedEntry = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if !Self::visibility_allows(&env, &viewer, &tenant_id, entry.visibility) {
            return Err(ContractError::Unauthorized);
        }
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(entry)
    }

    pub fn list_entries(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Result<Vec<FeedEntry>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let index_key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let index = Self::load_index(&env, &index_key);
        let oldest = Self::oldest_seq(index.head_seq);
        let start = if from < oldest { oldest } else { from };
        if start > index.head_seq {
            return Ok(Vec::new(&env));
        }
        let end = start.checked_add(limit).ok_or(ContractError::VersionOverflow)?;
        let last = if end > index.head_seq {
            index.head_seq
        } else {
            end
        };
        let cutoff = env
            .ledger()
            .timestamp()
            .checked_sub(index.retention_seconds)
            .unwrap_or(0);
        let mut page = Vec::new(&env);
        let mut cursor = start;
        while cursor <= last {
            let key = DataKey::Entry(tenant_id.clone(), project_id.clone(), cursor % MAX_FEED);
            if let Some(entry) = env.storage().persistent().get::<DataKey, FeedEntry>(&key) {
                env.storage()
                    .persistent()
                    .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
                if entry.at >= cutoff
                    && Self::visibility_allows(&env, &viewer, &tenant_id, entry.visibility)
                {
                    page.push_back(entry);
                }
            }
            if cursor == last {
                break;
            }
            cursor += 1;
        }
        Ok(page)
    }

    pub fn head_hash(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Index(tenant_id, project_id);
        let index = Self::load_index(&env, &key);
        Ok(index.head_hash)
    }

    pub fn read_cursor(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<u64, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Read(tenant_id, project_id, viewer);
        let cursor: u64 = env.storage().persistent().get(&key).unwrap_or(0);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        Ok(cursor)
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

    fn is_admin(env: &Env, account: &Address) -> bool {
        let stored: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        match stored {
            Some(admin) => *account == admin,
            None => false,
        }
    }

    fn require_viewer(
        env: &Env,
        viewer: &Address,
        tenant_id: &BytesN<32>,
    ) -> Result<(), ContractError> {
        viewer.require_auth();
        if Self::is_admin(env, viewer) {
            return Ok(());
        }
        let roles = [Role::Coordinator, Role::Member, Role::Observer];
        for role in roles.iter() {
            if Self::has_role(env, tenant_id, viewer, *role) {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    fn visibility_allows(
        env: &Env,
        viewer: &Address,
        tenant_id: &BytesN<32>,
        visibility: Visibility,
    ) -> bool {
        if Self::is_admin(env, viewer) || visibility == Visibility::Public {
            return true;
        }
        if Self::has_role(env, tenant_id, viewer, Role::Coordinator) {
            return true;
        }
        visibility == Visibility::Members
            && Self::has_role(env, tenant_id, viewer, Role::Member)
    }

    fn oldest_seq(head_seq: u32) -> u32 {
        if head_seq > MAX_FEED {
            head_seq - MAX_FEED + 1
        } else {
            1
        }
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

    fn load_index(env: &Env, key: &DataKey) -> FeedIndex {
        let index: FeedIndex = env.storage().persistent().get(key).unwrap_or(FeedIndex {
            head_seq: 0,
            head_hash: BytesN::from_array(env, &[0u8; 32]),
            retention_seconds: DEFAULT_RETENTION,
            version: 0,
        });
        if env.storage().persistent().has(key) {
            env.storage()
                .persistent()
                .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        index
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

    fn write_u64(env: &Env, key: &DataKey, value: u64) {
        env.storage().persistent().set(key, &value);
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

    fn write_index(env: &Env, key: &DataKey, value: &FeedIndex) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_entry(env: &Env, key: &DataKey, value: &FeedEntry) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
