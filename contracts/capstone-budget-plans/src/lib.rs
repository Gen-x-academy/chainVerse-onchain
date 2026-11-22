#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_LINES: u32 = 16;
const MAX_APPROVALS: u32 = 8;
const MAX_VERSIONS: u32 = 8;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    WrongTenant = 4,
    NotFound = 5,
    InvalidExpiry = 6,
    InvalidAmount = 7,
    EmptyLines = 8,
    TooManyLines = 9,
    InvalidThreshold = 10,
    VersionConflict = 11,
    AlreadyExists = 12,
    RequestConflict = 13,
    AlreadyApproved = 14,
    InvalidState = 15,
    Expired = 16,
    Overflow = 17,
    CapacityExceeded = 18,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Reviewer = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetState {
    Draft = 1,
    Submitted = 2,
    Approved = 3,
    Superseded = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetLine {
    pub category_hash: BytesN<32>,
    pub justification_hash: BytesN<32>,
    pub funding_source_hash: BytesN<32>,
    pub quantity: u32,
    pub unit_amount: i128,
    pub line_total: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetPlan {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub version: u32,
    pub state: BudgetState,
    pub lines_hash: BytesN<32>,
    pub total_amount: i128,
    pub approval_threshold: u32,
    pub approval_count: u32,
    pub request_id: BytesN<32>,
    pub request_hash: BytesN<32>,
    pub created_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestMarker {
    pub base_version: u32,
    pub target_version: u32,
    pub request_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    Plan(BytesN<32>, BytesN<32>),
    History(BytesN<32>, BytesN<32>, u32),
    Lines(BytesN<32>, BytesN<32>, u32),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Approval(BytesN<32>, BytesN<32>, u32, Address),
    ApprovalRequest(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneBudgetPlansContract;

#[contractimpl]
impl CapstoneBudgetPlansContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
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
        let key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        env.storage().persistent().set(&key, &true);
        Self::extend(&env, &key);
        env.events()
            .publish((symbol_short!("CBPROLG"),), (tenant_id, account, role));
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
        env.storage().persistent().set(&key, &false);
        Self::extend(&env, &key);
        env.events()
            .publish((symbol_short!("CBPRVRK"),), (tenant_id, account, role));
        Ok(())
    }

    pub fn has_role(env: Env, tenant_id: BytesN<32>, account: Address, role: Role) -> bool {
        let key = DataKey::Role(tenant_id, account, role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend(&env, &key);
        }
        active
    }

    pub fn propose_plan(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        request_hash: BytesN<32>,
        lines: Vec<BudgetLine>,
        lines_hash: BytesN<32>,
        approval_threshold: u32,
        expires_at: u64,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &coordinator, &tenant_id, Role::Coordinator)?;
        if expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        if approval_threshold == 0 || approval_threshold > MAX_APPROVALS {
            return Err(ContractError::InvalidThreshold);
        }
        let total_amount = Self::validate_lines(&lines)?;
        let request_key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != expected_version || marker.request_hash != request_hash {
                return Err(ContractError::RequestConflict);
            }
            return Ok(marker.target_version);
        }
        let plan_key = DataKey::Plan(tenant_id.clone(), project_id.clone());
        let current: Option<BudgetPlan> = env.storage().persistent().get(&plan_key);
        let next_version = match current {
            None => {
                if expected_version != 0 {
                    return Err(ContractError::VersionConflict);
                }
                1u32
            }
            Some(plan) => {
                if plan.tenant_id != tenant_id {
                    return Err(ContractError::WrongTenant);
                }
                if plan.version != expected_version {
                    return Err(ContractError::VersionConflict);
                }
                if plan.state != BudgetState::Approved {
                    return Err(ContractError::InvalidState);
                }
                let next = plan.version.checked_add(1).ok_or(ContractError::Overflow)?;
                if next > MAX_VERSIONS {
                    return Err(ContractError::CapacityExceeded);
                }
                let history_key =
                    DataKey::History(tenant_id.clone(), project_id.clone(), plan.version);
                env.storage().persistent().set(&history_key, &plan);
                Self::extend(&env, &history_key);
                next
            }
        };
        let plan = BudgetPlan {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            version: next_version,
            state: BudgetState::Draft,
            lines_hash: lines_hash.clone(),
            total_amount,
            approval_threshold,
            approval_count: 0,
            request_id: request_id.clone(),
            request_hash: request_hash.clone(),
            created_at: env.ledger().timestamp(),
            expires_at,
        };
        env.storage().persistent().set(&plan_key, &plan);
        Self::extend(&env, &plan_key);
        let lines_key = DataKey::Lines(tenant_id.clone(), project_id.clone(), next_version);
        env.storage().persistent().set(&lines_key, &lines);
        Self::extend(&env, &lines_key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: next_version,
            request_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBPPLN"),),
            (tenant_id, project_id, next_version, total_amount),
        );
        Ok(next_version)
    }

    pub fn submit_plan(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &coordinator, &tenant_id, Role::Coordinator)?;
        let key = DataKey::Plan(tenant_id.clone(), project_id.clone());
        let mut plan: BudgetPlan = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if plan.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if plan.version != expected_version {
            return Err(ContractError::VersionConflict);
        }
        if plan.state != BudgetState::Draft {
            return Err(ContractError::InvalidState);
        }
        if plan.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::Expired);
        }
        plan.state = BudgetState::Submitted;
        env.storage().persistent().set(&key, &plan);
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("CBPSUB"),),
            (tenant_id, project_id, expected_version),
        );
        Ok(())
    }

    pub fn approve_plan(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        request_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, Role::Reviewer)?;
        let key = DataKey::Plan(tenant_id.clone(), project_id.clone());
        let mut plan: BudgetPlan = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if plan.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if plan.version != expected_version {
            return Err(ContractError::VersionConflict);
        }
        if plan.state != BudgetState::Submitted {
            return Err(ContractError::InvalidState);
        }
        if plan.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::Expired);
        }
        let request_key = DataKey::ApprovalRequest(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != expected_version || marker.request_hash != request_hash {
                return Err(ContractError::RequestConflict);
            }
            return Ok(());
        }
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            plan.version,
            reviewer.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::AlreadyApproved);
        }
        if plan.approval_count >= plan.approval_threshold {
            return Err(ContractError::InvalidState);
        }
        let approval_count = plan
            .approval_count
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        env.storage().persistent().set(&approval_key, &true);
        Self::extend(&env, &approval_key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: plan.version,
            request_hash: request_hash.clone(),
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        plan.approval_count = approval_count;
        if approval_count == plan.approval_threshold {
            plan.state = BudgetState::Approved;
        }
        env.storage().persistent().set(&key, &plan);
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("CBPAPP"),),
            (tenant_id, project_id, plan.version, approval_count),
        );
        Ok(())
    }

    pub fn get_plan(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<BudgetPlan, ContractError> {
        Self::load_plan(&env, &tenant_id, &project_id)
    }

    pub fn get_lines(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        version: u32,
    ) -> Result<Vec<BudgetLine>, ContractError> {
        let key = DataKey::Lines(tenant_id, project_id, version);
        let lines = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(lines)
    }

    pub fn get_history(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        version: u32,
    ) -> Option<BudgetPlan> {
        env.storage()
            .persistent()
            .get(&DataKey::History(tenant_id, project_id, version))
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &configured != admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let key = DataKey::Role(tenant_id.clone(), caller.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend(env, &key);
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn load_plan(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<BudgetPlan, ContractError> {
        let key = DataKey::Plan(tenant_id.clone(), project_id.clone());
        let plan: BudgetPlan = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if plan.tenant_id != tenant_id.clone() {
            return Err(ContractError::WrongTenant);
        }
        Self::extend(env, &key);
        Ok(plan)
    }

    fn validate_lines(lines: &Vec<BudgetLine>) -> Result<i128, ContractError> {
        let len = lines.len();
        if len == 0 {
            return Err(ContractError::EmptyLines);
        }
        if len > MAX_LINES {
            return Err(ContractError::TooManyLines);
        }
        let mut total = 0i128;
        let mut index = 0u32;
        while index < len {
            let line = lines.get(index).ok_or(ContractError::NotFound)?;
            if line.quantity == 0 || line.unit_amount <= 0 {
                return Err(ContractError::InvalidAmount);
            }
            let expected = i128::from(line.quantity)
                .checked_mul(line.unit_amount)
                .ok_or(ContractError::Overflow)?;
            if line.line_total != expected {
                return Err(ContractError::InvalidAmount);
            }
            total = total.checked_add(expected).ok_or(ContractError::Overflow)?;
            index += 1;
        }
        Ok(total)
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
