#![no_std]
#![allow(clippy::too_many_arguments)]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_PAGE: u32 = 32;
const CONTRACT_VERSION: u32 = 1;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    RequestConflict = 4,
    StaleVersion = 5,
    NotFound = 6,
    AlreadyExists = 7,
    InvalidInput = 8,
    InvalidTransition = 9,
    LimitReached = 10,
    InvalidPage = 11,
    ArithmeticError = 12,
    RoleExpired = 13,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub active: bool,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRecord {
    pub input_hash: BytesN<32>,
    pub result: u32,
}

const MAX_ENTRIES: u32 = 256;
/// Planned entries may sit at most five years ahead of the ledger clock.
const MAX_PLAN_HORIZON: u64 = 157_680_000;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Manager = 1,
    Publisher = 2,
    Viewer = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Objective = 1,
    Task = 2,
    Milestone = 3,
    Decision = 4,
    Review = 5,
    Funding = 6,
    CharterChange = 7,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Team = 1,
    Restricted = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryState {
    Active = 1,
    Redacted = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineEntry {
    pub entry_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub seq: u32,
    pub kind: EntryKind,
    pub ref_hash: BytesN<32>,
    pub redaction_hash: BytesN<32>,
    pub occurred_at: u64,
    pub visibility: Visibility,
    pub state: EntryState,
    pub recorded_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelinePage {
    pub entries: Vec<TimelineEntry>,
    pub next_seq: u32,
    pub has_more: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Head(BytesN<32>, BytesN<32>),
    Entry(BytesN<32>, BytesN<32>, u32),
    EntrySeq(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneProjectTimeline;

#[contractimpl]
impl CapstoneProjectTimeline {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("TLN_INIT"),), (admin,));
        Ok(())
    }

    pub fn set_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        role: Role,
        active: bool,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if active && expires_at != 0 && expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        let grant = RoleGrant {
            active,
            expires_at: if active { expires_at } else { 0 },
        };
        env.storage().persistent().set(&key, &grant);
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("TLN_ROLE"),),
            (
                tenant_id,
                project_id,
                account,
                role as u32,
                active,
                grant.expires_at,
            ),
        );
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    pub fn append_entry(
        env: Env,
        publisher: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        entry_id: BytesN<32>,
        kind: EntryKind,
        ref_hash: BytesN<32>,
        occurred_at: u64,
        visibility: Visibility,
    ) -> Result<u32, ContractError> {
        publisher.require_auth();
        Self::require_any(
            &env,
            &publisher,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Publisher],
        )?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&entry_id.to_array());
        extra.extend_from_array(&(kind as u32).to_be_bytes());
        extra.extend_from_array(&ref_hash.to_array());
        extra.extend_from_array(&occurred_at.to_be_bytes());
        extra.extend_from_array(&(visibility as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP063_APPEND",
            &tenant_id,
            &project_id,
            &request_id,
            &publisher,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&entry_id) || !Self::nonzero(&ref_hash) || occurred_at == 0 {
            return Err(ContractError::InvalidInput);
        }
        let now = env.ledger().timestamp();
        let horizon = now
            .checked_add(MAX_PLAN_HORIZON)
            .ok_or(ContractError::ArithmeticError)?;
        if occurred_at > horizon {
            return Err(ContractError::InvalidInput);
        }
        let seq_key = DataKey::EntrySeq(tenant_id.clone(), project_id.clone(), entry_id.clone());
        if env.storage().persistent().has(&seq_key) {
            return Err(ContractError::AlreadyExists);
        }
        let head_key = DataKey::Head(tenant_id.clone(), project_id.clone());
        let head: u32 = env.storage().persistent().get(&head_key).unwrap_or(0);
        if head >= MAX_ENTRIES {
            return Err(ContractError::LimitReached);
        }
        let seq = Self::bump(head)?;
        let entry = TimelineEntry {
            entry_id: entry_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            seq,
            kind,
            ref_hash,
            redaction_hash: BytesN::from_array(&env, &[0u8; 32]),
            occurred_at,
            visibility,
            state: EntryState::Active,
            recorded_at: now,
        };
        let entry_key = DataKey::Entry(tenant_id.clone(), project_id.clone(), seq);
        env.storage().persistent().set(&entry_key, &entry);
        env.storage().persistent().set(&seq_key, &seq);
        env.storage().persistent().set(&head_key, &seq);
        Self::extend(&env, &entry_key);
        Self::extend(&env, &seq_key);
        Self::extend(&env, &head_key);
        Self::remember(&env, &tenant_id, &project_id, &request_id, &input_hash, seq);
        env.events().publish(
            (symbol_short!("TLN_ADD"),),
            (
                tenant_id,
                project_id,
                entry_id,
                seq,
                kind as u32,
                occurred_at,
                visibility as u32,
            ),
        );
        Ok(seq)
    }

    /// Replaces the stored reference with a zero hash so a withdrawn or
    /// mistaken pointer stops resolving. The sequence slot is kept, so every
    /// other entry keeps its position.
    pub fn redact_entry(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        entry_id: BytesN<32>,
        redaction_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&entry_id.to_array());
        extra.extend_from_array(&redaction_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP063_REDACT",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&redaction_hash) {
            return Err(ContractError::InvalidInput);
        }
        let seq_key = DataKey::EntrySeq(tenant_id.clone(), project_id.clone(), entry_id.clone());
        let seq: u32 = env
            .storage()
            .persistent()
            .get(&seq_key)
            .ok_or(ContractError::NotFound)?;
        let entry_key = DataKey::Entry(tenant_id.clone(), project_id.clone(), seq);
        let mut entry: TimelineEntry = env
            .storage()
            .persistent()
            .get(&entry_key)
            .ok_or(ContractError::NotFound)?;
        if entry.state != EntryState::Active {
            return Err(ContractError::InvalidTransition);
        }
        entry.state = EntryState::Redacted;
        entry.ref_hash = BytesN::from_array(&env, &[0u8; 32]);
        entry.redaction_hash = redaction_hash;
        env.storage().persistent().set(&entry_key, &entry);
        Self::extend(&env, &entry_key);
        Self::remember(&env, &tenant_id, &project_id, &request_id, &input_hash, seq);
        env.events().publish(
            (symbol_short!("TLN_RED"),),
            (tenant_id, project_id, entry_id, seq),
        );
        Ok(seq)
    }

    /// Restricted entries the caller may not see are reported as `NotFound`.
    pub fn get_entry(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        entry_id: BytesN<32>,
    ) -> Result<TimelineEntry, ContractError> {
        viewer.require_auth();
        let role = Self::require_any(
            &env,
            &viewer,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Publisher, Role::Viewer],
        )?;
        let seq: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::EntrySeq(
                tenant_id.clone(),
                project_id.clone(),
                entry_id,
            ))
            .ok_or(ContractError::NotFound)?;
        let key = DataKey::Entry(tenant_id, project_id, seq);
        let entry: TimelineEntry = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if !Self::visible_to(role, &entry) {
            return Err(ContractError::NotFound);
        }
        Self::extend(&env, &key);
        Ok(entry)
    }

    /// Entries come back in ascending sequence order, which never changes once
    /// assigned. `next_seq` is the cursor for the following page. Entries the
    /// caller may not see are skipped without shifting any cursor.
    pub fn list_entries(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from_seq: u32,
        limit: u32,
    ) -> Result<TimelinePage, ContractError> {
        viewer.require_auth();
        let role = Self::require_any(
            &env,
            &viewer,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Publisher, Role::Viewer],
        )?;
        if from_seq == 0 {
            return Err(ContractError::InvalidPage);
        }
        let head: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Head(tenant_id.clone(), project_id.clone()))
            .unwrap_or(0);
        let len = Self::bump(head)?;
        let (start, end) = Self::page_range(len, from_seq, limit)?;
        let mut entries = Vec::new(&env);
        let mut seq = start;
        while seq < end {
            let key = DataKey::Entry(tenant_id.clone(), project_id.clone(), seq);
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<DataKey, TimelineEntry>(&key)
            {
                if Self::visible_to(role, &entry) {
                    entries.push_back(entry);
                }
            }
            seq += 1;
        }
        Ok(TimelinePage {
            entries,
            next_seq: end,
            has_more: end < len,
        })
    }
}

impl CapstoneProjectTimeline {
    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &admin != caller {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Deny by default: succeeds only when the account holds one of `roles`,
    /// active and unexpired, for exactly this tenant and project.
    fn require_any(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        roles: &[Role],
    ) -> Result<Role, ContractError> {
        let now = env.ledger().timestamp();
        let mut expired = false;
        for role in roles.iter() {
            let key = DataKey::Role(
                account.clone(),
                tenant_id.clone(),
                project_id.clone(),
                *role,
            );
            if let Some(grant) = env.storage().persistent().get::<DataKey, RoleGrant>(&key) {
                if !grant.active {
                    continue;
                }
                if grant.expires_at != 0 && now >= grant.expires_at {
                    expired = true;
                    continue;
                }
                Self::extend(env, &key);
                return Ok(*role);
            }
        }
        if expired {
            Err(ContractError::RoleExpired)
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn hash_input(
        env: &Env,
        domain: &[u8],
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        actor: &Address,
        extra: &Bytes,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, domain));
        input.extend_from_array(&tenant_id.to_array());
        input.extend_from_array(&project_id.to_array());
        input.extend_from_array(&request_id.to_array());
        input.append(&actor.clone().to_xdr(env));
        input.append(extra);
        env.crypto().sha256(&input).to_bytes()
    }

    /// Returns the recorded result when this exact request already succeeded.
    /// The same `request_id` with a different payload or actor is rejected.
    fn replay(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        input_hash: &BytesN<32>,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(record) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestRecord>(&key)
        {
            if &record.input_hash != input_hash {
                return Err(ContractError::RequestConflict);
            }
            Self::extend(env, &key);
            return Ok(Some(record.result));
        }
        Ok(None)
    }

    fn remember(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        input_hash: &BytesN<32>,
        result: u32,
    ) {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let record = RequestRecord {
            input_hash: input_hash.clone(),
            result,
        };
        env.storage().persistent().set(&key, &record);
        Self::extend(env, &key);
    }

    fn nonzero(hash: &BytesN<32>) -> bool {
        hash.to_array().iter().any(|byte| *byte != 0)
    }

    fn bump(version: u32) -> Result<u32, ContractError> {
        version.checked_add(1).ok_or(ContractError::ArithmeticError)
    }

    fn page_range(len: u32, from: u32, limit: u32) -> Result<(u32, u32), ContractError> {
        if limit == 0 || limit > MAX_PAGE || from > len {
            return Err(ContractError::InvalidPage);
        }
        let end = from
            .checked_add(limit)
            .ok_or(ContractError::ArithmeticError)?;
        Ok((from, if end > len { len } else { end }))
    }
}

impl CapstoneProjectTimeline {
    fn visible_to(role: Role, entry: &TimelineEntry) -> bool {
        entry.visibility == Visibility::Team || role == Role::Manager
    }
}
