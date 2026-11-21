#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_REVIEWERS: u32 = 32;
const MAX_CAPACITY: u32 = 32;
const MAX_QUEUE: u32 = 128;
const MAX_AUDIT: u32 = 32;
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
    NotReviewer = 11,
    ReviewerAtCapacity = 12,
    AssignmentConflict = 13,
    ConflictOfInterest = 14,
    CollectionFull = 15,
    InvalidPage = 16,
    InvalidFilter = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Coordinator = 2,
    Reviewer = 3,
    Observer = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Priority {
    Low = 1,
    Normal = 2,
    High = 3,
    Urgent = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpertiseFit {
    Unassigned = 0,
    NoFit = 1,
    Partial = 2,
    Full = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueState {
    Pending = 1,
    Assigned = 2,
    Decided = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewOutcome {
    Approve = 1,
    Reject = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditKind {
    Enqueued = 1,
    Assigned = 2,
    PriorityChanged = 3,
    ConflictDeclared = 4,
    ConflictCleared = 5,
    Decided = 6,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    RoleCount(BytesN<32>),
    Version(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Reviewer(BytesN<32>, BytesN<32>, Address),
    ReviewerCount(BytesN<32>, BytesN<32>),
    QueueIndex(BytesN<32>, BytesN<32>),
    Item(BytesN<32>, BytesN<32>, BytesN<32>),
    Conflict(BytesN<32>, BytesN<32>, BytesN<32>, Address),
    Audit(BytesN<32>, BytesN<32>, BytesN<32>),
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
pub struct ReviewerProfile {
    pub capacity: u32,
    pub expertise_mask: u32,
    pub assigned: u32,
    pub decided: u32,
    pub active: bool,
    pub created_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueItem {
    pub id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub seq: u32,
    pub priority: Priority,
    pub required_expertise_mask: u32,
    pub fit: ExpertiseFit,
    pub assignee: Option<Address>,
    pub due_at: u64,
    pub conflict: bool,
    pub state: QueueState,
    pub outcome: Option<ReviewOutcome>,
    pub created_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditEntry {
    pub kind: AuditKind,
    pub actor_role: Role,
    pub at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueFilter {
    pub priority: Option<Priority>,
    pub assignee: Option<Address>,
    pub unassigned_only: bool,
    pub due_before: Option<u64>,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Workload {
    pub capacity: u32,
    pub assigned: u32,
    pub decided: u32,
    pub overdue: u32,
    pub active: bool,
    pub expertise_mask: u32,
}

struct RequestToken {
    replay: Option<RequestRecord>,
    next_version: u32,
}

#[contract]
pub struct CapstoneReviewQueuesContract;

#[contractimpl]
impl CapstoneReviewQueuesContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("CQRINIT"),), (admin,));
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

    pub fn set_reviewer(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        reviewer: Address,
        capacity: u32,
        expertise_mask: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        if capacity == 0 || capacity > MAX_CAPACITY || expertise_mask == 0 {
            return Err(ContractError::InvalidInput);
        }
        let reviewer_key = DataKey::Reviewer(
            tenant_id.clone(),
            project_id.clone(),
            reviewer.clone(),
        );
        let existing: Option<ReviewerProfile> = env.storage().persistent().get(&reviewer_key);
        if let Some(profile) = existing.as_ref() {
            if profile.assigned > capacity {
                return Err(ContractError::InvalidInput);
            }
        }
        let mut extra = Bytes::new(&env);
        extra.append(&reviewer.to_xdr(&env));
        extra.extend_from_array(&capacity.to_be_bytes());
        extra.extend_from_array(&expertise_mask.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_REVIEWER",
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
        let profile = match existing {
            Some(mut profile) => {
                profile.capacity = capacity;
                profile.expertise_mask = expertise_mask;
                profile.active = true;
                profile.version = token.next_version;
                profile
            }
            None => {
                let count_key = DataKey::ReviewerCount(tenant_id.clone(), project_id.clone());
                let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
                if count >= MAX_REVIEWERS {
                    return Err(ContractError::CollectionFull);
                }
                let next = count.checked_add(1).ok_or(ContractError::VersionOverflow)?;
                Self::write_u32(&env, &count_key, next);
                ReviewerProfile {
                    capacity,
                    expertise_mask,
                    assigned: 0,
                    decided: 0,
                    active: true,
                    created_at: env.ledger().timestamp(),
                    version: token.next_version,
                }
            }
        };
        Self::write_profile(&env, &reviewer_key, &profile);
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
            (symbol_short!("CQRRV"),),
            (tenant_id, project_id, capacity, expertise_mask),
        );
        Ok(token.next_version)
    }

    pub fn enqueue(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        item_id: BytesN<32>,
        required_expertise_mask: u32,
        priority: Priority,
        due_at: u64,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        if required_expertise_mask == 0 || !Self::valid_priority(priority) {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Item(tenant_id.clone(), project_id.clone(), item_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::QueueIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_ids(&env, &index_key);
        if index.len() >= MAX_QUEUE {
            return Err(ContractError::CollectionFull);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&item_id.to_array());
        extra.extend_from_array(&required_expertise_mask.to_be_bytes());
        extra.extend_from_array(&(priority as u32).to_be_bytes());
        extra.extend_from_array(&due_at.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_ENQUEUE",
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
        let seq = index.len().checked_add(1).ok_or(ContractError::VersionOverflow)?;
        let item = QueueItem {
            id: item_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            seq,
            priority,
            required_expertise_mask,
            fit: ExpertiseFit::Unassigned,
            assignee: None,
            due_at,
            conflict: false,
            state: QueueState::Pending,
            outcome: None,
            created_at: env.ledger().timestamp(),
            version: token.next_version,
        };
        Self::append_audit(
            &env,
            &tenant_id,
            &project_id,
            &item_id,
            AuditKind::Enqueued,
            Role::Coordinator,
        )?;
        let mut updated_index = index;
        updated_index.push_back(item_id.clone());
        Self::write_item(&env, &key, &item);
        Self::write_ids(&env, &index_key, &updated_index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &item_id,
        );
        env.events().publish(
            (symbol_short!("CQRENQ"),),
            (tenant_id, project_id, item_id, priority as u32),
        );
        Ok(token.next_version)
    }

    pub fn assign(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        item_id: BytesN<32>,
        reviewer: Address,
        expected_assignee: Option<Address>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&item_id.to_array());
        extra.append(&reviewer.to_xdr(&env));
        match expected_assignee.as_ref() {
            Some(address) => {
                extra.extend_from_array(&[1u8]);
                extra.append(&address.to_xdr(&env));
            }
            None => extra.extend_from_array(&[0u8]),
        }
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_ASSIGN",
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
        let key = DataKey::Item(tenant_id.clone(), project_id.clone(), item_id.clone());
        let mut item: QueueItem = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if item.state == QueueState::Decided {
            return Err(ContractError::InvalidState);
        }
        if expected_assignee != item.assignee {
            return Err(ContractError::AssignmentConflict);
        }
        let reviewer_key = DataKey::Reviewer(
            tenant_id.clone(),
            project_id.clone(),
            reviewer.clone(),
        );
        let mut profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&reviewer_key)
            .ok_or(ContractError::NotReviewer)?;
        if !profile.active || !Self::has_role(&env, &tenant_id, &reviewer, Role::Reviewer) {
            return Err(ContractError::NotReviewer);
        }
        let conflict_key = DataKey::Conflict(
            tenant_id.clone(),
            project_id.clone(),
            item_id.clone(),
            reviewer.clone(),
        );
        let conflicted = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&conflict_key)
            .unwrap_or(false);
        if conflicted {
            return Err(ContractError::ConflictOfInterest);
        }
        let same_assignee = item.assignee.as_ref() == Some(&reviewer);
        let next_assigned = if same_assignee {
            None
        } else {
            let assigned = profile
                .assigned
                .checked_add(1)
                .ok_or(ContractError::VersionOverflow)?;
            if assigned > profile.capacity {
                return Err(ContractError::ReviewerAtCapacity);
            }
            Some(assigned)
        };
        let mut previous_update: Option<(DataKey, ReviewerProfile)> = None;
        if next_assigned.is_some() {
            if let Some(previous) = item.assignee.as_ref() {
                let previous_key = DataKey::Reviewer(
                    tenant_id.clone(),
                    project_id.clone(),
                    previous.clone(),
                );
                let stored: Option<ReviewerProfile> =
                    env.storage().persistent().get(&previous_key);
                if let Some(mut previous_profile) = stored {
                    previous_profile.assigned = previous_profile
                        .assigned
                        .checked_sub(1)
                        .ok_or(ContractError::InvalidState)?;
                    previous_update = Some((previous_key, previous_profile));
                }
            }
        }
        Self::append_audit(
            &env,
            &tenant_id,
            &project_id,
            &item_id,
            AuditKind::Assigned,
            Role::Coordinator,
        )?;
        if let Some(assigned) = next_assigned {
            profile.assigned = assigned;
        }
        if let Some((previous_key, previous_profile)) = previous_update {
            Self::write_profile(&env, &previous_key, &previous_profile);
        }
        let overlap = item.required_expertise_mask & profile.expertise_mask;
        item.fit = if overlap == item.required_expertise_mask {
            ExpertiseFit::Full
        } else if overlap != 0 {
            ExpertiseFit::Partial
        } else {
            ExpertiseFit::NoFit
        };
        item.assignee = Some(reviewer.clone());
        item.conflict = false;
        item.state = QueueState::Assigned;
        item.version = token.next_version;
        Self::write_profile(&env, &reviewer_key, &profile);
        Self::write_item(&env, &key, &item);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &item_id,
        );
        env.events().publish(
            (symbol_short!("CQRASS"),),
            (tenant_id, project_id, item_id, item.fit as u32),
        );
        Ok(token.next_version)
    }

    pub fn declare_conflict(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        item_id: BytesN<32>,
        declared: bool,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Reviewer)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&item_id.to_array());
        extra.extend_from_array(&[declared as u8]);
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_CONFLICT",
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
        let key = DataKey::Item(tenant_id.clone(), project_id.clone(), item_id.clone());
        let mut item: QueueItem = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if item.state == QueueState::Decided || item.assignee.as_ref() != Some(&actor) {
            return Err(ContractError::Unauthorized);
        }
        let conflict_key = DataKey::Conflict(
            tenant_id.clone(),
            project_id.clone(),
            item_id.clone(),
            actor.clone(),
        );
        if env.storage().persistent().has(&conflict_key) {
            let prior: bool = env.storage().persistent().get(&conflict_key).unwrap_or(false);
            if prior == declared {
                return Err(ContractError::InvalidState);
            }
        }
        let kind = if declared {
            AuditKind::ConflictDeclared
        } else {
            AuditKind::ConflictCleared
        };
        Self::append_audit(&env, &tenant_id, &project_id, &item_id, kind, Role::Reviewer)?;
        Self::write_bool(&env, &conflict_key, declared);
        item.conflict = declared;
        item.version = token.next_version;
        Self::write_item(&env, &key, &item);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &item_id,
        );
        env.events().publish(
            (symbol_short!("CQRCOI"),),
            (tenant_id, project_id, item_id, declared as u32),
        );
        Ok(token.next_version)
    }

    pub fn set_priority(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        item_id: BytesN<32>,
        priority: Priority,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        if !Self::valid_priority(priority) {
            return Err(ContractError::InvalidInput);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&item_id.to_array());
        extra.extend_from_array(&(priority as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_PRIORITY",
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
        let key = DataKey::Item(tenant_id.clone(), project_id.clone(), item_id.clone());
        let mut item: QueueItem = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if item.state == QueueState::Decided {
            return Err(ContractError::InvalidState);
        }
        Self::append_audit(
            &env,
            &tenant_id,
            &project_id,
            &item_id,
            AuditKind::PriorityChanged,
            Role::Coordinator,
        )?;
        item.priority = priority;
        item.version = token.next_version;
        Self::write_item(&env, &key, &item);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &item_id,
        );
        env.events().publish(
            (symbol_short!("CQRPRIO"),),
            (tenant_id, project_id, item_id, priority as u32),
        );
        Ok(token.next_version)
    }

    pub fn record_outcome(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        item_id: BytesN<32>,
        outcome: ReviewOutcome,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Reviewer)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&item_id.to_array());
        extra.extend_from_array(&(outcome as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP073_OUTCOME",
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
        let key = DataKey::Item(tenant_id.clone(), project_id.clone(), item_id.clone());
        let mut item: QueueItem = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if item.state != QueueState::Assigned || item.assignee.as_ref() != Some(&actor) {
            return Err(ContractError::Unauthorized);
        }
        if item.conflict {
            return Err(ContractError::ConflictOfInterest);
        }
        let reviewer_key = DataKey::Reviewer(
            tenant_id.clone(),
            project_id.clone(),
            actor.clone(),
        );
        let mut profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&reviewer_key)
            .ok_or(ContractError::NotReviewer)?;
        profile.assigned = profile
            .assigned
            .checked_sub(1)
            .ok_or(ContractError::InvalidState)?;
        profile.decided = profile
            .decided
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        Self::append_audit(
            &env,
            &tenant_id,
            &project_id,
            &item_id,
            AuditKind::Decided,
            Role::Reviewer,
        )?;
        item.state = QueueState::Decided;
        item.outcome = Some(outcome);
        item.version = token.next_version;
        Self::write_profile(&env, &reviewer_key, &profile);
        Self::write_item(&env, &key, &item);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &item_id,
        );
        env.events().publish(
            (symbol_short!("CQROUT"),),
            (tenant_id, project_id, item_id, outcome as u32),
        );
        Ok(token.next_version)
    }

    pub fn list_queue(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
        filter: QueueFilter,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        if let Some(priority) = filter.priority.as_ref() {
            if !Self::valid_priority(*priority) {
                return Err(ContractError::InvalidFilter);
            }
        }
        if filter.unassigned_only && filter.assignee.is_some() {
            return Err(ContractError::InvalidFilter);
        }
        if let Some(due_before) = filter.due_before.as_ref() {
            if *due_before == 0 {
                return Err(ContractError::InvalidFilter);
            }
        }
        let key = DataKey::QueueIndex(tenant_id.clone(), project_id.clone());
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
                let item_key = DataKey::Item(tenant_id.clone(), project_id.clone(), id.clone());
                let item: QueueItem = env
                    .storage()
                    .persistent()
                    .get(&item_key)
                    .ok_or(ContractError::NotFound)?;
                env.storage()
                    .persistent()
                    .extend_ttl(&item_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
                let priority_match = match filter.priority.as_ref() {
                    Some(priority) => item.priority == *priority,
                    None => true,
                };
                let assignee_match = match filter.assignee.as_ref() {
                    Some(assignee) => item.assignee.as_ref() == Some(assignee),
                    None => true,
                };
                let unassigned_match = !filter.unassigned_only || item.assignee.is_none();
                let due_match = match filter.due_before.as_ref() {
                    Some(due) => item.due_at != 0 && item.due_at < *due,
                    None => true,
                };
                if priority_match && assignee_match && unassigned_match && due_match {
                    page.push_back(id);
                }
            }
            cursor += 1;
        }
        Ok(page)
    }

    pub fn workload(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        reviewer: Address,
    ) -> Result<Workload, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Reviewer(tenant_id.clone(), project_id.clone(), reviewer.clone());
        let profile: ReviewerProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let index_key = DataKey::QueueIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_ids(&env, &index_key);
        let now = env.ledger().timestamp();
        let mut overdue = 0u32;
        for id in index.iter() {
            let item_key = DataKey::Item(tenant_id.clone(), project_id.clone(), id);
            if let Some(item) = env.storage().persistent().get::<DataKey, QueueItem>(&item_key) {
                if item.state != QueueState::Decided
                    && item.assignee.as_ref() == Some(&reviewer)
                    && item.due_at != 0
                    && item.due_at < now
                {
                    overdue = overdue
                        .checked_add(1)
                        .ok_or(ContractError::VersionOverflow)?;
                }
            }
        }
        Ok(Workload {
            capacity: profile.capacity,
            assigned: profile.assigned,
            decided: profile.decided,
            overdue,
            active: profile.active,
            expertise_mask: profile.expertise_mask,
        })
    }

    pub fn get_item(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        item_id: BytesN<32>,
    ) -> Result<QueueItem, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Item(tenant_id, project_id, item_id);
        let item = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(item)
    }

    pub fn get_audit(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        item_id: BytesN<32>,
    ) -> Result<Vec<AuditEntry>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Audit(tenant_id, project_id, item_id);
        let trail = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(trail)
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
        let roles = [Role::Coordinator, Role::Reviewer, Role::Observer];
        for role in roles.iter() {
            if Self::has_role(env, tenant_id, viewer, *role) {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    fn valid_priority(priority: Priority) -> bool {
        priority == Priority::Low
            || priority == Priority::Normal
            || priority == Priority::High
            || priority == Priority::Urgent
    }

    fn append_audit(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        item_id: &BytesN<32>,
        kind: AuditKind,
        actor_role: Role,
    ) -> Result<(), ContractError> {
        let key = DataKey::Audit(
            tenant_id.clone(),
            project_id.clone(),
            item_id.clone(),
        );
        let mut trail: Vec<AuditEntry> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if trail.len() >= MAX_AUDIT {
            return Err(ContractError::CollectionFull);
        }
        trail.push_back(AuditEntry {
            kind,
            actor_role,
            at: env.ledger().timestamp(),
        });
        Self::write_audit(env, &key, &trail);
        Ok(())
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

    fn write_profile(env: &Env, key: &DataKey, value: &ReviewerProfile) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_item(env: &Env, key: &DataKey, value: &QueueItem) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_audit(env: &Env, key: &DataKey, value: &Vec<AuditEntry>) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
