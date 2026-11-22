#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_OBJECTIVES: u32 = 64;
const MAX_CRITERIA: u32 = 16;
const MAX_REVISIONS: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    ObjectiveNotFound = 5,
    ObjectiveAlreadyExists = 6,
    ZeroHash = 7,
    NoCriteria = 8,
    InvalidPriority = 9,
    CriteriaLimitReached = 10,
    VersionMismatch = 11,
    InvalidVersion = 12,
    InvalidState = 13,
    ObjectiveLimitReached = 14,
    RevisionLimitReached = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Participant = 1,
    Coordinator = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Public = 1,
    Restricted = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectiveState {
    Draft = 1,
    Active = 2,
    Closed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectiveRevision {
    pub version: u32,
    pub owner: Address,
    pub priority: u32,
    pub visibility: Visibility,
    pub baseline_hash: BytesN<32>,
    pub target_hash: BytesN<32>,
    pub target_date: u64,
    pub charter_hash: BytesN<32>,
    pub criteria_hashes: Vec<BytesN<32>>,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Objective {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub objective_id: BytesN<32>,
    pub state: ObjectiveState,
    pub expected_version: u32,
    pub current: ObjectiveRevision,
    pub revisions: Vec<ObjectiveRevision>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Objective(BytesN<32>, BytesN<32>, BytesN<32>),
    ObjectiveIds(BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectiveSpec {
    pub owner: Address,
    pub priority: u32,
    pub visibility: Visibility,
    pub baseline_hash: BytesN<32>,
    pub target_hash: BytesN<32>,
    pub target_date: u64,
    pub charter_hash: BytesN<32>,
    pub criteria_hashes: Vec<BytesN<32>>,
}

#[contract]
pub struct CapstoneObjectivesContract;

#[contractimpl]
impl CapstoneObjectivesContract {
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
            (symbol_short!("OBJROLE"),),
            (tenant, project, account, role, active),
        );
        Ok(())
    }

    pub fn create_objective(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        objective_id: BytesN<32>,
        spec: ObjectiveSpec,
        expected_version: u32,
    ) -> Result<Objective, ContractError> {
        caller.require_auth();
        Self::require_any_role(&env, &caller, &tenant, &project)?;
        Self::require_any_role(&env, &spec.owner, &tenant, &project)?;
        Self::require_owner_or_coordinator(&env, &caller, &spec.owner, &tenant, &project)?;
        Self::validate_revision(&env, &objective_id, &spec)?;
        let key = DataKey::Objective(
            tenant.clone(),
            project.clone(),
            objective_id.clone(),
        );
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, Objective>(&key)
        {
            let current = &existing.current;
            if current.owner == spec.owner
                && current.priority == spec.priority
                && current.visibility == spec.visibility
                && current.baseline_hash == spec.baseline_hash
                && current.target_hash == spec.target_hash
                && current.target_date == spec.target_date
                && current.charter_hash == spec.charter_hash
                && current.criteria_hashes == spec.criteria_hashes
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            return Err(ContractError::ObjectiveAlreadyExists);
        }
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let ids_key = DataKey::ObjectiveIds(tenant.clone(), project.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));
        if ids.len() >= MAX_OBJECTIVES {
            return Err(ContractError::ObjectiveLimitReached);
        }
        let now = env.ledger().timestamp();
        let revision = ObjectiveRevision {
            version: 1,
            owner: spec.owner.clone(),
            priority: spec.priority,
            visibility: spec.visibility.clone(),
            baseline_hash: spec.baseline_hash.clone(),
            target_hash: spec.target_hash.clone(),
            target_date: spec.target_date,
            charter_hash: spec.charter_hash.clone(),
            criteria_hashes: spec.criteria_hashes.clone(),
            created_at: now,
        };
        let mut revisions = Vec::new(&env);
        revisions.push_back(revision.clone());
        let objective = Objective {
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            objective_id: objective_id.clone(),
            state: ObjectiveState::Draft,
            expected_version: 1,
            current: revision,
            revisions,
            created_at: now,
            updated_at: now,
        };
        env.storage().persistent().set(&key, &objective);
        ids.push_back(objective_id.clone());
        env.storage().persistent().set(&ids_key, &ids);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &ids_key);
        env.events().publish(
            (symbol_short!("OBJCREATE"),),
            (tenant, project, objective_id, spec.owner, spec.priority),
        );
        Ok(objective)
    }

    pub fn amend_objective(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        objective_id: BytesN<32>,
        new_version: u32,
        spec: ObjectiveSpec,
        expected_version: u32,
    ) -> Result<Objective, ContractError> {
        caller.require_auth();
        Self::require_any_role(&env, &caller, &tenant, &project)?;
        Self::require_any_role(&env, &spec.owner, &tenant, &project)?;
        Self::require_owner_or_coordinator(&env, &caller, &spec.owner, &tenant, &project)?;
        Self::validate_revision(&env, &objective_id, &spec)?;
        let key = DataKey::Objective(
            tenant.clone(),
            project.clone(),
            objective_id.clone(),
        );
        let mut objective = Self::load_objective(&env, &key)?;
        if objective.state == ObjectiveState::Closed {
            return Err(ContractError::InvalidState);
        }
        let now = env.ledger().timestamp();
        let candidate = ObjectiveRevision {
            version: new_version,
            owner: spec.owner.clone(),
            priority: spec.priority,
            visibility: spec.visibility.clone(),
            baseline_hash: spec.baseline_hash.clone(),
            target_hash: spec.target_hash.clone(),
            target_date: spec.target_date,
            charter_hash: spec.charter_hash.clone(),
            criteria_hashes: spec.criteria_hashes.clone(),
            created_at: now,
        };
        if Self::same_revision_content(&objective.current, &candidate) {
            Self::extend_persistent(&env, &key);
            return Ok(objective);
        }
        if objective.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        if let Some(latest) = objective.revisions.last() {
            if Self::same_revision_content(&latest, &candidate) {
                Self::extend_persistent(&env, &key);
                return Ok(objective);
            }
        }
        if new_version <= objective.current.version {
            return Err(ContractError::InvalidVersion);
        }
        if objective.revisions.len() >= MAX_REVISIONS {
            return Err(ContractError::RevisionLimitReached);
        }
        objective.current = candidate.clone();
        objective.revisions.push_back(candidate);
        objective.expected_version = objective
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        objective.updated_at = now;
        env.storage().persistent().set(&key, &objective);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("OBJAMEND"),),
            (tenant, project, objective_id, new_version, spec.owner),
        );
        Ok(objective)
    }

    pub fn set_state(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        objective_id: BytesN<32>,
        state: ObjectiveState,
        expected_version: u32,
    ) -> Result<Objective, ContractError> {
        caller.require_auth();
        Self::require_any_role(&env, &caller, &tenant, &project)?;
        let key = DataKey::Objective(
            tenant.clone(),
            project.clone(),
            objective_id.clone(),
        );
        let mut objective = Self::load_objective(&env, &key)?;
        Self::require_owner_or_coordinator(
            &env,
            &caller,
            &objective.current.owner,
            &tenant,
            &project,
        )?;
        if objective.state == ObjectiveState::Closed {
            return Err(ContractError::InvalidState);
        }
        if objective.state == state {
            Self::extend_persistent(&env, &key);
            return Ok(objective);
        }
        let valid = (objective.state == ObjectiveState::Draft
            && (state == ObjectiveState::Active || state == ObjectiveState::Closed))
            || (objective.state == ObjectiveState::Active && state == ObjectiveState::Closed);
        if !valid {
            return Err(ContractError::InvalidState);
        }
        if objective.expected_version != expected_version {
            return Err(ContractError::VersionMismatch);
        }
        objective.state = state;
        objective.expected_version = objective
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        objective.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &objective);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("OBJSTATE"),),
            (tenant, project, objective_id, state),
        );
        Ok(objective)
    }

    pub fn get_objective(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        objective_id: BytesN<32>,
    ) -> Result<Objective, ContractError> {
        caller.require_auth();
        let key = DataKey::Objective(tenant.clone(), project.clone(), objective_id);
        let objective = Self::load_objective(&env, &key)?;
        Self::require_readable(&env, &caller, &objective, &tenant, &project)?;
        Ok(objective)
    }

    pub fn list_objective_ids(
        env: Env,
        caller: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        caller.require_auth();
        let ids_key = DataKey::ObjectiveIds(tenant.clone(), project.clone());
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));
        if env.storage().persistent().has(&ids_key) {
            Self::extend_persistent(&env, &ids_key);
        }
        let mut readable = Vec::new(&env);
        for i in 0..ids.len() {
            if let Some(id) = ids.get(i) {
                let key = DataKey::Objective(tenant.clone(), project.clone(), id.clone());
                if let Some(objective) = env.storage().persistent().get::<_, Objective>(&key) {
                    Self::extend_persistent(&env, &key);
                    if Self::is_readable(&env, &caller, &objective, &tenant, &project) {
                        readable.push_back(id);
                    }
                }
            }
        }
        let mut ordered: Vec<BytesN<32>> = Vec::new(&env);
        for i in 0..readable.len() {
            let id = readable.get(i).ok_or(ContractError::ObjectiveNotFound)?;
            let key = DataKey::Objective(tenant.clone(), project.clone(), id.clone());
            let objective = Self::load_objective(&env, &key)?;
            let mut position = ordered.len();
            while position > 0 {
                let previous_id = ordered
                    .get(position - 1)
                    .ok_or(ContractError::ObjectiveNotFound)?;
                let previous_key = DataKey::Objective(
                    tenant.clone(),
                    project.clone(),
                    previous_id.clone(),
                );
                let previous = Self::load_objective(&env, &previous_key)?;
                if Self::precedes(
                    objective.current.priority,
                    objective.current.target_date,
                    &id,
                    previous.current.priority,
                    previous.current.target_date,
                    &previous_id,
                ) {
                    position -= 1;
                } else {
                    break;
                }
            }
            ordered.insert(position, id);
        }
        Ok(ordered)
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

    fn has_role(
        env: &Env,
        account: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
        role: Role,
    ) -> bool {
        let key = DataKey::Role(account.clone(), tenant.clone(), project.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend_persistent(env, &key);
        }
        active
    }

    fn require_any_role(
        env: &Env,
        account: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let participant = Self::has_role(env, account, tenant, project, Role::Participant);
        let coordinator = Self::has_role(env, account, tenant, project, Role::Coordinator);
        if !participant && !coordinator {
            return Err(ContractError::InvalidRole);
        }
        Ok(())
    }

    fn require_owner_or_coordinator(
        env: &Env,
        caller: &Address,
        owner: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        if caller == owner {
            return Ok(());
        }
        if Self::has_role(env, caller, tenant, project, Role::Coordinator) {
            return Ok(());
        }
        Err(ContractError::Unauthorized)
    }

    fn is_readable(
        env: &Env,
        caller: &Address,
        objective: &Objective,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> bool {
        if objective.current.visibility == Visibility::Public {
            return true;
        }
        caller == &objective.current.owner
            || Self::has_role(env, caller, tenant, project, Role::Coordinator)
    }

    fn require_readable(
        env: &Env,
        caller: &Address,
        objective: &Objective,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
    ) -> Result<(), ContractError> {
        if Self::is_readable(env, caller, objective, tenant, project) {
            return Ok(());
        }
        Err(ContractError::Unauthorized)
    }

    fn precedes(
        a_priority: u32,
        a_date: u64,
        a_id: &BytesN<32>,
        b_priority: u32,
        b_date: u64,
        b_id: &BytesN<32>,
    ) -> bool {
        if a_priority != b_priority {
            return a_priority < b_priority;
        }
        if a_date != b_date {
            return a_date < b_date;
        }
        a_id.to_array() < b_id.to_array()
    }

    fn validate_revision(
        env: &Env,
        objective_id: &BytesN<32>,
        spec: &ObjectiveSpec,
    ) -> Result<(), ContractError> {
        let zero = BytesN::from_array(env, &[0u8; 32]);
        if objective_id == &zero
            || spec.baseline_hash == zero
            || spec.target_hash == zero
            || spec.charter_hash == zero
        {
            return Err(ContractError::ZeroHash);
        }
        if spec.priority == 0 {
            return Err(ContractError::InvalidPriority);
        }
        if spec.criteria_hashes.is_empty() {
            return Err(ContractError::NoCriteria);
        }
        if spec.criteria_hashes.len() > MAX_CRITERIA {
            return Err(ContractError::CriteriaLimitReached);
        }
        for i in 0..spec.criteria_hashes.len() {
            if let Some(criterion) = spec.criteria_hashes.get(i) {
                if criterion == zero {
                    return Err(ContractError::ZeroHash);
                }
            }
        }
        Ok(())
    }

    fn same_revision_content(a: &ObjectiveRevision, b: &ObjectiveRevision) -> bool {
        a.version == b.version
            && a.owner == b.owner
            && a.priority == b.priority
            && a.visibility == b.visibility
            && a.baseline_hash == b.baseline_hash
            && a.target_hash == b.target_hash
            && a.target_date == b.target_date
            && a.charter_hash == b.charter_hash
            && a.criteria_hashes == b.criteria_hashes
    }

    fn load_objective(env: &Env, key: &DataKey) -> Result<Objective, ContractError> {
        let objective: Objective = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::ObjectiveNotFound)?;
        Self::extend_persistent(env, key);
        Ok(objective)
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
