#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const MAX_DELIVERABLES: u32 = 256;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    AlreadySubmitted = 4,
    NotFound = 5,
    VersionMismatch = 6,
    LimitReached = 7,
    InvalidVersion = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Submitter = 1,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliverableKind {
    File = 1,
    Link = 2,
    Dataset = 3,
    Commit = 4,
    Demonstration = 5,
    Text = 6,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deliverable {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub milestone_id: BytesN<32>,
    pub deliverable_id: BytesN<32>,
    pub version: u32,
    pub kind: DeliverableKind,
    pub content_hash: BytesN<32>,
    pub requirement_hash: BytesN<32>,
    pub submitter: Address,
    pub submitted_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Count(BytesN<32>, BytesN<32>, BytesN<32>),
    Deliverable(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    Latest(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneMilestoneDeliverablesContract;

#[contractimpl]
impl CapstoneMilestoneDeliverablesContract {
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
            (symbol_short!("CMD_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn submit_deliverable(
        env: Env,
        submitter: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        deliverable_id: BytesN<32>,
        version: u32,
        kind: DeliverableKind,
        content_hash: BytesN<32>,
        requirement_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<Deliverable, ContractError> {
        submitter.require_auth();
        Self::require_role(&env, &submitter, &tenant_id, &project_id, Role::Submitter)?;
        let latest_key = DataKey::Latest(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        let latest: u32 = env.storage().persistent().get(&latest_key).unwrap_or(0);
        let next = latest.checked_add(1).ok_or(ContractError::InvalidVersion)?;
        if version != next || expected_version != latest {
            return Err(ContractError::VersionMismatch);
        }
        let key = DataKey::Deliverable(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
            deliverable_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, Deliverable>(&key) {
            if existing.version == version
                && existing.kind == kind
                && existing.content_hash == content_hash
                && existing.requirement_hash == requirement_hash
                && existing.submitter == submitter
            {
                Self::extend(&env, &key);
                Self::extend(&env, &latest_key);
                return Ok(existing);
            }
            return Err(ContractError::AlreadySubmitted);
        }
        let count_key = DataKey::Count(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_DELIVERABLES {
            return Err(ContractError::LimitReached);
        }
        let deliverable = Deliverable {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            milestone_id: milestone_id.clone(),
            deliverable_id: deliverable_id.clone(),
            version,
            kind,
            content_hash: content_hash.clone(),
            requirement_hash,
            submitter,
            submitted_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &deliverable);
        env.storage().persistent().set(&latest_key, &version);
        env.storage()
            .persistent()
            .set(&count_key, &(count.checked_add(1).ok_or(ContractError::LimitReached)?));
        Self::extend(&env, &key);
        Self::extend(&env, &latest_key);
        Self::extend(&env, &count_key);
        env.events().publish(
            (symbol_short!("CMD_SUB"),),
            (tenant_id, project_id, milestone_id, deliverable_id, version, kind),
        );
        Ok(deliverable)
    }

    pub fn get_deliverable(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        deliverable_id: BytesN<32>,
    ) -> Option<Deliverable> {
        env.storage().persistent().get(&DataKey::Deliverable(
            tenant_id,
            project_id,
            milestone_id,
            deliverable_id,
        ))
    }

    pub fn latest_version(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
    ) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Latest(tenant_id, project_id, milestone_id))
            .unwrap_or(0)
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
