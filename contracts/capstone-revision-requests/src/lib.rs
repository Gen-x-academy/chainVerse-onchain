#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const MAX_REVISION_REQUESTS: u32 = 128;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    RequestExists = 5,
    RequestNotFound = 6,
    DeadlineExpired = 7,
    InvalidState = 8,
    VersionMismatch = 9,
    LimitReached = 10,
    InvalidDeadline = 11,
    AlreadyResponded = 12,
    ResubmissionNotAllowed = 13,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Requester = 1,
    Responder = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionStatus {
    Open = 1,
    Responded = 2,
    Resubmitted = 3,
    Closed = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionRequest {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub proposal_version: u32,
    pub requirement_hash: BytesN<32>,
    pub deadline: u64,
    pub requester: Address,
    pub responder: Address,
    pub status: RevisionStatus,
    pub response_hash: Option<BytesN<32>>,
    pub resubmission_version: u32,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Count(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneRevisionRequestsContract;

#[contractimpl]
impl CapstoneRevisionRequestsContract {
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
            (symbol_short!("CRR_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn request_revision(
        env: Env,
        requester: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        proposal_version: u32,
        requirement_hash: BytesN<32>,
        deadline: u64,
        responder: Address,
        expected_version: u32,
    ) -> Result<RevisionRequest, ContractError> {
        requester.require_auth();
        Self::require_role(&env, &requester, &tenant_id, &project_id, Role::Requester)?;
        Self::require_role(&env, &responder, &tenant_id, &project_id, Role::Responder)?;
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let now = env.ledger().timestamp();
        if deadline <= now {
            return Err(ContractError::InvalidDeadline);
        }
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RevisionRequest>(&key)
        {
            if existing.requester == requester
                && existing.proposal_version == proposal_version
                && existing.requirement_hash == requirement_hash
                && existing.deadline == deadline
                && existing.responder == responder
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            return Err(ContractError::RequestExists);
        }
        let count_key = DataKey::Count(tenant_id.clone(), project_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_REVISION_REQUESTS {
            return Err(ContractError::LimitReached);
        }
        let next_count = count.checked_add(1).ok_or(ContractError::LimitReached)?;
        let request = RevisionRequest {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            request_id: request_id.clone(),
            proposal_version,
            requirement_hash: requirement_hash.clone(),
            deadline,
            requester: requester.clone(),
            responder,
            status: RevisionStatus::Open,
            response_hash: None,
            resubmission_version: 0,
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &request);
        env.storage().persistent().set(&count_key, &next_count);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &count_key);
        env.events().publish(
            (symbol_short!("CRR_REQ"),),
            (tenant_id, project_id, request_id, proposal_version, deadline),
        );
        Ok(request)
    }

    pub fn respond_revision(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        response_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<RevisionRequest, ContractError> {
        responder.require_auth();
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let mut request = Self::load_request(&env, &key)?;
        if request.responder != responder {
            return Err(ContractError::Unauthorized);
        }
        if env.ledger().timestamp() > request.deadline {
            return Err(ContractError::DeadlineExpired);
        }
        if request.status == RevisionStatus::Responded {
            if request.response_hash == Some(response_hash.clone()) {
                Self::extend_persistent(&env, &key);
                return Ok(request);
            }
            return Err(ContractError::AlreadyResponded);
        }
        if request.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if request.status != RevisionStatus::Open {
            return Err(ContractError::InvalidState);
        }
        request.status = RevisionStatus::Responded;
        request.response_hash = Some(response_hash.clone());
        request.expected_version = request
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        request.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &request);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CRR_RESP"),),
            (tenant_id, project_id, request_id, response_hash),
        );
        Ok(request)
    }

    pub fn resubmit_proposal(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        resubmission_version: u32,
        expected_version: u32,
    ) -> Result<RevisionRequest, ContractError> {
        responder.require_auth();
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let mut request = Self::load_request(&env, &key)?;
        if request.responder != responder {
            return Err(ContractError::Unauthorized);
        }
        if request.status == RevisionStatus::Resubmitted {
            if request.resubmission_version == resubmission_version {
                Self::extend_persistent(&env, &key);
                return Ok(request);
            }
            return Err(ContractError::ResubmissionNotAllowed);
        }
        if request.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if request.status != RevisionStatus::Responded {
            return Err(ContractError::InvalidState);
        }
        if resubmission_version <= request.proposal_version {
            return Err(ContractError::ResubmissionNotAllowed);
        }
        request.status = RevisionStatus::Resubmitted;
        request.resubmission_version = resubmission_version;
        request.expected_version = request
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        request.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &request);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CRR_RESUB"),),
            (tenant_id, project_id, request_id, resubmission_version),
        );
        Ok(request)
    }

    pub fn close_request(
        env: Env,
        requester: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<RevisionRequest, ContractError> {
        requester.require_auth();
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let mut request = Self::load_request(&env, &key)?;
        if request.requester != requester {
            return Err(ContractError::Unauthorized);
        }
        if request.status == RevisionStatus::Closed {
            Self::extend_persistent(&env, &key);
            return Ok(request);
        }
        if request.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if request.status != RevisionStatus::Resubmitted && request.status != RevisionStatus::Responded
        {
            return Err(ContractError::InvalidState);
        }
        request.status = RevisionStatus::Closed;
        request.expected_version = request
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        request.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &request);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CRR_CLOSE"),),
            (tenant_id, project_id, request_id),
        );
        Ok(request)
    }

    pub fn get_request(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
    ) -> Option<RevisionRequest> {
        env.storage().persistent().get(&DataKey::Request(
            tenant_id,
            project_id,
            request_id,
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
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn load_request(env: &Env, key: &DataKey) -> Result<RevisionRequest, ContractError> {
        env.storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::RequestNotFound)
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
