#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const MAX_DECISIONS: u32 = 256;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    DecisionExists = 4,
    NotFound = 5,
    VersionMismatch = 6,
    LimitReached = 7,
    InvalidState = 8,
    AlreadyDecided = 9,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Reviewer = 1,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionKind {
    RequestChanges = 1,
    Accept = 2,
    Reject = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcceptanceStatus {
    Pending = 1,
    Decided = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliverableDecision {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub milestone_id: BytesN<32>,
    pub deliverable_id: BytesN<32>,
    pub submission_version: u32,
    pub rubric_hash: BytesN<32>,
    pub reason_hash: BytesN<32>,
    pub decision: DecisionKind,
    pub status: AcceptanceStatus,
    pub reviewer: Address,
    pub decided_at: u64,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Count(BytesN<32>, BytesN<32>, BytesN<32>),
    Decision(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneDeliverableAcceptanceContract;

#[contractimpl]
impl CapstoneDeliverableAcceptanceContract {
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
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("CDA_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn decide(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        deliverable_id: BytesN<32>,
        submission_version: u32,
        rubric_hash: BytesN<32>,
        reason_hash: BytesN<32>,
        decision: DecisionKind,
        expected_version: u32,
    ) -> Result<DeliverableDecision, ContractError> {
        reviewer.require_auth();
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        let key = DataKey::Decision(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
            deliverable_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, DeliverableDecision>(&key) {
            if existing.submission_version == submission_version
                && existing.rubric_hash == rubric_hash
                && existing.reason_hash == reason_hash
                && existing.decision == decision
                && existing.reviewer == reviewer
            {
                Self::extend(&env, &key);
                return Ok(existing);
            }
            return Err(ContractError::AlreadyDecided);
        }
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let count_key = DataKey::Count(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_DECISIONS {
            return Err(ContractError::LimitReached);
        }
        let record = DeliverableDecision {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            milestone_id: milestone_id.clone(),
            deliverable_id: deliverable_id.clone(),
            submission_version,
            rubric_hash: rubric_hash.clone(),
            reason_hash: reason_hash.clone(),
            decision,
            status: AcceptanceStatus::Decided,
            reviewer,
            decided_at: env.ledger().timestamp(),
            expected_version: 1,
        };
        env.storage().persistent().set(&key, &record);
        env.storage()
            .persistent()
            .set(&count_key, &(count.checked_add(1).ok_or(ContractError::LimitReached)?));
        Self::extend(&env, &key);
        Self::extend(&env, &count_key);
        env.events().publish(
            (symbol_short!("CDA_DEC"),),
            (
                tenant_id,
                project_id,
                milestone_id,
                deliverable_id,
                submission_version,
                decision,
            ),
        );
        Ok(record)
    }

    pub fn get_decision(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        deliverable_id: BytesN<32>,
    ) -> Option<DeliverableDecision> {
        env.storage().persistent().get(&DataKey::Decision(
            tenant_id,
            project_id,
            milestone_id,
            deliverable_id,
        ))
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
        Self::extend(env, &key);
        Ok(())
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
