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
const MAX_CHECKINS: u32 = 64;
const MAX_CADENCE_SECONDS: u32 = 31_536_000;
const MAX_CONFIDENCE_BPS: u32 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    ObjectiveExists = 5,
    ObjectiveNotFound = 6,
    CheckinExists = 7,
    CheckinNotFound = 8,
    WrongOwner = 9,
    ObjectiveClosed = 10,
    CadenceViolation = 11,
    InvalidConfidence = 12,
    InvalidCadence = 13,
    InvalidSupersedes = 14,
    CheckinLimitReached = 15,
    ObjectiveLimitReached = 16,
    VersionMismatch = 17,
    LimitReached = 18,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Planner = 1,
    Participant = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckinStatus {
    OnTrack = 1,
    AtRisk = 2,
    Blocked = 3,
    Closed = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectiveState {
    Open = 1,
    Closed = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Objective {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub objective_id: BytesN<32>,
    pub owner: Address,
    pub delegate: Option<Address>,
    pub cadence_seconds: u32,
    pub spec_hash: BytesN<32>,
    pub state: ObjectiveState,
    pub current_status: CheckinStatus,
    pub latest_checkin_id: Option<BytesN<32>>,
    pub last_checkin_at: Option<u64>,
    pub checkin_count: u32,
    pub expected_version: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkin {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub objective_id: BytesN<32>,
    pub checkin_id: BytesN<32>,
    pub filer: Address,
    pub status: CheckinStatus,
    pub confidence_bps: u32,
    pub evidence_hash: BytesN<32>,
    pub report_hash: BytesN<32>,
    pub supersedes: Option<BytesN<32>>,
    pub recorded_at: u64,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterObjectiveInput {
    pub owner: Address,
    pub delegate: Option<Address>,
    pub cadence_seconds: u32,
    pub spec_hash: BytesN<32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileCheckinInput {
    pub checkin_id: BytesN<32>,
    pub status: CheckinStatus,
    pub confidence_bps: u32,
    pub evidence_hash: BytesN<32>,
    pub report_hash: BytesN<32>,
    pub supersedes: Option<BytesN<32>>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Objective(BytesN<32>, BytesN<32>, BytesN<32>),
    Checkin(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    CheckinIds(BytesN<32>, BytesN<32>, BytesN<32>),
    ObjectiveCount(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneObjectiveCheckinsContract;

#[contractimpl]
impl CapstoneObjectiveCheckinsContract {
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
            (symbol_short!("OBC_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn register_objective(
        env: Env,
        planner: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        objective_id: BytesN<32>,
        input: RegisterObjectiveInput,
    ) -> Result<Objective, ContractError> {
        planner.require_auth();
        Self::require_role(&env, &planner, &tenant_id, &project_id, Role::Planner)?;
        let key = DataKey::Objective(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, Objective>(&key) {
            if input.expected_version == 0
                && existing.owner == input.owner
                && existing.delegate == input.delegate
                && existing.cadence_seconds == input.cadence_seconds
                && existing.spec_hash == input.spec_hash
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if input.expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::ObjectiveExists);
        }
        if input.expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        if input.cadence_seconds == 0 || input.cadence_seconds > MAX_CADENCE_SECONDS {
            return Err(ContractError::InvalidCadence);
        }
        let count_key = DataKey::ObjectiveCount(tenant_id.clone(), project_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_OBJECTIVES {
            return Err(ContractError::ObjectiveLimitReached);
        }
        let now = env.ledger().timestamp();
        let objective = Objective {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            objective_id: objective_id.clone(),
            owner: input.owner,
            delegate: input.delegate,
            cadence_seconds: input.cadence_seconds,
            spec_hash: input.spec_hash,
            state: ObjectiveState::Open,
            current_status: CheckinStatus::OnTrack,
            latest_checkin_id: None,
            last_checkin_at: None,
            checkin_count: 0,
            expected_version: 1,
            created_at: now,
            updated_at: now,
        };
        let ids_key = DataKey::CheckinIds(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
        );
        env.storage().persistent().set(&key, &objective);
        let ids: Vec<BytesN<32>> = Vec::new(&env);
        env.storage().persistent().set(&ids_key, &ids);
        env.storage()
            .persistent()
            .set(&count_key, &count.checked_add(1).ok_or(ContractError::LimitReached)?);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &ids_key);
        Self::extend_persistent(&env, &count_key);
        env.events().publish(
            (symbol_short!("OBC_OBJ"),),
            (tenant_id, project_id, objective_id, objective.owner.clone()),
        );
        Ok(objective)
    }

    pub fn file_checkin(
        env: Env,
        filer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        objective_id: BytesN<32>,
        input: FileCheckinInput,
    ) -> Result<Checkin, ContractError> {
        filer.require_auth();
        Self::require_role(&env, &filer, &tenant_id, &project_id, Role::Participant)?;
        let objective_key = DataKey::Objective(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
        );
        let mut objective = Self::load_objective(&env, &objective_key)?;
        if filer != objective.owner && Some(filer.clone()) != objective.delegate {
            return Err(ContractError::WrongOwner);
        }
        if objective.state == ObjectiveState::Closed {
            return Err(ContractError::ObjectiveClosed);
        }
        let key = DataKey::Checkin(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
            input.checkin_id.clone(),
        );
        if let Some(existing) = env.storage().persistent().get::<_, Checkin>(&key) {
            if input.expected_version == 0
                && existing.filer == filer
                && existing.status == input.status
                && existing.confidence_bps == input.confidence_bps
                && existing.evidence_hash == input.evidence_hash
                && existing.report_hash == input.report_hash
                && existing.supersedes == input.supersedes
            {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if input.expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::CheckinExists);
        }
        if input.expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        if input.confidence_bps > MAX_CONFIDENCE_BPS {
            return Err(ContractError::InvalidConfidence);
        }
        if objective.checkin_count >= MAX_CHECKINS {
            return Err(ContractError::CheckinLimitReached);
        }
        if let Some(last_at) = objective.last_checkin_at {
            let now = env.ledger().timestamp();
            let elapsed = now
                .checked_sub(last_at)
                .ok_or(ContractError::CadenceViolation)?;
            if elapsed < u64::from(objective.cadence_seconds) {
                return Err(ContractError::CadenceViolation);
            }
        }
        if let Some(supersedes) = input.supersedes.clone() {
            if objective.latest_checkin_id != Some(supersedes) {
                return Err(ContractError::InvalidSupersedes);
            }
        }
        let now = env.ledger().timestamp();
        let checkin = Checkin {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            objective_id: objective_id.clone(),
            checkin_id: input.checkin_id.clone(),
            filer: filer.clone(),
            status: input.status,
            confidence_bps: input.confidence_bps,
            evidence_hash: input.evidence_hash,
            report_hash: input.report_hash,
            supersedes: input.supersedes,
            recorded_at: now,
            expected_version: 1,
        };
        let ids_key = DataKey::CheckinIds(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
        );
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .ok_or(ContractError::ObjectiveNotFound)?;
        ids.push_back(input.checkin_id.clone());
        env.storage().persistent().set(&key, &checkin);
        env.storage().persistent().set(&ids_key, &ids);
        objective.latest_checkin_id = Some(input.checkin_id.clone());
        objective.last_checkin_at = Some(now);
        objective.checkin_count = objective
            .checkin_count
            .checked_add(1)
            .ok_or(ContractError::LimitReached)?;
        objective.current_status = input.status;
        objective.state = if input.status == CheckinStatus::Closed {
            ObjectiveState::Closed
        } else {
            ObjectiveState::Open
        };
        objective.expected_version = objective
            .expected_version
            .checked_add(1)
            .ok_or(ContractError::VersionMismatch)?;
        objective.updated_at = now;
        env.storage().persistent().set(&objective_key, &objective);
        Self::extend_persistent(&env, &key);
        Self::extend_persistent(&env, &ids_key);
        Self::extend_persistent(&env, &objective_key);
        env.events().publish(
            (symbol_short!("OBC_CHK"),),
            (tenant_id, project_id, objective_id, input.checkin_id, input.status),
        );
        Ok(checkin)
    }

    pub fn get_checkin(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        objective_id: BytesN<32>,
        checkin_id: BytesN<32>,
    ) -> Result<Checkin, ContractError> {
        let key = DataKey::Checkin(tenant_id, project_id, objective_id, checkin_id);
        let checkin: Checkin = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::CheckinNotFound)?;
        Self::extend_persistent(&env, &key);
        Ok(checkin)
    }

    pub fn get_latest(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        objective_id: BytesN<32>,
    ) -> Result<Checkin, ContractError> {
        let objective_key = DataKey::Objective(
            tenant_id.clone(),
            project_id.clone(),
            objective_id.clone(),
        );
        let objective = Self::load_objective(&env, &objective_key)?;
        let latest = objective
            .latest_checkin_id
            .ok_or(ContractError::CheckinNotFound)?;
        Self::get_checkin(env, tenant_id, project_id, objective_id, latest)
    }

    pub fn get_checkin_ids(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        objective_id: BytesN<32>,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        let key = DataKey::CheckinIds(tenant_id, project_id, objective_id);
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ObjectiveNotFound)?;
        Self::extend_persistent(&env, &key);
        Ok(ids)
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
