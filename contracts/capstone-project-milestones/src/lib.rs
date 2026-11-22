#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_MILESTONES: u32 = 64;
const MAX_DEPENDENCIES: u32 = 16;
const MAX_DELIVERABLES: u32 = 16;
const MAX_EVIDENCE: u32 = 16;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    MilestoneExists = 5,
    MilestoneNotFound = 6,
    DependencyNotFound = 7,
    CrossProjectDependency = 8,
    IncompleteDependency = 9,
    CycleDetected = 10,
    InvalidOrder = 11,
    InvalidDependency = 12,
    DependencyLimitReached = 13,
    DeliverableLimitReached = 14,
    EvidenceLimitReached = 15,
    DuplicateOrder = 16,
    WrongReviewer = 17,
    AlreadyCompleted = 18,
    MissingEvidence = 19,
    VersionMismatch = 20,
    LimitReached = 21,
    InvalidDueDate = 22,
    InvalidFundingRef = 23,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Planner = 1,
    Participant = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneState {
    Pending = 1,
    Completed = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionRule {
    AllDeliverables = 1,
    Reviewer = 2,
    ExternalEvidence = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Milestone {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub milestone_id: BytesN<32>,
    pub order: u32,
    pub description_hash: BytesN<32>,
    pub due_at: u64,
    pub dependencies: Vec<BytesN<32>>,
    pub reviewer: Address,
    pub deliverable_hashes: Vec<BytesN<32>>,
    pub completion_rule: CompletionRule,
    pub funding_ref: Option<BytesN<32>>,
    pub state: MilestoneState,
    pub evidence_hash: Option<BytesN<32>>,
    pub completed_at: Option<u64>,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateMilestoneInput {
    pub order: u32,
    pub description_hash: BytesN<32>,
    pub due_at: u64,
    pub dependencies: Vec<BytesN<32>>,
    pub reviewer: Address,
    pub deliverable_hashes: Vec<BytesN<32>>,
    pub completion_rule: CompletionRule,
    pub funding_ref: Option<BytesN<32>>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Milestone(BytesN<32>, BytesN<32>, BytesN<32>),
    Order(BytesN<32>, BytesN<32>, u32),
    MilestoneProject(BytesN<32>, BytesN<32>),
    ProjectCount(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneProjectMilestonesContract;

#[contractimpl]
impl CapstoneProjectMilestonesContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
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
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().set(&key, &active);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CPM_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn create_milestone(
        env: Env,
        planner: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        input: CreateMilestoneInput,
    ) -> Result<Milestone, ContractError> {
        planner.require_auth();
        Self::require_role(&env, &planner, &tenant_id, &project_id, Role::Planner)?;
        let key = DataKey::Milestone(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, Milestone>(&key) {
            if input.expected_version == 0
                && existing.order == input.order
                && existing.description_hash == input.description_hash
                && existing.due_at == input.due_at
                && existing.dependencies == input.dependencies
                && existing.reviewer == input.reviewer
                && existing.deliverable_hashes == input.deliverable_hashes
                && existing.completion_rule == input.completion_rule
                && existing.funding_ref == input.funding_ref
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if input.expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::MilestoneExists);
        }
        if input.expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        if input.due_at == 0 {
            return Err(ContractError::InvalidDueDate);
        }
        if input.dependencies.len() > MAX_DEPENDENCIES {
            return Err(ContractError::DependencyLimitReached);
        }
        if input.deliverable_hashes.len() > MAX_DELIVERABLES {
            return Err(ContractError::DeliverableLimitReached);
        }
        if let Some(funding_ref) = input.funding_ref.clone() {
            if funding_ref == BytesN::from_array(&env, &[0u8; 32]) {
                return Err(ContractError::InvalidFundingRef);
            }
        }
        let count_key = DataKey::ProjectCount(tenant_id.clone(), project_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_MILESTONES || input.order == 0 || input.order > count + 1 {
            return Err(ContractError::InvalidOrder);
        }
        let order_key = DataKey::Order(tenant_id.clone(), project_id.clone(), input.order);
        if env.storage().persistent().has(&order_key) {
            return Err(ContractError::DuplicateOrder);
        }
        let global_key = DataKey::MilestoneProject(tenant_id.clone(), milestone_id.clone());
        if env.storage().persistent().has(&global_key) {
            return Err(ContractError::MilestoneExists);
        }
        let mut seen: Vec<BytesN<32>> = Vec::new(&env);
        for i in 0..input.dependencies.len() {
            let dependency = input
                .dependencies
                .get(i)
                .ok_or(ContractError::InvalidDependency)?;
            if dependency == milestone_id {
                return Err(ContractError::CycleDetected);
            }
            if seen.contains(dependency.clone()) {
                return Err(ContractError::InvalidDependency);
            }
            seen.push_back(dependency.clone());
            let project_key = DataKey::MilestoneProject(tenant_id.clone(), dependency.clone());
            let dependency_project: BytesN<32> = env
                .storage()
                .persistent()
                .get(&project_key)
                .ok_or(ContractError::DependencyNotFound)?;
            if dependency_project != project_id {
                return Err(ContractError::CrossProjectDependency);
            }
            let dependency_key = DataKey::Milestone(
                tenant_id.clone(),
                project_id.clone(),
                dependency.clone(),
            );
            let dependency_milestone: Milestone = env
                .storage()
                .persistent()
                .get(&dependency_key)
                .ok_or(ContractError::DependencyNotFound)?;
            if dependency_milestone.state != MilestoneState::Completed {
                return Err(ContractError::IncompleteDependency);
            }
            Self::extend_persistent(&env, &dependency_key);
            Self::extend_persistent(&env, &project_key);
        }
        if Self::would_create_cycle(
            &env,
            &tenant_id,
            &project_id,
            &input.dependencies,
            &milestone_id,
        ) {
            return Err(ContractError::CycleDetected);
        }
        let now = env.ledger().timestamp();
        let milestone = Milestone {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            milestone_id: milestone_id.clone(),
            order: input.order,
            description_hash: input.description_hash,
            due_at: input.due_at,
            dependencies: input.dependencies,
            reviewer: input.reviewer,
            deliverable_hashes: input.deliverable_hashes,
            completion_rule: input.completion_rule,
            funding_ref: input.funding_ref,
            state: MilestoneState::Pending,
            evidence_hash: None,
            completed_at: None,
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &milestone);
        env.storage().persistent().set(&order_key, &milestone_id);
        env.storage().persistent().set(&global_key, &project_id);
        env.storage()
            .persistent()
            .set(&count_key, &count.checked_add(1).ok_or(ContractError::LimitReached)?);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &order_key);
        Self::extend_persistent(&env, &global_key);
        Self::extend_persistent(&env, &count_key);
        env.events().publish(
            (symbol_short!("CPM_MILE"),),
            (tenant_id, project_id, milestone_id, input.order),
        );
        Ok(milestone)
    }

    pub fn complete_milestone(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        evidence_hashes: Vec<BytesN<32>>,
        expected_version: u32,
    ) -> Result<Milestone, ContractError> {
        reviewer.require_auth();
        let key = DataKey::Milestone(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        let mut milestone = Self::load_milestone(&env, &key)?;
        if reviewer != milestone.reviewer {
            return Err(ContractError::WrongReviewer);
        }
        if evidence_hashes.len() > MAX_EVIDENCE {
            return Err(ContractError::EvidenceLimitReached);
        }
        let evidence_hash = Self::hash_hashes(&env, &evidence_hashes);
        if milestone.state == MilestoneState::Completed {
            if milestone.evidence_hash == Some(evidence_hash.clone()) {
                Self::extend_persistent(&env, &key);
                return Ok(milestone);
            }
            if expected_version != milestone.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::AlreadyCompleted);
        }
        if expected_version != milestone.expected_version {
            return Err(ContractError::VersionMismatch);
        }
        match milestone.completion_rule {
            CompletionRule::AllDeliverables => {
                if milestone.deliverable_hashes.is_empty() {
                    return Err(ContractError::MissingEvidence);
                }
                for i in 0..milestone.deliverable_hashes.len() {
                    let deliverable = milestone
                        .deliverable_hashes
                        .get(i)
                        .ok_or(ContractError::MissingEvidence)?;
                    if !evidence_hashes.contains(deliverable.clone()) {
                        return Err(ContractError::MissingEvidence);
                    }
                }
            }
            CompletionRule::Reviewer => {}
            CompletionRule::ExternalEvidence => {
                if evidence_hashes.is_empty() {
                    return Err(ContractError::MissingEvidence);
                }
            }
        }
        milestone.state = MilestoneState::Completed;
        milestone.evidence_hash = Some(evidence_hash);
        milestone.completed_at = Some(env.ledger().timestamp());
        milestone.expected_version = milestone
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        milestone.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &milestone);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CPM_DONE"),),
            (tenant_id, project_id, milestone_id, milestone.completed_at),
        );
        Ok(milestone)
    }

    pub fn get_milestone(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
    ) -> Result<Milestone, ContractError> {
        let key = DataKey::Milestone(tenant_id, project_id, milestone_id);
        Self::load_milestone(&env, &key)
    }

    pub fn version(_env: Env) -> u32 {
        SCHEMA_VERSION
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
        Ok(())
    }

    fn require_role(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !active {
            return Err(ContractError::Unauthorized);
        }
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn load_milestone(env: &Env, key: &DataKey) -> Result<Milestone, ContractError> {
        let milestone: Milestone = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::MilestoneNotFound)?;
        Self::extend_persistent(env, key);
        Ok(milestone)
    }

    fn would_create_cycle(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        dependencies: &Vec<BytesN<32>>,
        milestone_id: &BytesN<32>,
    ) -> bool {
        let mut stack: Vec<BytesN<32>> = Vec::new(env);
        let mut seen: Vec<BytesN<32>> = Vec::new(env);
        for i in 0..dependencies.len() {
            if let Some(dependency) = dependencies.get(i) {
                stack.push_back(dependency);
            }
        }
        while let Some(current) = stack.pop_back() {
            if current == *milestone_id {
                return true;
            }
            if seen.contains(current.clone()) {
                continue;
            }
            seen.push_back(current.clone());
            let key = DataKey::Milestone(tenant_id.clone(), project_id.clone(), current);
            if let Some(milestone) = env.storage().persistent().get::<_, Milestone>(&key) {
                for i in 0..milestone.dependencies.len() {
                    if let Some(dependency) = milestone.dependencies.get(i) {
                        stack.push_back(dependency);
                    }
                }
            }
        }
        false
    }

    fn hash_hashes(env: &Env, hashes: &Vec<BytesN<32>>) -> BytesN<32> {
        let mut buffer = Bytes::new(env);
        buffer.extend_from_slice(&(hashes.len() as u64).to_le_bytes());
        for i in 0..hashes.len() {
            if let Some(item) = hashes.get(i) {
                buffer.extend_from_array(&item.to_array());
            }
        }
        env.crypto().sha256(&buffer).to_bytes()
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
