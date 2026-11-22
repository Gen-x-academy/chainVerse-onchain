#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_APPROVERS: u32 = 8;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    ClosureNotFound = 5,
    ClosureAlreadyExists = 6,
    OpenClosureExists = 7,
    InvalidState = 8,
    VersionMismatch = 9,
    ZeroHash = 10,
    InvalidEffectiveAt = 11,
    DuplicateApprover = 12,
    ApprovalLimitReached = 13,
    SelfApproval = 14,
    EffectiveAtNotReached = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Participant = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosureOutcome {
    Completed = 1,
    MutualTermination = 2,
    Inactivity = 3,
    PolicyAction = 4,
    SponsorWithdrawal = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClosureState {
    Requested = 1,
    Approved = 2,
    Rejected = 3,
    Cancelled = 4,
    Closed = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Closure {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub closure_id: BytesN<32>,
    pub requester: Address,
    pub outcome: ClosureOutcome,
    pub reason_hash: BytesN<32>,
    pub effective_at: u64,
    pub state: ClosureState,
    pub approvers: Vec<Address>,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Closure(BytesN<32>, BytesN<32>, BytesN<32>),
    Open(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneProjectClosuresContract;

#[contractimpl]
impl CapstoneProjectClosuresContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn admin(env: Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    pub fn set_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        role: Role,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(account.clone(), tenant.clone(), project.clone(), role.clone());
        env.storage().persistent().set(&key, &active);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CLROLE"),),
            (tenant, project, account, role, active),
        );
        Ok(())
    }

    pub fn request_closure(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
        outcome: ClosureOutcome,
        reason_hash: BytesN<32>,
        effective_at: u64,
        expected_version: u32,
    ) -> Result<Closure, ContractError> {
        caller.require_auth();
        Self::require_any_role(&env, &caller, &tenant, &project)?;
        let zero = Self::zero_hash(&env);
        if closure_id == zero || reason_hash == zero {
            return Err(ContractError::ZeroHash);
        }
        let now = env.ledger().timestamp();
        if effective_at <= now {
            return Err(ContractError::InvalidEffectiveAt);
        }
        let key = DataKey::Closure(
            tenant.clone(),
            project.clone(),
            closure_id.clone(),
        );
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, Closure>(&key)
        {
            if existing.requester == caller
                && existing.outcome == outcome
                && existing.reason_hash == reason_hash
                && existing.effective_at == effective_at
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            return Err(ContractError::ClosureAlreadyExists);
        }
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let open_key = DataKey::Open(tenant.clone(), project.clone());
        if env.storage().persistent().has(&open_key) {
            return Err(ContractError::OpenClosureExists);
        }
        let closure = Closure {
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            closure_id: closure_id.clone(),
            requester: caller.clone(),
            outcome,
            reason_hash: reason_hash.clone(),
            effective_at,
            state: ClosureState::Requested,
            approvers: Vec::new(&env),
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &closure);
        env.storage().persistent().set(&open_key, &closure_id);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &open_key);
        env.events().publish(
            (symbol_short!("CLREQ"),),
            (tenant, project, closure_id, outcome, effective_at),
        );
        Ok(closure)
    }

    pub fn approve_closure(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
        approver: Address,
        expected_version: u32,
    ) -> Result<Closure, ContractError> {
        caller.require_auth();
        if caller != approver {
            return Err(ContractError::Unauthorized);
        }
        Self::require_role(
            &env,
            &approver,
            &tenant,
            &project,
            Role::Coordinator,
        )?;
        let key = DataKey::Closure(
            tenant.clone(),
            project.clone(),
            closure_id.clone(),
        );
        let mut closure = Self::load_closure(&env, &key)?;
        if closure.requester == approver {
            return Err(ContractError::SelfApproval);
        }
        if closure.state == ClosureState::Approved
            && Self::contains_approver(&closure.approvers, &approver)
        {
            Self::extend_persistent(&env, &key);
            return Ok(closure);
        }
        if closure.state != ClosureState::Requested && closure.state != ClosureState::Approved {
            return Err(ContractError::InvalidState);
        }
        if closure.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if Self::contains_approver(&closure.approvers, &approver) {
            return Err(ContractError::DuplicateApprover);
        }
        if closure.approvers.len() >= MAX_APPROVERS {
            return Err(ContractError::ApprovalLimitReached);
        }
        closure.approvers.push_back(approver.clone());
        closure.state = ClosureState::Approved;
        closure.expected_version = closure
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        closure.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &closure);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CLAPP"),),
            (tenant, project, closure_id, approver),
        );
        Ok(closure)
    }

    pub fn reject_closure(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<Closure, ContractError> {
        caller.require_auth();
        let key = DataKey::Closure(
            tenant.clone(),
            project.clone(),
            closure_id.clone(),
        );
        let mut closure = Self::load_closure(&env, &key)?;
        if closure.state == ClosureState::Rejected {
            Self::extend_persistent(&env, &key);
            return Ok(closure);
        }
        if caller != closure.requester {
            Self::require_role(
                &env,
                &caller,
                &tenant,
                &project,
                Role::Coordinator,
            )?;
        }
        Self::require_transitionable(&closure.state)?;
        if closure.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        closure.state = ClosureState::Rejected;
        closure.expected_version = closure
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        closure.updated_at = env.ledger().timestamp();
        env.storage().persistent().remove(&DataKey::Open(
            tenant.clone(),
            project.clone(),
        ));
        env.storage().persistent().set(&key, &closure);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CLREJ"),),
            (tenant, project, closure_id),
        );
        Ok(closure)
    }

    pub fn cancel_closure(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<Closure, ContractError> {
        caller.require_auth();
        let key = DataKey::Closure(
            tenant.clone(),
            project.clone(),
            closure_id.clone(),
        );
        let mut closure = Self::load_closure(&env, &key)?;
        if caller != closure.requester {
            return Err(ContractError::Unauthorized);
        }
        if closure.state == ClosureState::Cancelled {
            Self::extend_persistent(&env, &key);
            return Ok(closure);
        }
        Self::require_transitionable(&closure.state)?;
        if closure.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        closure.state = ClosureState::Cancelled;
        closure.expected_version = closure
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        closure.updated_at = env.ledger().timestamp();
        env.storage().persistent().remove(&DataKey::Open(
            tenant.clone(),
            project.clone(),
        ));
        env.storage().persistent().set(&key, &closure);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CLCAN"),),
            (tenant, project, closure_id),
        );
        Ok(closure)
    }

    pub fn close_closure(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<Closure, ContractError> {
        caller.require_auth();
        let key = DataKey::Closure(
            tenant.clone(),
            project.clone(),
            closure_id.clone(),
        );
        let mut closure = Self::load_closure(&env, &key)?;
        if closure.state == ClosureState::Closed {
            Self::extend_persistent(&env, &key);
            return Ok(closure);
        }
        if caller != closure.requester {
            Self::require_role(
                &env,
                &caller,
                &tenant,
                &project,
                Role::Coordinator,
            )?;
        }
        if closure.state != ClosureState::Approved {
            return Err(ContractError::InvalidState);
        }
        if closure.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        let now = env.ledger().timestamp();
        if now < closure.effective_at {
            return Err(ContractError::EffectiveAtNotReached);
        }
        closure.state = ClosureState::Closed;
        closure.expected_version = closure
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        closure.updated_at = now;
        env.storage().persistent().remove(&DataKey::Open(
            tenant.clone(),
            project.clone(),
        ));
        env.storage().persistent().set(&key, &closure);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("CLCLS"),),
            (tenant, project, closure_id, closure.outcome),
        );
        Ok(closure)
    }

    pub fn get_closure(
        env: Env,
        tenant: BytesN<32>,
        project: BytesN<32>,
        closure_id: BytesN<32>,
    ) -> Result<Closure, ContractError> {
        let key = DataKey::Closure(tenant, project, closure_id);
        let closure = Self::load_closure(&env, &key)?;
        Ok(closure)
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
        if *caller != admin {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_role(
        env: &Env,
        account: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        let key = DataKey::Role(account.clone(), tenant.clone(), project.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !active {
            return Err(ContractError::InvalidRole);
        }
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn require_any_role(
        env: &Env,
        account: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let coordinator = DataKey::Role(
            account.clone(),
            tenant.clone(),
            project.clone(),
            Role::Coordinator,
        );
        let participant = DataKey::Role(
            account.clone(),
            tenant.clone(),
            project.clone(),
            Role::Participant,
        );
        let is_coordinator: bool = env
            .storage()
            .persistent()
            .get(&coordinator)
            .unwrap_or(false);
        let is_participant: bool = env
            .storage()
            .persistent()
            .get(&participant)
            .unwrap_or(false);
        if !is_coordinator && !is_participant {
            return Err(ContractError::Unauthorized);
        }
        if env.storage().persistent().has(&coordinator) {
            Self::extend_persistent(env, &coordinator);
        }
        if env.storage().persistent().has(&participant) {
            Self::extend_persistent(env, &participant);
        }
        Ok(())
    }

    fn require_transitionable(state: &ClosureState) -> Result<(), ContractError> {
        if *state != ClosureState::Requested && *state != ClosureState::Approved {
            return Err(ContractError::InvalidState);
        }
        Ok(())
    }

    fn contains_approver(approvers: &Vec<Address>, approver: &Address) -> bool {
        for i in 0..approvers.len() {
            if let Some(existing) = approvers.get(i) {
                if existing == *approver {
                    return true;
                }
            }
        }
        false
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn load_closure(env: &Env, key: &DataKey) -> Result<Closure, ContractError> {
        let closure: Closure = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::ClosureNotFound)?;
        Self::extend_persistent(env, key);
        Ok(closure)
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
