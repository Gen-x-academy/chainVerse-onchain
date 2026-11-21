#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_DECISIONS: u32 = 64;
const MAX_APPROVERS: u32 = 8;
const MAX_PAGE: u32 = 64;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    VersionConflict = 4,
    VersionOverflow = 5,
    RequestConflict = 6,
    NotFound = 7,
    AlreadyExists = 8,
    InvalidInput = 9,
    InvalidState = 10,
    AlreadyApproved = 11,
    ThresholdNotMet = 12,
    NotEffective = 13,
    CollectionFull = 14,
    InvalidPage = 15,
    InvalidSupersession = 16,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Coordinator = 2,
    Approver = 3,
    Viewer = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionState {
    Proposed = 1,
    Approved = 2,
    Effective = 3,
    Superseded = 4,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    RoleCount(BytesN<32>),
    Version(BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Decision(BytesN<32>, BytesN<32>, BytesN<32>),
    DecisionIndex(BytesN<32>, BytesN<32>),
    Approval(BytesN<32>, BytesN<32>, BytesN<32>, Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRecord {
    pub input_hash: BytesN<32>,
    pub result_id: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionRecord {
    pub id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub content_hash: BytesN<32>,
    pub evidence_hash: BytesN<32>,
    pub effective_at: u64,
    pub approvals_required: u32,
    pub state: DecisionState,
    pub supersedes: Option<BytesN<32>>,
    pub approvals: Vec<Address>,
    pub created_at: u64,
    pub version: u32,
}

struct RequestToken {
    replay: Option<RequestRecord>,
    next_version: u32,
}

#[contract]
pub struct CapstoneProjectDecisionsContract;

#[contractimpl]
impl CapstoneProjectDecisionsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("DCINIT"),), (admin,));
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
        let role_key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        if !env.storage().persistent().has(&role_key) {
            let count_key = DataKey::RoleCount(tenant_id.clone());
            let count: u32 = env
                .storage()
                .persistent()
                .get(&count_key)
                .unwrap_or(0);
            if count >= MAX_ROLES {
                return Err(ContractError::CollectionFull);
            }
            let next = count.checked_add(1).ok_or(ContractError::VersionOverflow)?;
            Self::write_u32(&env, &count_key, next);
        }
        Self::write_bool(&env, &role_key, true);
        env.events()
            .publish((symbol_short!("ROLEGR"),), (tenant_id, account, role as u32));
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
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::NotFound);
        }
        Self::write_bool(&env, &key, false);
        env.events()
            .publish((symbol_short!("ROLERV"),), (tenant_id, account, role as u32));
        Ok(())
    }

    pub fn propose(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        decision_id: BytesN<32>,
        content_hash: BytesN<32>,
        evidence_hash: BytesN<32>,
        effective_at: u64,
        approvals_required: u32,
        supersedes: Option<BytesN<32>>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        if approvals_required == 0 || approvals_required > MAX_APPROVERS {
            return Err(ContractError::InvalidInput);
        }
        if !Self::nonzero(&content_hash) || !Self::nonzero(&evidence_hash) {
            return Err(ContractError::InvalidInput);
        }
        let decision_key = DataKey::Decision(
            tenant_id.clone(),
            project_id.clone(),
            decision_id.clone(),
        );
        if env.storage().persistent().has(&decision_key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::DecisionIndex(tenant_id.clone(), project_id.clone());
        let index = Self::load_index(&env, &index_key);
        if index.len() >= MAX_DECISIONS {
            return Err(ContractError::CollectionFull);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&decision_id.to_array());
        extra.extend_from_array(&content_hash.to_array());
        extra.extend_from_array(&evidence_hash.to_array());
        extra.extend_from_array(&effective_at.to_be_bytes());
        extra.extend_from_array(&approvals_required.to_be_bytes());
        match supersedes.as_ref() {
            Some(previous) => {
                extra.extend_from_array(&[1u8]);
                extra.extend_from_array(&previous.to_array());
            }
            None => extra.extend_from_array(&[0u8]),
        }
        let input_hash = Self::hash_input(
            &env,
            b"CAP070_PROPOSE",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        if let Some(previous_id) = supersedes.as_ref() {
            let previous_key = DataKey::Decision(
                tenant_id.clone(),
                project_id.clone(),
                previous_id.clone(),
            );
            let mut previous: DecisionRecord = env
                .storage()
                .persistent()
                .get(&previous_key)
                .ok_or(ContractError::InvalidSupersession)?;
            if previous.state != DecisionState::Effective {
                return Err(ContractError::InvalidSupersession);
            }
            previous.state = DecisionState::Superseded;
            Self::write_decision(&env, &previous_key, &previous);
            env.events().publish(
                (symbol_short!("DCSUP"),),
                (tenant_id.clone(), project_id.clone(), previous_id.clone(), decision_id.clone()),
            );
        }
        let decision = DecisionRecord {
            id: decision_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            content_hash,
            evidence_hash,
            effective_at,
            approvals_required,
            state: DecisionState::Proposed,
            supersedes,
            approvals: Vec::new(&env),
            created_at: env.ledger().timestamp(),
            version: token.next_version,
        };
        let mut updated_index = index;
        updated_index.push_back(decision_id.clone());
        Self::write_decision(&env, &decision_key, &decision);
        Self::write_vec(&env, &index_key, &updated_index);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &decision_id,
        );
        env.events().publish(
            (symbol_short!("DCPROP"),),
            (tenant_id, project_id, decision_id, token.next_version),
        );
        Ok(token.next_version)
    }

    pub fn approve(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        decision_id: BytesN<32>,
        approval_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Approver)?;
        if !Self::nonzero(&approval_hash) {
            return Err(ContractError::InvalidInput);
        }
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&decision_id.to_array());
        extra.extend_from_array(&approval_hash.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP070_APPROVE",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let decision_key = DataKey::Decision(
            tenant_id.clone(),
            project_id.clone(),
            decision_id.clone(),
        );
        let mut decision: DecisionRecord = env
            .storage()
            .persistent()
            .get(&decision_key)
            .ok_or(ContractError::NotFound)?;
        if decision.state != DecisionState::Proposed {
            return Err(ContractError::InvalidState);
        }
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            decision_id.clone(),
            actor.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::AlreadyApproved);
        }
        if decision.approvals.len() >= MAX_APPROVERS {
            return Err(ContractError::CollectionFull);
        }
        decision.approvals.push_back(actor.clone());
        let count = decision.approvals.len();
        if count >= decision.approvals_required {
            decision.state = DecisionState::Approved;
        }
        Self::write_bool(&env, &approval_key, true);
        Self::write_decision(&env, &decision_key, &decision);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &decision_id,
        );
        env.events().publish(
            (symbol_short!("DCAPPR"),),
            (tenant_id, project_id, decision_id, count, decision.state as u32),
        );
        Ok(token.next_version)
    }

    pub fn activate(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        decision_id: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Coordinator)?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&decision_id.to_array());
        let input_hash = Self::hash_input(
            &env,
            b"CAP070_ACTIVATE",
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &extra,
        );
        let token = Self::begin_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            expected_version,
            &input_hash,
        )?;
        if let Some(record) = token.replay.as_ref() {
            Self::touch_request(&env, &tenant_id, &project_id, &request_id);
            return Ok(record.version);
        }
        let key = DataKey::Decision(
            tenant_id.clone(),
            project_id.clone(),
            decision_id.clone(),
        );
        let mut decision: DecisionRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if decision.state != DecisionState::Approved {
            return Err(ContractError::InvalidState);
        }
        if decision.approvals.len() < decision.approvals_required {
            return Err(ContractError::ThresholdNotMet);
        }
        if env.ledger().timestamp() < decision.effective_at {
            return Err(ContractError::NotEffective);
        }
        decision.state = DecisionState::Effective;
        Self::write_decision(&env, &key, &decision);
        Self::finish_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &token,
            &input_hash,
            &decision_id,
        );
        env.events().publish(
            (symbol_short!("DCACT"),),
            (
                tenant_id,
                project_id,
                decision_id,
                DecisionState::Effective as u32,
            ),
        );
        Ok(token.next_version)
    }

    pub fn get_decision(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        decision_id: BytesN<32>,
    ) -> Result<DecisionRecord, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        let key = DataKey::Decision(tenant_id, project_id, decision_id);
        let decision = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(decision)
    }

    pub fn list_decisions(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Result<Vec<BytesN<32>>, ContractError> {
        Self::require_viewer(&env, &viewer, &tenant_id)?;
        if limit == 0 || limit > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }
        let key = DataKey::DecisionIndex(tenant_id, project_id);
        let index = Self::load_index(&env, &key);
        if from > index.len() {
            return Err(ContractError::InvalidPage);
        }
        let end = from.checked_add(limit).ok_or(ContractError::VersionOverflow)?;
        let last = if end > index.len() { index.len() } else { end };
        let mut page = Vec::new(&env);
        let mut cursor = from;
        while cursor < last {
            if let Some(id) = index.get(cursor) {
                page.push_back(id);
            }
            cursor += 1;
        }
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(page)
    }

    pub fn project_version(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::load_version(&env, &tenant_id, &project_id)
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
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn has_role(env: &Env, tenant_id: &BytesN<32>, account: &Address, role: Role) -> bool {
        let key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        let active = env
            .storage()
            .persistent()
            .get::<DataKey, bool>(&key)
            .unwrap_or(false);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        active
    }

    fn require_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        actor.require_auth();
        if Self::has_role(env, tenant_id, actor, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn require_viewer(
        env: &Env,
        viewer: &Address,
        tenant_id: &BytesN<32>,
    ) -> Result<(), ContractError> {
        viewer.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *viewer == admin {
            return Ok(());
        }
        let roles = [Role::Coordinator, Role::Approver, Role::Viewer];
        for role in roles.iter() {
            if Self::has_role(env, tenant_id, viewer, *role) {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
    }

    fn begin_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        expected_version: u32,
        input_hash: &BytesN<32>,
    ) -> Result<RequestToken, ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(record) = env.storage().persistent().get::<DataKey, RequestRecord>(&key) {
            if &record.input_hash != input_hash {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            return Ok(RequestToken {
                replay: Some(record),
                next_version: 0,
            });
        }
        let current = Self::load_version(env, tenant_id, project_id);
        if expected_version != current {
            return Err(ContractError::VersionConflict);
        }
        let next_version = current
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        Ok(RequestToken {
            replay: None,
            next_version,
        })
    }

    fn finish_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        token: &RequestToken,
        input_hash: &BytesN<32>,
        result_id: &BytesN<32>,
    ) {
        let request_key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let record = RequestRecord {
            input_hash: input_hash.clone(),
            result_id: result_id.clone(),
            version: token.next_version,
        };
        Self::write_record(env, &request_key, &record);
        let version_key = DataKey::Version(tenant_id.clone(), project_id.clone());
        Self::write_u32(env, &version_key, token.next_version);
    }

    fn touch_request(env: &Env, tenant_id: &BytesN<32>, project_id: &BytesN<32>, request_id: &BytesN<32>) {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_version(env: &Env, tenant_id: &BytesN<32>, project_id: &BytesN<32>) -> u32 {
        let key = DataKey::Version(tenant_id.clone(), project_id.clone());
        let version: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        if env.storage().persistent().has(&key) {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        version
    }

    fn load_index(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        let index = env
            .storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env));
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        index
    }

    fn hash_input(
        env: &Env,
        domain: &[u8],
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        expected_version: u32,
        extra: &Bytes,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, domain));
        input.extend_from_array(&tenant_id.to_array());
        input.extend_from_array(&project_id.to_array());
        input.extend_from_array(&request_id.to_array());
        input.extend_from_array(&expected_version.to_be_bytes());
        input.append(extra);
        env.crypto().sha256(&input)
    }

    fn nonzero(hash: &BytesN<32>) -> bool {
        hash.to_array().iter().any(|byte| *byte != 0)
    }

    fn write_bool(env: &Env, key: &DataKey, value: bool) {
        env.storage().persistent().set(key, &value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_u32(env: &Env, key: &DataKey, value: u32) {
        env.storage().persistent().set(key, &value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_vec(env: &Env, key: &DataKey, value: &Vec<BytesN<32>>) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_record(env: &Env, key: &DataKey, value: &RequestRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn write_decision(env: &Env, key: &DataKey, value: &DecisionRecord) {
        env.storage().persistent().set(key, value);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
