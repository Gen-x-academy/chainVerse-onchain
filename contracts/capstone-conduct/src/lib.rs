#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_POLICIES: u32 = 32;
const MAX_BREACHES_PER_SUBJECT: u32 = 16;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    PolicyNotFound = 5,
    PolicyRetired = 6,
    NotApplicable = 7,
    ZeroHash = 8,
    AcceptanceNotFound = 9,
    BreachExists = 10,
    BreachNotFound = 11,
    BreachLimitReached = 12,
    PolicyLimitReached = 13,
    EffectiveAtNotReached = 14,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Participant = 1,
    Coordinator = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    Participant = 1,
    Coordinator = 2,
    Both = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Low = 1,
    Medium = 2,
    High = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyState {
    Active = 1,
    Retired = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreachState {
    Open = 1,
    Resolved = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    pub version: u32,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub policy_hash: BytesN<32>,
    pub effective_at: u64,
    pub mandatory: bool,
    pub audience: Audience,
    pub state: PolicyState,
    pub published_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acceptance {
    pub actor: Address,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub version: u32,
    pub role: Role,
    pub policy_hash: BytesN<32>,
    pub accepted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Breach {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub case_ref: BytesN<32>,
    pub subject: Address,
    pub policy_version: u32,
    pub severity: Severity,
    pub evidence_hash: BytesN<32>,
    pub state: BreachState,
    pub recorded_at: u64,
    pub resolved_at: Option<u64>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    PolicyCount(BytesN<32>, BytesN<32>),
    Policy(BytesN<32>, BytesN<32>, u32),
    Acceptance(Address, BytesN<32>, BytesN<32>, u32),
    Breach(BytesN<32>, BytesN<32>, BytesN<32>),
    BreachIndex(BytesN<32>, BytesN<32>, Address),
}

#[contract]
pub struct CapstoneConductContract;

#[contractimpl]
impl CapstoneConductContract {
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
            (symbol_short!("CONROLE"),),
            (tenant, project, account, role, active),
        );
        Ok(())
    }

    pub fn publish_policy(
        env: Env,
        admin: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        policy_hash: BytesN<32>,
        effective_at: u64,
        mandatory: bool,
        audience: Audience,
    ) -> Result<Policy, ContractError> {
        Self::require_admin(&env, &admin)?;
        if policy_hash == Self::zero_hash(&env) {
            return Err(ContractError::ZeroHash);
        }
        let count_key = DataKey::PolicyCount(tenant.clone(), project.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_POLICIES {
            return Err(ContractError::PolicyLimitReached);
        }
        let version = count
            .checked_add(1)
            .ok_or(ContractError::PolicyLimitReached)?;
        let now = env.ledger().timestamp();
        let policy = Policy {
            version,
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            policy_hash: policy_hash.clone(),
            effective_at,
            mandatory,
            audience: audience.clone(),
            state: PolicyState::Active,
            published_at: now,
        };
        let key = DataKey::Policy(tenant.clone(), project.clone(), version);
        env.storage().persistent().set(&key, &policy);
        env.storage().persistent().set(&count_key, &version);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &count_key);
        env.events().publish(
            (symbol_short!("POLPUB"),),
            (tenant, project, version, policy_hash, mandatory, audience),
        );
        Ok(policy)
    }

    pub fn retire_policy(
        env: Env,
        admin: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        version: u32,
    ) -> Result<Policy, ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Policy(tenant.clone(), project.clone(), version);
        let mut policy = Self::load_policy(&env, &key)?;
        if policy.state == PolicyState::Retired {
            Self::extend_persistent(&env, &key);
            return Ok(policy);
        }
        policy.state = PolicyState::Retired;
        env.storage().persistent().set(&key, &policy);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("POLRET"),),
            (tenant, project, version),
        );
        Ok(policy)
    }

    pub fn accept_policy(
        env: Env,
        actor: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        version: u32,
        role: Role,
    ) -> Result<Acceptance, ContractError> {
        actor.require_auth();
        Self::require_role(&env, &actor, &tenant, &project, role.clone())?;
        let policy_key = DataKey::Policy(tenant.clone(), project.clone(), version);
        let policy = Self::load_policy(&env, &policy_key)?;
        if policy.state == PolicyState::Retired {
            return Err(ContractError::PolicyRetired);
        }
        if !Self::applies_to(&policy.audience, &role) {
            return Err(ContractError::NotApplicable);
        }
        let now = env.ledger().timestamp();
        if now < policy.effective_at {
            return Err(ContractError::EffectiveAtNotReached);
        }
        let acceptance = Acceptance {
            actor: actor.clone(),
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            version,
            role: role.clone(),
            policy_hash: policy.policy_hash.clone(),
            accepted_at: now,
        };
        let key = DataKey::Acceptance(actor.clone(), tenant.clone(), project.clone(), version);
        env.storage().persistent().set(&key, &acceptance);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("POLACC"),),
            (tenant, project, version, actor, role),
        );
        Ok(acceptance)
    }

    pub fn requires_acknowledgement(
        env: Env,
        actor: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        role: Role,
    ) -> Result<Vec<u32>, ContractError> {
        actor.require_auth();
        Self::pending_acknowledgements(&env, &actor, &tenant, &project, &role)
    }

    fn pending_acknowledgements(
        env: &Env,
        actor: &Address,
        tenant: &BytesN<32>,
        project: &BytesN<32>,
        role: &Role,
    ) -> Result<Vec<u32>, ContractError> {
        Self::require_role(env, actor, tenant, project, role.clone())?;
        let now = env.ledger().timestamp();
        let count_key = DataKey::PolicyCount(tenant.clone(), project.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        Self::extend_persistent(env, &count_key);
        let mut pending = Vec::new(env);
        for version in 1..=count {
            let policy_key = DataKey::Policy(tenant.clone(), project.clone(), version);
            let policy: Policy = match env.storage().persistent().get(&policy_key) {
                Some(policy) => policy,
                None => continue,
            };
            Self::extend_persistent(env, &policy_key);
            if !policy.mandatory
                || policy.state != PolicyState::Active
                || now < policy.effective_at
                || !Self::applies_to(&policy.audience, role)
            {
                continue;
            }
            let acceptance_key =
                DataKey::Acceptance(actor.clone(), tenant.clone(), project.clone(), version);
            let fresh = match env
                .storage()
                .persistent()
                .get::<_, Acceptance>(&acceptance_key)
            {
                Some(acceptance) => {
                    Self::extend_persistent(env, &acceptance_key);
                    acceptance.policy_hash == policy.policy_hash && acceptance.role == *role
                }
                None => false,
            };
            if !fresh {
                pending.push_back(version);
            }
        }
        Ok(pending)
    }

    pub fn is_compliant(
        env: Env,
        actor: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        role: Role,
    ) -> Result<bool, ContractError> {
        let pending =
            Self::pending_acknowledgements(&env, &actor, &tenant, &project, &role)?;
        if !pending.is_empty() {
            return Ok(false);
        }
        let index_key = DataKey::BreachIndex(tenant.clone(), project.clone(), actor.clone());
        let cases: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if env.storage().persistent().has(&index_key) {
            Self::extend_persistent(&env, &index_key);
        }
        for i in 0..cases.len() {
            if let Some(case_ref) = cases.get(i) {
                let breach_key =
                    DataKey::Breach(tenant.clone(), project.clone(), case_ref.clone());
                let breach: Breach = match env.storage().persistent().get(&breach_key) {
                    Some(breach) => breach,
                    None => continue,
                };
                Self::extend_persistent(&env, &breach_key);
                if breach.subject == actor && breach.state == BreachState::Open {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    pub fn record_breach(
        env: Env,
        admin: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        case_ref: BytesN<32>,
        subject: Address,
        policy_version: u32,
        severity: Severity,
        evidence_hash: BytesN<32>,
    ) -> Result<Breach, ContractError> {
        Self::require_admin(&env, &admin)?;
        let zero = Self::zero_hash(&env);
        if case_ref == zero || evidence_hash == zero {
            return Err(ContractError::ZeroHash);
        }
        Self::require_any_role(&env, &subject, &tenant, &project)?;
        let policy_key = DataKey::Policy(tenant.clone(), project.clone(), policy_version);
        Self::load_policy(&env, &policy_key)?;
        let breach_key = DataKey::Breach(
            tenant.clone(),
            project.clone(),
            case_ref.clone(),
        );
        if env.storage().persistent().has(&breach_key) {
            return Err(ContractError::BreachExists);
        }
        let index_key = DataKey::BreachIndex(tenant.clone(), project.clone(), subject.clone());
        let mut cases: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if cases.len() >= MAX_BREACHES_PER_SUBJECT {
            return Err(ContractError::BreachLimitReached);
        }
        let now = env.ledger().timestamp();
        let breach = Breach {
            tenant_id: tenant.clone(),
            project_id: project.clone(),
            case_ref: case_ref.clone(),
            subject: subject.clone(),
            policy_version,
            severity: severity.clone(),
            evidence_hash: evidence_hash.clone(),
            state: BreachState::Open,
            recorded_at: now,
            resolved_at: None,
        };
        env.storage().persistent().set(&breach_key, &breach);
        cases.push_back(case_ref.clone());
        env.storage().persistent().set(&index_key, &cases);
        Self::extend_persistent(&env, &breach_key);
        Self::extend_persistent(&env, &index_key);
        env.events().publish(
            (symbol_short!("BRCHREC"),),
            (tenant, project, case_ref, subject, severity, policy_version),
        );
        Ok(breach)
    }

    pub fn resolve_breach(
        env: Env,
        admin: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        case_ref: BytesN<32>,
    ) -> Result<Breach, ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Breach(tenant.clone(), project.clone(), case_ref.clone());
        let mut breach = Self::load_breach(&env, &key)?;
        if breach.state == BreachState::Resolved {
            Self::extend_persistent(&env, &key);
            return Ok(breach);
        }
        breach.state = BreachState::Resolved;
        breach.resolved_at = Some(env.ledger().timestamp());
        env.storage().persistent().set(&key, &breach);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("BRCHRES"),),
            (tenant, project, case_ref),
        );
        Ok(breach)
    }

    pub fn get_policy(
        env: Env,
        tenant: BytesN<32>,
        project: BytesN<32>,
        version: u32,
    ) -> Result<Policy, ContractError> {
        let key = DataKey::Policy(tenant, project, version);
        Self::load_policy(&env, &key)
    }

    pub fn get_acceptance(
        env: Env,
        actor: Address,
        tenant: BytesN<32>,
        project: BytesN<32>,
        version: u32,
    ) -> Result<Acceptance, ContractError> {
        let key = DataKey::Acceptance(actor, tenant, project, version);
        let acceptance: Acceptance = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::AcceptanceNotFound)?;
        Self::extend_persistent(&env, &key);
        Ok(acceptance)
    }

    pub fn get_breach(
        env: Env,
        tenant: BytesN<32>,
        project: BytesN<32>,
        case_ref: BytesN<32>,
    ) -> Result<Breach, ContractError> {
        let key = DataKey::Breach(tenant, project, case_ref);
        Self::load_breach(&env, &key)
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
        let participant = DataKey::Role(
            account.clone(),
            tenant.clone(),
            project.clone(),
            Role::Participant,
        );
        let coordinator = DataKey::Role(
            account.clone(),
            tenant.clone(),
            project.clone(),
            Role::Coordinator,
        );
        let is_participant: bool = env
            .storage()
            .persistent()
            .get(&participant)
            .unwrap_or(false);
        let is_coordinator: bool = env
            .storage()
            .persistent()
            .get(&coordinator)
            .unwrap_or(false);
        if !is_participant && !is_coordinator {
            return Err(ContractError::InvalidRole);
        }
        if env.storage().persistent().has(&participant) {
            Self::extend_persistent(env, &participant);
        }
        if env.storage().persistent().has(&coordinator) {
            Self::extend_persistent(env, &coordinator);
        }
        Ok(())
    }

    fn applies_to(audience: &Audience, role: &Role) -> bool {
        match audience {
            Audience::Both => true,
            Audience::Participant => *role == Role::Participant,
            Audience::Coordinator => *role == Role::Coordinator,
        }
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn load_policy(env: &Env, key: &DataKey) -> Result<Policy, ContractError> {
        let policy: Policy = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::PolicyNotFound)?;
        Self::extend_persistent(env, key);
        Ok(policy)
    }

    fn load_breach(env: &Env, key: &DataKey) -> Result<Breach, ContractError> {
        let breach: Breach = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(ContractError::BreachNotFound)?;
        Self::extend_persistent(env, key);
        Ok(breach)
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
