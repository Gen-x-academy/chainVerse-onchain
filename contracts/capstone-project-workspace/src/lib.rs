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
    InvalidState = 14,
    AlreadyMember = 15,
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

const MAX_MEMBERS: u32 = 64;
/// A membership may be time boxed to at most five years.
const MAX_MEMBERSHIP_TERM: u64 = 157_680_000;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Manager = 1,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberRole {
    Lead = 1,
    Contributor = 2,
    Observer = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceState {
    Active = 1,
    Paused = 2,
    Archived = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    Overview = 1,
    Members = 2,
    Objectives = 3,
    Milestones = 4,
    Resources = 5,
    Activity = 6,
    NextActions = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SectionRef {
    pub section: Section,
    pub ref_hash: BytesN<32>,
    pub version: u32,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Member {
    pub account: Address,
    pub role: MemberRole,
    pub added_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub state: WorkspaceState,
    pub member_count: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberPage {
    pub members: Vec<Address>,
    pub next: u32,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Access {
    Manager,
    Member(MemberRole),
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Workspace(BytesN<32>, BytesN<32>),
    Member(BytesN<32>, BytesN<32>, Address),
    MemberList(BytesN<32>, BytesN<32>),
    Section(BytesN<32>, BytesN<32>, Section),
}

#[contract]
pub struct CapstoneProjectWorkspace;

#[contractimpl]
impl CapstoneProjectWorkspace {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("WSP_INIT"),), (admin,));
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
            (symbol_short!("WSP_ROLE"),),
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

    pub fn create_workspace(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        overview_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&overview_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP065_CREATE",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&overview_hash) {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Workspace(tenant_id.clone(), project_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let now = env.ledger().timestamp();
        let workspace = Workspace {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            state: WorkspaceState::Active,
            member_count: 0,
            created_at: now,
            updated_at: now,
            version: 1,
        };
        let section_key =
            DataKey::Section(tenant_id.clone(), project_id.clone(), Section::Overview);
        let section = SectionRef {
            section: Section::Overview,
            ref_hash: overview_hash,
            version: 1,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &workspace);
        env.storage().persistent().set(&section_key, &section);
        Self::extend(&env, &key);
        Self::extend(&env, &section_key);
        Self::remember(&env, &tenant_id, &project_id, &request_id, &input_hash, 1);
        env.events()
            .publish((symbol_short!("WSP_NEW"),), (tenant_id, project_id, 1u32));
        Ok(1)
    }

    pub fn add_member(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        member: Address,
        member_role: MemberRole,
        expires_at: u64,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.append(&member.clone().to_xdr(&env));
        extra.extend_from_array(&(member_role as u32).to_be_bytes());
        extra.extend_from_array(&expires_at.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP065_ADD_MEMBER",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let now = env.ledger().timestamp();
        if expires_at != 0 {
            let horizon = now
                .checked_add(MAX_MEMBERSHIP_TERM)
                .ok_or(ContractError::ArithmeticError)?;
            if expires_at <= now || expires_at > horizon {
                return Err(ContractError::InvalidInput);
            }
        }
        let (workspace_key, mut workspace) = Self::load_workspace(&env, &tenant_id, &project_id)?;
        if workspace.state != WorkspaceState::Active {
            return Err(ContractError::InvalidState);
        }
        let member_key = DataKey::Member(tenant_id.clone(), project_id.clone(), member.clone());
        if env.storage().persistent().has(&member_key) {
            return Err(ContractError::AlreadyMember);
        }
        let list_key = DataKey::MemberList(tenant_id.clone(), project_id.clone());
        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| Vec::new(&env));
        if list.len() >= MAX_MEMBERS {
            return Err(ContractError::LimitReached);
        }
        list.push_back(member.clone());
        let record = Member {
            account: member.clone(),
            role: member_role,
            added_at: now,
            expires_at,
        };
        workspace.member_count = list.len();
        workspace.updated_at = now;
        workspace.version = Self::bump(workspace.version)?;
        env.storage().persistent().set(&member_key, &record);
        env.storage().persistent().set(&list_key, &list);
        env.storage().persistent().set(&workspace_key, &workspace);
        Self::extend(&env, &member_key);
        Self::extend(&env, &list_key);
        Self::extend(&env, &workspace_key);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            workspace.version,
        );
        env.events().publish(
            (symbol_short!("WSP_MEM"),),
            (
                tenant_id,
                project_id,
                member,
                member_role as u32,
                expires_at,
                workspace.version,
            ),
        );
        Ok(workspace.version)
    }

    pub fn remove_member(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        member: Address,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.append(&member.clone().to_xdr(&env));
        let input_hash = Self::hash_input(
            &env,
            b"CAP065_REMOVE_MEMBER",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let (workspace_key, mut workspace) = Self::load_workspace(&env, &tenant_id, &project_id)?;
        if workspace.state == WorkspaceState::Archived {
            return Err(ContractError::InvalidState);
        }
        let member_key = DataKey::Member(tenant_id.clone(), project_id.clone(), member.clone());
        if !env.storage().persistent().has(&member_key) {
            return Err(ContractError::NotFound);
        }
        let list_key = DataKey::MemberList(tenant_id.clone(), project_id.clone());
        let mut list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&list_key)
            .unwrap_or_else(|| Vec::new(&env));
        let mut position = None;
        let mut cursor = 0;
        while cursor < list.len() {
            if list.get(cursor) == Some(member.clone()) {
                position = Some(cursor);
                break;
            }
            cursor += 1;
        }
        let position = position.ok_or(ContractError::NotFound)?;
        list.remove(position);
        env.storage().persistent().remove(&member_key);
        env.storage().persistent().set(&list_key, &list);
        workspace.member_count = list.len();
        workspace.updated_at = env.ledger().timestamp();
        workspace.version = Self::bump(workspace.version)?;
        env.storage().persistent().set(&workspace_key, &workspace);
        Self::extend(&env, &list_key);
        Self::extend(&env, &workspace_key);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            workspace.version,
        );
        env.events().publish(
            (symbol_short!("WSP_RMV"),),
            (tenant_id, project_id, member, workspace.version),
        );
        Ok(workspace.version)
    }

    /// Returns the new version of the section. A section that was never set
    /// has version 0, so the first write passes `expected_section_version = 0`.
    pub fn update_section(
        env: Env,
        editor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        section: Section,
        expected_section_version: u32,
        ref_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        editor.require_auth();
        let access = Self::resolve_access(&env, &editor, &tenant_id, &project_id)?;
        if !Self::can_edit(access, section) {
            return Err(ContractError::Unauthorized);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&(section as u32).to_be_bytes());
        extra.extend_from_array(&expected_section_version.to_be_bytes());
        extra.extend_from_array(&ref_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP065_SECTION",
            &tenant_id,
            &project_id,
            &request_id,
            &editor,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&ref_hash) {
            return Err(ContractError::InvalidInput);
        }
        let (workspace_key, mut workspace) = Self::load_workspace(&env, &tenant_id, &project_id)?;
        if workspace.state != WorkspaceState::Active {
            return Err(ContractError::InvalidState);
        }
        let section_key = DataKey::Section(tenant_id.clone(), project_id.clone(), section);
        let current: u32 = env
            .storage()
            .persistent()
            .get::<DataKey, SectionRef>(&section_key)
            .map(|existing| existing.version)
            .unwrap_or(0);
        if current != expected_section_version {
            return Err(ContractError::StaleVersion);
        }
        let now = env.ledger().timestamp();
        let next = Self::bump(current)?;
        let record = SectionRef {
            section,
            ref_hash,
            version: next,
            updated_at: now,
        };
        workspace.updated_at = now;
        workspace.version = Self::bump(workspace.version)?;
        env.storage().persistent().set(&section_key, &record);
        env.storage().persistent().set(&workspace_key, &workspace);
        Self::extend(&env, &section_key);
        Self::extend(&env, &workspace_key);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            next,
        );
        env.events().publish(
            (symbol_short!("WSP_SEC"),),
            (
                tenant_id,
                project_id,
                section as u32,
                next,
                workspace.version,
            ),
        );
        Ok(next)
    }

    pub fn set_state(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        to: WorkspaceState,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&(to as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP065_STATE",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let (workspace_key, mut workspace) = Self::load_workspace(&env, &tenant_id, &project_id)?;
        if workspace.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let from = workspace.state;
        let allowed = matches!(
            (from, to),
            (WorkspaceState::Active, WorkspaceState::Paused)
                | (WorkspaceState::Paused, WorkspaceState::Active)
                | (WorkspaceState::Active, WorkspaceState::Archived)
                | (WorkspaceState::Paused, WorkspaceState::Archived)
        );
        if !allowed {
            return Err(ContractError::InvalidTransition);
        }
        workspace.state = to;
        workspace.updated_at = env.ledger().timestamp();
        workspace.version = Self::bump(workspace.version)?;
        env.storage().persistent().set(&workspace_key, &workspace);
        Self::extend(&env, &workspace_key);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            workspace.version,
        );
        env.events().publish(
            (symbol_short!("WSP_STS"),),
            (
                tenant_id,
                project_id,
                from as u32,
                to as u32,
                workspace.version,
            ),
        );
        Ok(workspace.version)
    }

    pub fn get_home(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<Workspace, ContractError> {
        viewer.require_auth();
        Self::resolve_access(&env, &viewer, &tenant_id, &project_id)?;
        let (_, workspace) = Self::load_workspace(&env, &tenant_id, &project_id)?;
        Ok(workspace)
    }

    pub fn get_section(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        section: Section,
    ) -> Result<SectionRef, ContractError> {
        viewer.require_auth();
        Self::resolve_access(&env, &viewer, &tenant_id, &project_id)?;
        let key = DataKey::Section(tenant_id, project_id, section);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(record)
    }

    pub fn get_member(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        member: Address,
    ) -> Result<Member, ContractError> {
        viewer.require_auth();
        Self::resolve_access(&env, &viewer, &tenant_id, &project_id)?;
        let key = DataKey::Member(tenant_id, project_id, member);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)
    }

    pub fn list_members(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Result<MemberPage, ContractError> {
        viewer.require_auth();
        Self::resolve_access(&env, &viewer, &tenant_id, &project_id)?;
        let list: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::MemberList(tenant_id, project_id))
            .unwrap_or_else(|| Vec::new(&env));
        let (start, end) = Self::page_range(list.len(), from, limit)?;
        let mut members = Vec::new(&env);
        let mut cursor = start;
        while cursor < end {
            if let Some(account) = list.get(cursor) {
                members.push_back(account);
            }
            cursor += 1;
        }
        Ok(MemberPage {
            members,
            next: end,
            has_more: end < list.len(),
        })
    }
}

impl CapstoneProjectWorkspace {
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

impl CapstoneProjectWorkspace {
    /// Managers and unexpired members may read; everyone else is denied.
    fn resolve_access(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<Access, ContractError> {
        match Self::require_any(env, account, tenant_id, project_id, &[Role::Manager]) {
            Ok(_) => Ok(Access::Manager),
            Err(denied) => {
                let key = DataKey::Member(tenant_id.clone(), project_id.clone(), account.clone());
                match env.storage().persistent().get::<DataKey, Member>(&key) {
                    Some(member) => {
                        if member.expires_at != 0 && env.ledger().timestamp() >= member.expires_at {
                            return Err(ContractError::RoleExpired);
                        }
                        Self::extend(env, &key);
                        Ok(Access::Member(member.role))
                    }
                    None => Err(denied),
                }
            }
        }
    }

    fn can_edit(access: Access, section: Section) -> bool {
        match access {
            Access::Manager => true,
            Access::Member(MemberRole::Lead) => section != Section::Members,
            Access::Member(MemberRole::Contributor) => matches!(
                section,
                Section::Resources | Section::Activity | Section::NextActions
            ),
            Access::Member(MemberRole::Observer) => false,
        }
    }

    fn load_workspace(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<(DataKey, Workspace), ContractError> {
        let key = DataKey::Workspace(tenant_id.clone(), project_id.clone());
        let workspace: Workspace = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Ok((key, workspace))
    }
}
