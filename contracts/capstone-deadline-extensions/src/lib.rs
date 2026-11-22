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
    AlreadyOpen = 14,
    DeadlinePassed = 15,
    SeparationOfDuty = 16,
    RequestExpired = 17,
    AlreadyApproved = 18,
    NotYetExpired = 19,
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

const MAX_EXTENSIONS: u32 = 64;
const MAX_APPROVERS: u32 = 8;
/// A single extension may move a deadline by at most 90 days.
const MAX_EXTENSION_SECS: u64 = 7_776_000;
/// Reviewers get at most 30 days to decide a request.
const MAX_DECISION_WINDOW: u64 = 2_592_000;
const MAX_FUNDING_DELTA: i128 = 1_000_000_000_000_000_000;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Manager = 1,
    Requester = 2,
    Approver = 3,
    Viewer = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionState {
    Requested = 1,
    Approved = 2,
    Rejected = 3,
    Withdrawn = 4,
    Expired = 5,
    Applied = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInput {
    pub extension_id: BytesN<32>,
    pub target_id: BytesN<32>,
    pub reason_hash: BytesN<32>,
    pub impact_hash: BytesN<32>,
    pub current_deadline: u64,
    pub proposed_deadline: u64,
    pub funding_delta: i128,
    pub approvals_required: u32,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionRecord {
    pub extension_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub target_id: BytesN<32>,
    pub requester: Address,
    pub reason_hash: BytesN<32>,
    pub impact_hash: BytesN<32>,
    pub current_deadline: u64,
    pub proposed_deadline: u64,
    pub funding_delta: i128,
    pub approvals_required: u32,
    pub approvals: u32,
    pub expires_at: u64,
    pub state: ExtensionState,
    pub decision_hash: BytesN<32>,
    pub revised_schedule_hash: BytesN<32>,
    pub created_at: u64,
    pub decided_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionPage {
    pub ids: Vec<BytesN<32>>,
    pub next: u32,
    pub has_more: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Extension(BytesN<32>, BytesN<32>, BytesN<32>),
    ExtensionIndex(BytesN<32>, BytesN<32>),
    Open(BytesN<32>, BytesN<32>, BytesN<32>),
    Approval(BytesN<32>, BytesN<32>, BytesN<32>, Address),
}

#[contract]
pub struct CapstoneDeadlineExtensions;

#[contractimpl]
impl CapstoneDeadlineExtensions {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("EXT_INIT"),), (admin,));
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
            (symbol_short!("EXT_ROLE"),),
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

    pub fn submit_request(
        env: Env,
        requester: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        input: ExtensionInput,
    ) -> Result<u32, ContractError> {
        requester.require_auth();
        Self::require_any(
            &env,
            &requester,
            &tenant_id,
            &project_id,
            &[Role::Requester],
        )?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&input.extension_id.to_array());
        extra.extend_from_array(&input.target_id.to_array());
        extra.extend_from_array(&input.reason_hash.to_array());
        extra.extend_from_array(&input.impact_hash.to_array());
        extra.extend_from_array(&input.current_deadline.to_be_bytes());
        extra.extend_from_array(&input.proposed_deadline.to_be_bytes());
        extra.extend_from_array(&input.funding_delta.to_be_bytes());
        extra.extend_from_array(&input.approvals_required.to_be_bytes());
        extra.extend_from_array(&input.expires_at.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_SUBMIT",
            &tenant_id,
            &project_id,
            &request_id,
            &requester,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&input.extension_id)
            || !Self::nonzero(&input.target_id)
            || !Self::nonzero(&input.reason_hash)
            || !Self::nonzero(&input.impact_hash)
            || input.approvals_required == 0
            || input.approvals_required > MAX_APPROVERS
            || input.funding_delta < 0
            || input.funding_delta > MAX_FUNDING_DELTA
            || input.proposed_deadline <= input.current_deadline
        {
            return Err(ContractError::InvalidInput);
        }
        if input.proposed_deadline - input.current_deadline > MAX_EXTENSION_SECS {
            return Err(ContractError::InvalidInput);
        }
        let now = env.ledger().timestamp();
        if input.current_deadline <= now {
            return Err(ContractError::DeadlinePassed);
        }
        let window = now
            .checked_add(MAX_DECISION_WINDOW)
            .ok_or(ContractError::ArithmeticError)?;
        if input.expires_at <= now
            || input.expires_at > window
            || input.expires_at > input.current_deadline
        {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Extension(
            tenant_id.clone(),
            project_id.clone(),
            input.extension_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let open_key = DataKey::Open(
            tenant_id.clone(),
            project_id.clone(),
            input.target_id.clone(),
        );
        if env.storage().persistent().has(&open_key) {
            return Err(ContractError::AlreadyOpen);
        }
        let index_key = DataKey::ExtensionIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if index.len() >= MAX_EXTENSIONS {
            return Err(ContractError::LimitReached);
        }
        let zero = BytesN::from_array(&env, &[0u8; 32]);
        let record = ExtensionRecord {
            extension_id: input.extension_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            target_id: input.target_id.clone(),
            requester,
            reason_hash: input.reason_hash,
            impact_hash: input.impact_hash,
            current_deadline: input.current_deadline,
            proposed_deadline: input.proposed_deadline,
            funding_delta: input.funding_delta,
            approvals_required: input.approvals_required,
            approvals: 0,
            expires_at: input.expires_at,
            state: ExtensionState::Requested,
            decision_hash: zero.clone(),
            revised_schedule_hash: zero,
            created_at: now,
            decided_at: 0,
            version: 1,
        };
        index.push_back(input.extension_id.clone());
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .set(&open_key, &input.extension_id);
        env.storage().persistent().set(&index_key, &index);
        Self::extend(&env, &key);
        Self::extend(&env, &open_key);
        Self::extend(&env, &index_key);
        Self::remember(&env, &tenant_id, &project_id, &request_id, &input_hash, 1);
        env.events().publish(
            (symbol_short!("EXT_REQ"),),
            (
                tenant_id,
                project_id,
                input.extension_id,
                input.target_id,
                input.proposed_deadline,
                input.funding_delta,
                1u32,
            ),
        );
        Ok(1)
    }

    pub fn approve(
        env: Env,
        approver: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        extension_id: BytesN<32>,
        expected_version: u32,
        approval_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        approver.require_auth();
        Self::require_any(&env, &approver, &tenant_id, &project_id, &[Role::Approver])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&extension_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&approval_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_APPROVE",
            &tenant_id,
            &project_id,
            &request_id,
            &approver,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&approval_hash) {
            return Err(ContractError::InvalidInput);
        }
        let (key, mut record) = Self::load_pending(
            &env,
            &tenant_id,
            &project_id,
            &extension_id,
            expected_version,
            &approver,
        )?;
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            extension_id.clone(),
            approver.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::AlreadyApproved);
        }
        record.approvals = Self::bump(record.approvals)?;
        if record.approvals >= record.approvals_required {
            record.state = ExtensionState::Approved;
        }
        record.version = Self::bump(record.version)?;
        env.storage()
            .persistent()
            .set(&approval_key, &approval_hash);
        Self::extend(&env, &approval_key);
        Self::save(&env, &key, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("EXT_APPR"),),
            (
                tenant_id,
                project_id,
                extension_id,
                record.approvals,
                record.state as u32,
                record.version,
            ),
        );
        Ok(record.version)
    }

    pub fn reject(
        env: Env,
        approver: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        extension_id: BytesN<32>,
        expected_version: u32,
        decision_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        approver.require_auth();
        Self::require_any(&env, &approver, &tenant_id, &project_id, &[Role::Approver])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&extension_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&decision_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_REJECT",
            &tenant_id,
            &project_id,
            &request_id,
            &approver,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&decision_hash) {
            return Err(ContractError::InvalidInput);
        }
        let (key, mut record) = Self::load_pending(
            &env,
            &tenant_id,
            &project_id,
            &extension_id,
            expected_version,
            &approver,
        )?;
        record.state = ExtensionState::Rejected;
        record.decision_hash = decision_hash;
        record.decided_at = env.ledger().timestamp();
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::release_open(&env, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("EXT_REJ"),),
            (tenant_id, project_id, extension_id, record.version),
        );
        Ok(record.version)
    }

    pub fn withdraw(
        env: Env,
        requester: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        extension_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        requester.require_auth();
        Self::require_any(
            &env,
            &requester,
            &tenant_id,
            &project_id,
            &[Role::Requester],
        )?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&extension_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_WITHDRAW",
            &tenant_id,
            &project_id,
            &request_id,
            &requester,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let key = DataKey::Extension(tenant_id.clone(), project_id.clone(), extension_id.clone());
        let mut record: ExtensionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if record.requester != requester {
            return Err(ContractError::Unauthorized);
        }
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != ExtensionState::Requested {
            return Err(ContractError::InvalidTransition);
        }
        record.state = ExtensionState::Withdrawn;
        record.decided_at = env.ledger().timestamp();
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::release_open(&env, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("EXT_WDR"),),
            (tenant_id, project_id, extension_id, record.version),
        );
        Ok(record.version)
    }

    /// Records the revised schedule and the final decision. After this call the
    /// record is terminal and immutable.
    pub fn publish_revision(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        extension_id: BytesN<32>,
        expected_version: u32,
        revised_schedule_hash: BytesN<32>,
        decision_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&extension_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&revised_schedule_hash.to_array());
        extra.extend_from_array(&decision_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_PUBLISH",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&revised_schedule_hash) || !Self::nonzero(&decision_hash) {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Extension(tenant_id.clone(), project_id.clone(), extension_id.clone());
        let mut record: ExtensionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != ExtensionState::Approved {
            return Err(ContractError::InvalidTransition);
        }
        let now = env.ledger().timestamp();
        if now >= record.expires_at {
            return Err(ContractError::RequestExpired);
        }
        record.state = ExtensionState::Applied;
        record.revised_schedule_hash = revised_schedule_hash;
        record.decision_hash = decision_hash;
        record.decided_at = now;
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::release_open(&env, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("EXT_APL"),),
            (
                tenant_id,
                project_id,
                extension_id,
                record.proposed_deadline,
                record.funding_delta,
                record.version,
            ),
        );
        Ok(record.version)
    }

    /// Any scoped role may close out an overdue request so the target is not
    /// blocked forever by a stalled review.
    pub fn expire(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        extension_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        actor.require_auth();
        Self::require_any(
            &env,
            &actor,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Requester, Role::Approver, Role::Viewer],
        )?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&extension_id.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP064_EXPIRE",
            &tenant_id,
            &project_id,
            &request_id,
            &actor,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let key = DataKey::Extension(tenant_id.clone(), project_id.clone(), extension_id.clone());
        let mut record: ExtensionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if !matches!(
            record.state,
            ExtensionState::Requested | ExtensionState::Approved
        ) {
            return Err(ContractError::InvalidTransition);
        }
        let now = env.ledger().timestamp();
        if now < record.expires_at {
            return Err(ContractError::NotYetExpired);
        }
        record.state = ExtensionState::Expired;
        record.decided_at = now;
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::release_open(&env, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("EXT_EXP"),),
            (tenant_id, project_id, extension_id, record.version),
        );
        Ok(record.version)
    }

    pub fn get_extension(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        extension_id: BytesN<32>,
    ) -> Result<ExtensionRecord, ContractError> {
        viewer.require_auth();
        Self::require_viewer(&env, &viewer, &tenant_id, &project_id)?;
        let key = DataKey::Extension(tenant_id, project_id, extension_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(record)
    }

    pub fn open_request_for(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        target_id: BytesN<32>,
    ) -> Result<Option<BytesN<32>>, ContractError> {
        viewer.require_auth();
        Self::require_viewer(&env, &viewer, &tenant_id, &project_id)?;
        Ok(env
            .storage()
            .persistent()
            .get(&DataKey::Open(tenant_id, project_id, target_id)))
    }

    pub fn list_extensions(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Result<ExtensionPage, ContractError> {
        viewer.require_auth();
        Self::require_viewer(&env, &viewer, &tenant_id, &project_id)?;
        let index_key = DataKey::ExtensionIndex(tenant_id, project_id);
        let index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        let (start, end) = Self::page_range(index.len(), from, limit)?;
        let mut ids = Vec::new(&env);
        let mut cursor = start;
        while cursor < end {
            if let Some(id) = index.get(cursor) {
                ids.push_back(id);
            }
            cursor += 1;
        }
        Ok(ExtensionPage {
            ids,
            next: end,
            has_more: end < index.len(),
        })
    }
}

impl CapstoneDeadlineExtensions {
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

impl CapstoneDeadlineExtensions {
    fn require_viewer(
        env: &Env,
        viewer: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<Role, ContractError> {
        Self::require_any(
            env,
            viewer,
            tenant_id,
            project_id,
            &[Role::Manager, Role::Requester, Role::Approver, Role::Viewer],
        )
    }

    /// Loads a request that is still waiting for reviewer input and enforces
    /// the checks shared by approve and reject.
    fn load_pending(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        extension_id: &BytesN<32>,
        expected_version: u32,
        reviewer: &Address,
    ) -> Result<(DataKey, ExtensionRecord), ContractError> {
        let key = DataKey::Extension(tenant_id.clone(), project_id.clone(), extension_id.clone());
        let record: ExtensionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != ExtensionState::Requested {
            return Err(ContractError::InvalidTransition);
        }
        if env.ledger().timestamp() >= record.expires_at {
            return Err(ContractError::RequestExpired);
        }
        if &record.requester == reviewer {
            return Err(ContractError::SeparationOfDuty);
        }
        Ok((key, record))
    }

    fn release_open(env: &Env, record: &ExtensionRecord) {
        let open_key = DataKey::Open(
            record.tenant_id.clone(),
            record.project_id.clone(),
            record.target_id.clone(),
        );
        let current: Option<BytesN<32>> = env.storage().persistent().get(&open_key);
        if current == Some(record.extension_id.clone()) {
            env.storage().persistent().remove(&open_key);
        }
    }

    fn save(env: &Env, key: &DataKey, record: &ExtensionRecord) {
        env.storage().persistent().set(key, record);
        Self::extend(env, key);
    }
}
