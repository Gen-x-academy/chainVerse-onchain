#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

pub const MAX_MEMBERS: u32 = 32;
pub const MAX_THREADS: u32 = 32;
pub const MAX_MESSAGES: u32 = 64;
pub const MAX_MENTIONS: u32 = 8;
pub const MAX_ATTACHMENTS: u32 = 4;
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
    InvalidThreadState = 8,
    ThreadsFull = 9,
    MessagesFull = 10,
    MembersFull = 11,
    Expired = 12,
    InvalidRetention = 13,
    TimestampOverflow = 14,
    VersionOverflow = 15,
    InvalidPage = 16,
    UnsupportedSchema = 17,
    InvalidParent = 18,
    TooManyMentions = 19,
    TooManyAttachments = 20,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Config(BytesN<32>, BytesN<32>),
    Role(BytesN<32>, BytesN<32>, Address),
    Thread(BytesN<32>, BytesN<32>, BytesN<32>),
    ThreadIndex(BytesN<32>, BytesN<32>),
    Message(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    MessageIndex(BytesN<32>, BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadRole {
    Participant,
    Moderator,
    Admin,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadState {
    Open,
    Resolved,
    Locked,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    Unresolved,
    Resolved,
    ResolvedWithHelp,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageModeration {
    Visible,
    Hidden,
    Locked,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadAction {
    Grant,
    Open,
    Post,
    Resolve,
    Reopen,
    Moderate,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModerationAction {
    Hide,
    Restore,
    Lock,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadConfig {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub admin: Address,
    pub state_version: u32,
    pub member_count: u32,
    pub thread_count: u32,
    pub created_at: u64,
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadDraft {
    pub thread_id: BytesN<32>,
    pub topic_hash: BytesN<32>,
    pub context_hash: BytesN<32>,
    pub retention_seconds: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDraft {
    pub thread_id: BytesN<32>,
    pub message_id: BytesN<32>,
    pub parent_id: BytesN<32>,
    pub content_hash: BytesN<32>,
    pub mention_digest: BytesN<32>,
    pub mention_count: u32,
    pub attachments: Vec<BytesN<32>>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadRecord {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub thread_id: BytesN<32>,
    pub owner: Address,
    pub state: ThreadState,
    pub resolution: Resolution,
    pub resolution_hash: BytesN<32>,
    pub message_count: u32,
    pub sequence: u32,
    pub expires_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub schema_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageRecord {
    pub thread_id: BytesN<32>,
    pub message_id: BytesN<32>,
    pub sequence: u32,
    pub parent_id: BytesN<32>,
    pub content_hash: BytesN<32>,
    pub mention_digest: BytesN<32>,
    pub mention_count: u32,
    pub attachments: Vec<BytesN<32>>,
    pub moderation: MessageModeration,
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
pub struct CapstoneDiscussionThreadsContract;

#[contractimpl]
impl CapstoneDiscussionThreadsContract {
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
        let config = ThreadConfig {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            admin: admin.clone(),
            state_version: 0,
            member_count: 1,
            thread_count: 0,
            created_at: env.ledger().timestamp(),
            schema_version: SCHEMA_VERSION,
        };
        env.storage().persistent().set(&config_key, &config);
        Self::extend(&env, &config_key);
        let role_key = Self::role_key(&tenant_id, &project_id, &admin);
        env.storage().persistent().set(&role_key, &ThreadRole::Admin);
        Self::extend(&env, &role_key);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        member: Address,
        role: ThreadRole,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(&env, &tenant_id, &project_id, &admin, ThreadRole::Admin)?;
        let admin_hash = Self::address_hash(&env, &admin);
        let member_hash = Self::address_hash(&env, &member);
        let role_bytes = Self::role_code(role).to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ThreadAction::Grant).to_be_bytes();
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
            (symbol_short!("THRROLE"),),
            (
                member_hash,
                ThreadAction::Grant,
                config.state_version,
                Self::role_code(role),
            ),
        );
        Ok(member_hash)
    }

    pub fn open_thread(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        draft: ThreadDraft,
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
            ThreadRole::Participant,
        )?;
        let actor_hash = Self::address_hash(&env, &actor);
        let id_bytes = draft.thread_id.to_array();
        let topic_bytes = draft.topic_hash.to_array();
        let context_bytes = draft.context_hash.to_array();
        let retention_bytes = draft.retention_seconds.to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ThreadAction::Open).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &topic_bytes,
                &context_bytes,
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
            &draft.thread_id,
        )? {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if draft.retention_seconds == 0 || draft.retention_seconds > MAX_RETENTION {
            return Err(ContractError::InvalidRetention);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(draft.retention_seconds)
            .ok_or(ContractError::TimestampOverflow)?;
        let key = Self::thread_key(&tenant_id, &project_id, &draft.thread_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = Self::thread_index_key(&tenant_id, &project_id);
        let mut index = Self::load_index(&env, &index_key);
        if index.len() >= MAX_THREADS {
            return Err(ContractError::ThreadsFull);
        }
        index.push_back(draft.thread_id.clone());
        Self::store_index(&env, &index_key, &index);
        let thread = ThreadRecord {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            thread_id: draft.thread_id.clone(),
            owner: actor,
            state: ThreadState::Open,
            resolution: Resolution::Unresolved,
            resolution_hash: Self::zero_hash(&env),
            message_count: 0,
            sequence: 0,
            expires_at,
            created_at: now,
            updated_at: now,
            schema_version: SCHEMA_VERSION,
        };
        Self::store_thread(&env, &key, &thread);
        let message_index_key = Self::message_index_key(&tenant_id, &project_id, &draft.thread_id);
        env.storage()
            .persistent()
            .set(&message_index_key, &Vec::<BytesN<32>>::new(&env));
        Self::extend(&env, &message_index_key);
        let thread_count = config
            .thread_count
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        config.thread_count = thread_count;
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &draft.thread_id,
        );
        env.events().publish(
            (symbol_short!("THRNEW"),),
            (
                draft.thread_id,
                ThreadAction::Open,
                config.state_version,
                draft.topic_hash,
            ),
        );
        Ok(thread.thread_id)
    }

    pub fn post_message(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        draft: MessageDraft,
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
            ThreadRole::Participant,
        )?;
        let mut thread = Self::load_thread(&env, &tenant_id, &project_id, &draft.thread_id)?;
        if draft.parent_id != Self::zero_hash(&env) {
            let parent_key = Self::message_key(
                &tenant_id,
                &project_id,
                &draft.thread_id,
                &draft.parent_id,
            );
            if !env.storage().persistent().has(&parent_key) {
                return Err(ContractError::InvalidParent);
            }
            Self::extend(&env, &parent_key);
        }
        let actor_hash = Self::address_hash(&env, &actor);
        let thread_bytes = draft.thread_id.to_array();
        let id_bytes = draft.message_id.to_array();
        let parent_bytes = draft.parent_id.to_array();
        let content_bytes = draft.content_hash.to_array();
        let mention_bytes = draft.mention_digest.to_array();
        let attachments_hash = Self::attachments_hash(&env, &draft.attachments);
        let mention_count_bytes = draft.mention_count.to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ThreadAction::Post).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &thread_bytes,
                &id_bytes,
                &parent_bytes,
                &content_bytes,
                &mention_bytes,
                attachments_hash.as_slice(),
                &mention_count_bytes,
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
            &draft.message_id,
        )? {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if thread.state != ThreadState::Open {
            return Err(ContractError::InvalidThreadState);
        }
        if draft.mention_count > MAX_MENTIONS {
            return Err(ContractError::TooManyMentions);
        }
        if draft.attachments.len() > MAX_ATTACHMENTS {
            return Err(ContractError::TooManyAttachments);
        }
        let message_key = Self::message_key(
            &tenant_id,
            &project_id,
            &draft.thread_id,
            &draft.message_id,
        );
        if env.storage().persistent().has(&message_key) {
            return Err(ContractError::AlreadyExists);
        }
        if thread.message_count >= MAX_MESSAGES {
            return Err(ContractError::MessagesFull);
        }
        let message_count = thread
            .message_count
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let sequence = thread
            .sequence
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let message = MessageRecord {
            thread_id: draft.thread_id.clone(),
            message_id: draft.message_id.clone(),
            sequence,
            parent_id: draft.parent_id.clone(),
            content_hash: draft.content_hash.clone(),
            mention_digest: draft.mention_digest,
            mention_count: draft.mention_count,
            attachments: draft.attachments,
            moderation: MessageModeration::Visible,
            recorded_at: env.ledger().timestamp(),
            schema_version: SCHEMA_VERSION,
        };
        env.storage().persistent().set(&message_key, &message);
        Self::extend(&env, &message_key);
        let index_key = Self::message_index_key(&tenant_id, &project_id, &draft.thread_id);
        let mut index = Self::load_index(&env, &index_key);
        index.push_back(draft.message_id.clone());
        Self::store_index(&env, &index_key, &index);
        thread.message_count = message_count;
        thread.sequence = sequence;
        thread.updated_at = env.ledger().timestamp();
        let thread_key = Self::thread_key(&tenant_id, &project_id, &draft.thread_id);
        Self::store_thread(&env, &thread_key, &thread);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload,
            &draft.message_id,
        );
        env.events().publish(
            (symbol_short!("THRMSG"),),
            (
                draft.message_id,
                ThreadAction::Post,
                sequence,
                draft.content_hash,
            ),
        );
        Ok(draft.message_id)
    }

    pub fn resolve_thread(
        env: Env,
        moderator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
        resolution: Resolution,
        resolution_hash: BytesN<32>,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &moderator,
            ThreadRole::Moderator,
        )?;
        let mut thread = Self::load_thread(&env, &tenant_id, &project_id, &thread_id)?;
        let actor_hash = Self::address_hash(&env, &moderator);
        let id_bytes = thread_id.to_array();
        let resolution_bytes = Self::resolution_code(resolution).to_be_bytes();
        let hash_bytes = resolution_hash.to_array();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ThreadAction::Resolve).to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &resolution_bytes,
                &hash_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &thread_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if thread.state != ThreadState::Open || resolution == Resolution::Unresolved {
            return Err(ContractError::InvalidThreadState);
        }
        thread.state = ThreadState::Resolved;
        thread.resolution = resolution;
        thread.resolution_hash = resolution_hash.clone();
        thread.updated_at = env.ledger().timestamp();
        let key = Self::thread_key(&tenant_id, &project_id, &thread_id);
        Self::store_thread(&env, &key, &thread);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &thread_id);
        env.events().publish(
            (symbol_short!("THRRES"),),
            (
                thread_id,
                ThreadAction::Resolve,
                config.state_version,
                resolution_hash,
            ),
        );
        Ok(thread.thread_id)
    }

    pub fn reopen_thread(
        env: Env,
        moderator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &moderator,
            ThreadRole::Moderator,
        )?;
        let mut thread = Self::load_thread(&env, &tenant_id, &project_id, &thread_id)?;
        let actor_hash = Self::address_hash(&env, &moderator);
        let id_bytes = thread_id.to_array();
        let version_bytes = expected_version.to_be_bytes();
        let action_bytes = Self::action_code(ThreadAction::Reopen).to_be_bytes();
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
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &thread_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        if thread.state != ThreadState::Resolved {
            return Err(ContractError::InvalidThreadState);
        }
        thread.state = ThreadState::Open;
        thread.resolution = Resolution::Unresolved;
        thread.resolution_hash = Self::zero_hash(&env);
        thread.updated_at = env.ledger().timestamp();
        let key = Self::thread_key(&tenant_id, &project_id, &thread_id);
        Self::store_thread(&env, &key, &thread);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &thread_id);
        env.events().publish(
            (symbol_short!("THROPN"),),
            (
                thread_id,
                ThreadAction::Reopen,
                config.state_version,
                Self::resolution_code(Resolution::Unresolved),
            ),
        );
        Ok(thread.thread_id)
    }

    pub fn moderate_message(
        env: Env,
        moderator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
        message_id: BytesN<32>,
        action: ModerationAction,
        intent_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<BytesN<32>, ContractError> {
        let config_key = Self::config_key(&tenant_id, &project_id);
        let mut config = Self::load_config(&env, &tenant_id, &project_id)?;
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &moderator,
            ThreadRole::Moderator,
        )?;
        let mut message = Self::load_message(&env, &tenant_id, &project_id, &thread_id, &message_id)?;
        let actor_hash = Self::address_hash(&env, &moderator);
        let id_bytes = message_id.to_array();
        let action_bytes = Self::action_code(ThreadAction::Moderate).to_be_bytes();
        let moderation_bytes = Self::moderation_code(action).to_be_bytes();
        let version_bytes = expected_version.to_be_bytes();
        let payload = Self::hash_values(
            &env,
            &[
                actor_hash.as_slice(),
                &id_bytes,
                &moderation_bytes,
                &version_bytes,
                &action_bytes,
            ],
        );
        if let Some(existing) =
            Self::replay(&env, &tenant_id, &project_id, &intent_id, &payload, &message_id)?
        {
            return Ok(existing);
        }
        Self::ensure_version(&config, expected_version)?;
        message.moderation = match action {
            ModerationAction::Hide => MessageModeration::Hidden,
            ModerationAction::Restore => MessageModeration::Visible,
            ModerationAction::Lock => MessageModeration::Locked,
        };
        let key = Self::message_key(&tenant_id, &project_id, &thread_id, &message_id);
        env.storage().persistent().set(&key, &message);
        Self::extend(&env, &key);
        Self::bump_version(&env, &config_key, &mut config)?;
        Self::write_request(&env, &tenant_id, &project_id, &intent_id, &payload, &message_id);
        env.events().publish(
            (symbol_short!("THRMOD"),),
            (
                message_id,
                ThreadAction::Moderate,
                message.sequence,
                Self::moderation_code(action),
            ),
        );
        Ok(message.message_id)
    }

    pub fn get_thread(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
    ) -> Result<ThreadRecord, ContractError> {
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            ThreadRole::Participant,
        )?;
        Self::load_thread(&env, &tenant_id, &project_id, &thread_id)
    }

    pub fn get_message(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
        message_id: BytesN<32>,
    ) -> Result<MessageRecord, ContractError> {
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            ThreadRole::Participant,
        )?;
        Self::load_message(&env, &tenant_id, &project_id, &thread_id, &message_id)
    }

    pub fn threads_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            ThreadRole::Participant,
        )?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let key = Self::thread_index_key(&tenant_id, &project_id);
        let index = Self::load_index(&env, &key);
        let mut page = Vec::new(&env);
        let mut position = offset;
        while position < index.len() && page.len() < limit {
            if let Some(thread_id) = index.get(position) {
                page.push_back(thread_id);
            }
            position += 1;
        }
        Self::extend(&env, &key);
        Ok(page)
    }

    pub fn messages_page(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        thread_id: BytesN<32>,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_role(
            &env,
            &tenant_id,
            &project_id,
            &actor,
            ThreadRole::Participant,
        )?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        Self::load_thread(&env, &tenant_id, &project_id, &thread_id)?;
        let key = Self::message_index_key(&tenant_id, &project_id, &thread_id);
        let index = Self::load_index(&env, &key);
        let mut page = Vec::new(&env);
        let mut position = offset;
        while position < index.len() && page.len() < limit {
            if let Some(message_id) = index.get(position) {
                page.push_back(message_id);
            }
            position += 1;
        }
        Self::extend(&env, &key);
        Ok(page)
    }

    fn config_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> DataKey {
        DataKey::Config(tenant_id.clone(), project_id.clone())
    }

    fn role_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>, actor: &Address) -> DataKey {
        DataKey::Role(tenant_id.clone(), project_id.clone(), actor.clone())
    }

    fn thread_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        thread_id: &BytesN<32>,
    ) -> DataKey {
        DataKey::Thread(tenant_id.clone(), project_id.clone(), thread_id.clone())
    }

    fn thread_index_key(tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> DataKey {
        DataKey::ThreadIndex(tenant_id.clone(), project_id.clone())
    }

    fn message_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        thread_id: &BytesN<32>,
        message_id: &BytesN<32>,
    ) -> DataKey {
        DataKey::Message(
            tenant_id.clone(),
            project_id.clone(),
            thread_id.clone(),
            message_id.clone(),
        )
    }

    fn message_index_key(
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        thread_id: &BytesN<32>,
    ) -> DataKey {
        DataKey::MessageIndex(tenant_id.clone(), project_id.clone(), thread_id.clone())
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
    ) -> Result<ThreadConfig, ContractError> {
        let key = Self::config_key(tenant_id, project_id);
        let config = env
            .storage()
            .persistent()
            .get::<_, ThreadConfig>(&key)
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
        minimum: ThreadRole,
    ) -> Result<ThreadRole, ContractError> {
        actor.require_auth();
        let key = Self::role_key(tenant_id, project_id, actor);
        let stored = env
            .storage()
            .persistent()
            .get::<_, ThreadRole>(&key)
            .ok_or(ContractError::NotAuthorized)?;
        Self::extend(env, &key);
        if !Self::role_allows(stored, minimum) {
            return Err(ContractError::NotAuthorized);
        }
        Ok(stored)
    }

    fn role_allows(stored: ThreadRole, minimum: ThreadRole) -> bool {
        match minimum {
            ThreadRole::Participant => true,
            ThreadRole::Moderator => stored == ThreadRole::Moderator || stored == ThreadRole::Admin,
            ThreadRole::Admin => stored == ThreadRole::Admin,
        }
    }

    fn role_code(role: ThreadRole) -> u32 {
        match role {
            ThreadRole::Participant => 1,
            ThreadRole::Moderator => 2,
            ThreadRole::Admin => 3,
        }
    }

    fn action_code(action: ThreadAction) -> u32 {
        match action {
            ThreadAction::Grant => 1,
            ThreadAction::Open => 2,
            ThreadAction::Post => 3,
            ThreadAction::Resolve => 4,
            ThreadAction::Reopen => 5,
            ThreadAction::Moderate => 6,
        }
    }

    fn resolution_code(resolution: Resolution) -> u32 {
        match resolution {
            Resolution::Unresolved => 0,
            Resolution::Resolved => 1,
            Resolution::ResolvedWithHelp => 2,
        }
    }

    fn moderation_code(action: ModerationAction) -> u32 {
        match action {
            ModerationAction::Hide => 1,
            ModerationAction::Restore => 2,
            ModerationAction::Lock => 3,
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

    fn attachments_hash(env: &Env, attachments: &Vec<BytesN<32>>) -> BytesN<32> {
        let mut data = Bytes::new(env);
        let mut position = 0;
        while position < attachments.len() {
            if let Some(attachment) = attachments.get(position) {
                let bytes = attachment.to_array();
                data.append(&Bytes::from_slice(env, &bytes));
            }
            position += 1;
        }
        env.crypto().sha256(&data)
    }

    fn ensure_version(config: &ThreadConfig, expected_version: u32) -> Result<(), ContractError> {
        if config.state_version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        Ok(())
    }

    fn bump_version(
        env: &Env,
        config_key: &DataKey,
        config: &mut ThreadConfig,
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

    fn load_thread(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        thread_id: &BytesN<32>,
    ) -> Result<ThreadRecord, ContractError> {
        let key = Self::thread_key(tenant_id, project_id, thread_id);
        let thread = env
            .storage()
            .persistent()
            .get::<_, ThreadRecord>(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(env, &key);
        if thread.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        if env.ledger().timestamp() > thread.expires_at {
            return Err(ContractError::Expired);
        }
        Ok(thread)
    }

    fn store_thread(env: &Env, key: &DataKey, thread: &ThreadRecord) {
        env.storage().persistent().set(key, thread);
        Self::extend(env, key);
    }

    fn load_message(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        thread_id: &BytesN<32>,
        message_id: &BytesN<32>,
    ) -> Result<MessageRecord, ContractError> {
        let key = Self::message_key(tenant_id, project_id, thread_id, message_id);
        let message = env
            .storage()
            .persistent()
            .get::<_, MessageRecord>(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(env, &key);
        if message.schema_version != SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        Ok(message)
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
