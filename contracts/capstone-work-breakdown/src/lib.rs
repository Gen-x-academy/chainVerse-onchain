#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_TASKS: u32 = 64;
const MAX_DEPENDENCIES: u32 = 16;
const TOTAL_WEIGHT_BPS: u32 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    PackageExists = 5,
    PackageNotFound = 6,
    TaskExists = 7,
    TaskNotFound = 8,
    InvalidOrder = 9,
    InvalidWeight = 10,
    InvalidEstimate = 11,
    InvalidDependency = 12,
    DuplicateOrder = 13,
    DependencyLimitReached = 14,
    TaskLimitReached = 15,
    InvalidState = 16,
    TerminalState = 17,
    VersionMismatch = 18,
    LimitReached = 19,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Planner = 1,
    Participant = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    Planned = 1,
    InProgress = 2,
    Blocked = 3,
    Done = 4,
    Cancelled = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub package_id: BytesN<32>,
    pub planned_weights: Vec<u32>,
    pub task_count: u32,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Task {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub package_id: BytesN<32>,
    pub task_id: BytesN<32>,
    pub order: u32,
    pub spec_hash: BytesN<32>,
    pub dependencies: Vec<BytesN<32>>,
    pub estimate_seconds: u64,
    pub acceptance_hash: BytesN<32>,
    pub assignee: Address,
    pub progress_weight_bps: u32,
    pub state: TaskState,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTaskInput {
    pub order: u32,
    pub spec_hash: BytesN<32>,
    pub dependencies: Vec<BytesN<32>>,
    pub estimate_seconds: u64,
    pub acceptance_hash: BytesN<32>,
    pub assignee: Address,
    pub planned_weights: Vec<u32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateTransitionInput {
    pub state: TaskState,
    pub acceptance_hash: BytesN<32>,
    pub assignee: Address,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Package(BytesN<32>, BytesN<32>, BytesN<32>),
    Task(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    Order(BytesN<32>, BytesN<32>, BytesN<32>, u32),
}

#[contract]
pub struct CapstoneWorkBreakdownContract;

#[contractimpl]
impl CapstoneWorkBreakdownContract {
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
            (symbol_short!("WBK_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn create_package(
        env: Env,
        planner: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        package_id: BytesN<32>,
        planned_weights: Vec<u32>,
        expected_version: u32,
    ) -> Result<Package, ContractError> {
        planner.require_auth();
        Self::require_role(&env, &planner, &tenant_id, &project_id, Role::Planner)?;
        let key = DataKey::Package(tenant_id.clone(), project_id.clone(), package_id.clone());
        if let Some(existing) = env.storage().persistent().get::<_, Package>(&key) {
            if existing.planned_weights == planned_weights {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::PackageExists);
        }
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        Self::validate_weights(&planned_weights)?;
        let now = env.ledger().timestamp();
        let package = Package {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            package_id: package_id.clone(),
            planned_weights,
            task_count: 0,
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &package);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("WBK_PKG"),),
            (tenant_id, project_id, package_id, now),
        );
        Ok(package)
    }

    pub fn create_task(
        env: Env,
        planner: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        package_id: BytesN<32>,
        task_id: BytesN<32>,
        input: CreateTaskInput,
    ) -> Result<Task, ContractError> {
        planner.require_auth();
        Self::require_role(&env, &planner, &tenant_id, &project_id, Role::Planner)?;
        let package_key = DataKey::Package(tenant_id.clone(), project_id.clone(), package_id.clone());
        let mut package = Self::load_package(&env, &package_key)?;
        let key = DataKey::Task(
            tenant_id.clone(),
            project_id.clone(),
            package_id.clone(),
            task_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, Task>(&key) {
            let weight = Self::weight_at(&package.planned_weights, input.order)?;
            if input.expected_version == 0
                && existing.order == input.order
                && existing.spec_hash == input.spec_hash
                && existing.dependencies == input.dependencies
                && existing.estimate_seconds == input.estimate_seconds
                && existing.acceptance_hash == input.acceptance_hash
                && existing.assignee == input.assignee
                && existing.progress_weight_bps == weight
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if input.expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::TaskExists);
        }
        if input.expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        Self::validate_weights(&input.planned_weights)?;
        if input.planned_weights != package.planned_weights {
            return Err(ContractError::InvalidWeight);
        }
        if package.task_count >= MAX_TASKS || package.task_count >= package.planned_weights.len() {
            return Err(ContractError::TaskLimitReached);
        }
        if input.order == 0
            || input.order > package.planned_weights.len()
            || input.order > package.task_count + 1
        {
            return Err(ContractError::InvalidOrder);
        }
        let order_key = DataKey::Order(
            tenant_id.clone(),
            project_id.clone(),
            package_id.clone(),
            input.order,
        );
        if env.storage().persistent().has(&order_key) {
            return Err(ContractError::DuplicateOrder);
        }
        if input.estimate_seconds == 0 {
            return Err(ContractError::InvalidEstimate);
        }
        Self::validate_dependencies(
            &env,
            &tenant_id,
            &project_id,
            &package_id,
            &task_id,
            &input.dependencies,
        )?;
        let weight = Self::weight_at(&package.planned_weights, input.order)?;
        let now = env.ledger().timestamp();
        let task = Task {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            package_id: package_id.clone(),
            task_id: task_id.clone(),
            order: input.order,
            spec_hash: input.spec_hash,
            dependencies: input.dependencies,
            estimate_seconds: input.estimate_seconds,
            acceptance_hash: input.acceptance_hash,
            assignee: input.assignee,
            progress_weight_bps: weight,
            state: TaskState::Planned,
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &task);
        env.storage().persistent().set(&order_key, &task_id);
        package.task_count = package
            .task_count
            .checked_add(1)
            .ok_or(ContractError::LimitReached)?;
        package.expected_version = package
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        package.updated_at = now;
        env.storage().persistent().set(&package_key, &package);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &order_key);
        Self::extend_persistent(&env, &package_key);
        env.events().publish(
            (symbol_short!("WBK_TASK"),),
            (tenant_id, project_id, package_id, task_id, input.order),
        );
        Ok(task)
    }

    pub fn set_state(
        env: Env,
        caller: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        package_id: BytesN<32>,
        task_id: BytesN<32>,
        input: StateTransitionInput,
    ) -> Result<Task, ContractError> {
        caller.require_auth();
        let key = DataKey::Task(
            tenant_id.clone(),
            project_id.clone(),
            package_id.clone(),
            task_id.clone(),
        );
        let mut task = Self::load_task(&env, &key)?;
        if caller != task.assignee {
            Self::require_role(&env, &caller, &tenant_id, &project_id, Role::Planner)?;
        }
        if task.state == input.state
            && task.acceptance_hash == input.acceptance_hash
            && task.assignee == input.assignee
        {
            Self::extend_persistent(&env, &key);
            return Ok(task);
        }
        if input.expected_version != task.expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if Self::is_terminal(task.state) {
            return Err(ContractError::TerminalState);
        }
        if input.state == TaskState::Planned && task.state != TaskState::Blocked {
            return Err(ContractError::InvalidState);
        }
        if input.state == TaskState::Done && task.state != TaskState::InProgress {
            return Err(ContractError::InvalidState);
        }
        task.state = input.state;
        task.acceptance_hash = input.acceptance_hash.clone();
        task.assignee = input.assignee.clone();
        task.expected_version = task
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        task.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &task);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("WBK_STATE"),),
            (tenant_id, project_id, package_id, task_id, input.state),
        );
        Ok(task)
    }

    pub fn get_package(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        package_id: BytesN<32>,
    ) -> Result<Package, ContractError> {
        let key = DataKey::Package(tenant_id, project_id, package_id);
        Self::load_package(&env, &key)
    }

    pub fn get_task(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        package_id: BytesN<32>,
        task_id: BytesN<32>,
    ) -> Result<Task, ContractError> {
        let key = DataKey::Task(tenant_id, project_id, package_id, task_id);
        Self::load_task(&env, &key)
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

    fn load_package(env: &Env, key: &DataKey) -> Result<Package, ContractError> {
        let package: Package = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::PackageNotFound)?;
        Self::extend_persistent(env, key);
        Ok(package)
    }

    fn load_task(env: &Env, key: &DataKey) -> Result<Task, ContractError> {
        let task: Task = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::TaskNotFound)?;
        Self::extend_persistent(env, key);
        Ok(task)
    }

    fn validate_weights(weights: &Vec<u32>) -> Result<(), ContractError> {
        if weights.is_empty() || weights.len() > MAX_TASKS {
            return Err(ContractError::InvalidWeight);
        }
        let mut total: u32 = 0;
        for i in 0..weights.len() {
            let weight = weights.get(i).ok_or(ContractError::InvalidWeight)?;
            if weight == 0 {
                return Err(ContractError::InvalidWeight);
            }
            total = total
                .checked_add(weight)
                .ok_or(ContractError::InvalidWeight)?;
        }
        if total != TOTAL_WEIGHT_BPS {
            return Err(ContractError::InvalidWeight);
        }
        Ok(())
    }

    fn weight_at(weights: &Vec<u32>, order: u32) -> Result<u32, ContractError> {
        if order == 0 {
            return Err(ContractError::InvalidOrder);
        }
        weights
            .get(order - 1)
            .filter(|weight| *weight > 0)
            .ok_or(ContractError::InvalidOrder)
    }

    fn validate_dependencies(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        package_id: &BytesN<32>,
        task_id: &BytesN<32>,
        dependencies: &Vec<BytesN<32>>,
    ) -> Result<(), ContractError> {
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(ContractError::DependencyLimitReached);
        }
        let mut seen: Vec<BytesN<32>> = Vec::new(env);
        for i in 0..dependencies.len() {
            let dependency = dependencies.get(i).ok_or(ContractError::InvalidDependency)?;
            if dependency == *task_id {
                return Err(ContractError::InvalidDependency);
            }
            if seen.contains(dependency.clone()) {
                return Err(ContractError::InvalidDependency);
            }
            let key = DataKey::Task(
                tenant_id.clone(),
                project_id.clone(),
                package_id.clone(),
                dependency.clone(),
            );
            if !env.storage().persistent().has(&key) {
                return Err(ContractError::InvalidDependency);
            }
            Self::extend_persistent(env, &key);
            seen.push_back(dependency);
        }
        Ok(())
    }

    fn is_terminal(state: TaskState) -> bool {
        state == TaskState::Done || state == TaskState::Cancelled
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
