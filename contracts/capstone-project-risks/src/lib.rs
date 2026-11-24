#![no_std]
#![allow(clippy::too_many_arguments)]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_PAGE: u32 = 32;
const CONTRACT_VERSION: u32 = 1;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    RequestConflict = 4,
    StaleVersion = 5,
    NotFound = 6,
    AlreadyExists = 7,
    InvalidInput = 8,
    InvalidTransition = 9,
    LimitReached = 10,
    InvalidPage = 11,
    ArithmeticError = 12,
    RoleExpired = 13,
    TerminalRisk = 14,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub active: bool,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestRecord {
    pub input_hash: BytesN<32>,
    pub result: u32,
}

const MAX_RISKS: u32 = 128;
/// Longest allowed gap between now and a scheduled review: 366 days.
const MAX_REVIEW_HORIZON: u64 = 31_622_400;

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Manager = 1,
    Reviewer = 2,
    Viewer = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    Schedule = 1,
    Budget = 2,
    Scope = 3,
    Technical = 4,
    People = 5,
    External = 6,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskStatus {
    Identified = 1,
    Mitigating = 2,
    Accepted = 3,
    Closed = 4,
    Materialized = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Team = 1,
    Restricted = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskInput {
    pub risk_id: BytesN<32>,
    pub category: Category,
    pub likelihood: u32,
    pub impact: u32,
    pub owner: Address,
    pub mitigation_hash: BytesN<32>,
    pub contingency_hash: BytesN<32>,
    pub next_review_at: u64,
    pub visibility: Visibility,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskRecord {
    pub risk_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub category: Category,
    pub likelihood: u32,
    pub impact: u32,
    pub score: u32,
    pub owner: Address,
    pub mitigation_hash: BytesN<32>,
    pub contingency_hash: BytesN<32>,
    pub review_hash: BytesN<32>,
    pub next_review_at: u64,
    pub last_reviewed_at: u64,
    pub status: RiskStatus,
    pub visibility: Visibility,
    pub created_at: u64,
    pub updated_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskPage {
    pub ids: Vec<BytesN<32>>,
    pub next: u32,
    pub has_more: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Risk(BytesN<32>, BytesN<32>, BytesN<32>),
    RiskIndex(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneProjectRisks;

#[contractimpl]
impl CapstoneProjectRisks {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish((symbol_short!("RSK_INIT"),), (admin,));
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
        expires_at: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        if active && expires_at != 0 && expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        let grant = RoleGrant {
            active,
            expires_at: if active { expires_at } else { 0 },
        };
        env.storage().persistent().set(&key, &grant);
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("RSK_ROLE"),),
            (
                tenant_id,
                project_id,
                account,
                role as u32,
                active,
                grant.expires_at,
            ),
        );
        Ok(())
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    pub fn register_risk(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        input: RiskInput,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&input.risk_id.to_array());
        extra.extend_from_array(&(input.category as u32).to_be_bytes());
        extra.extend_from_array(&input.likelihood.to_be_bytes());
        extra.extend_from_array(&input.impact.to_be_bytes());
        extra.append(&input.owner.clone().to_xdr(&env));
        extra.extend_from_array(&input.mitigation_hash.to_array());
        extra.extend_from_array(&input.contingency_hash.to_array());
        extra.extend_from_array(&input.next_review_at.to_be_bytes());
        extra.extend_from_array(&(input.visibility as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP062_REGISTER",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        Self::validate_assessment(input.likelihood, input.impact)?;
        if !Self::nonzero(&input.risk_id)
            || !Self::nonzero(&input.mitigation_hash)
            || !Self::nonzero(&input.contingency_hash)
        {
            return Err(ContractError::InvalidInput);
        }
        let now = env.ledger().timestamp();
        Self::validate_review_date(now, input.next_review_at)?;
        let key = DataKey::Risk(tenant_id.clone(), project_id.clone(), input.risk_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let index_key = DataKey::RiskIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        if index.len() >= MAX_RISKS {
            return Err(ContractError::LimitReached);
        }
        let score = input.likelihood * input.impact;
        let record = RiskRecord {
            risk_id: input.risk_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            category: input.category,
            likelihood: input.likelihood,
            impact: input.impact,
            score,
            owner: input.owner,
            mitigation_hash: input.mitigation_hash,
            contingency_hash: input.contingency_hash,
            review_hash: BytesN::from_array(&env, &[0u8; 32]),
            next_review_at: input.next_review_at,
            last_reviewed_at: 0,
            status: RiskStatus::Identified,
            visibility: input.visibility,
            created_at: now,
            updated_at: now,
            version: 1,
        };
        index.push_back(input.risk_id.clone());
        env.storage().persistent().set(&key, &record);
        env.storage().persistent().set(&index_key, &index);
        Self::extend(&env, &key);
        Self::extend(&env, &index_key);
        Self::remember(&env, &tenant_id, &project_id, &request_id, &input_hash, 1);
        env.events().publish(
            (symbol_short!("RSK_REG"),),
            (
                tenant_id,
                project_id,
                input.risk_id,
                input.category as u32,
                score,
                1u32,
            ),
        );
        Ok(1)
    }

    pub fn update_assessment(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        likelihood: u32,
        impact: u32,
        mitigation_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::update_assessment_inner(
            env,
            manager,
            tenant_id,
            project_id,
            request_id,
            risk_id,
            expected_version,
            Some(likelihood),
            Some(impact),
            Some(mitigation_hash),
            None,
        )
    }

    pub fn update_contingency(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        contingency_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::update_assessment_inner(
            env,
            manager,
            tenant_id,
            project_id,
            request_id,
            risk_id,
            expected_version,
            None,
            None,
            None,
            Some(contingency_hash),
        )
    }

    pub fn record_review(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        review_hash: BytesN<32>,
        next_review_at: u64,
    ) -> Result<u32, ContractError> {
        actor.require_auth();
        Self::require_any(
            &env,
            &actor,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Reviewer],
        )?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&risk_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&review_hash.to_array());
        extra.extend_from_array(&next_review_at.to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP062_REVIEW",
            &tenant_id,
            &project_id,
            &request_id,
            &actor,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if !Self::nonzero(&review_hash) {
            return Err(ContractError::InvalidInput);
        }
        let now = env.ledger().timestamp();
        Self::validate_review_date(now, next_review_at)?;
        let (key, mut record) = Self::load_for_update(
            &env,
            &tenant_id,
            &project_id,
            &risk_id,
            expected_version,
            false,
        )?;
        record.review_hash = review_hash;
        record.next_review_at = next_review_at;
        record.last_reviewed_at = now;
        record.updated_at = now;
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("RSK_REV"),),
            (
                tenant_id,
                project_id,
                risk_id,
                next_review_at,
                record.version,
            ),
        );
        Ok(record.version)
    }

    pub fn transition_status(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        to: RiskStatus,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&risk_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.extend_from_array(&(to as u32).to_be_bytes());
        let input_hash = Self::hash_input(
            &env,
            b"CAP062_STATUS",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let (key, mut record) = Self::load_for_update(
            &env,
            &tenant_id,
            &project_id,
            &risk_id,
            expected_version,
            true,
        )?;
        if !Self::transition_allowed(record.status, to) {
            return Err(ContractError::InvalidTransition);
        }
        let from = record.status;
        record.status = to;
        record.updated_at = env.ledger().timestamp();
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("RSK_STS"),),
            (
                tenant_id,
                project_id,
                risk_id,
                from as u32,
                to as u32,
                record.version,
            ),
        );
        Ok(record.version)
    }

    pub fn reassign_owner(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        new_owner: Address,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&risk_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        extra.append(&new_owner.clone().to_xdr(&env));
        let input_hash = Self::hash_input(
            &env,
            b"CAP062_OWNER",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        let (key, mut record) = Self::load_for_update(
            &env,
            &tenant_id,
            &project_id,
            &risk_id,
            expected_version,
            false,
        )?;
        record.owner = new_owner.clone();
        record.updated_at = env.ledger().timestamp();
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("RSK_OWN"),),
            (tenant_id, project_id, risk_id, new_owner, record.version),
        );
        Ok(record.version)
    }

    /// Restricted risks the caller may not see are reported as `NotFound`
    /// so their existence is not disclosed.
    pub fn get_risk(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        risk_id: BytesN<32>,
    ) -> Result<RiskRecord, ContractError> {
        viewer.require_auth();
        let role = Self::require_any(
            &env,
            &viewer,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Reviewer, Role::Viewer],
        )?;
        let key = DataKey::Risk(tenant_id, project_id, risk_id);
        let record: RiskRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if !Self::visible_to(role, &viewer, &record) {
            return Err(ContractError::NotFound);
        }
        Self::extend(&env, &key);
        Ok(record)
    }

    pub fn list_risks(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        from: u32,
        limit: u32,
    ) -> Result<RiskPage, ContractError> {
        viewer.require_auth();
        let role = Self::require_any(
            &env,
            &viewer,
            &tenant_id,
            &project_id,
            &[Role::Manager, Role::Reviewer, Role::Viewer],
        )?;
        let index_key = DataKey::RiskIndex(tenant_id.clone(), project_id.clone());
        let index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(&env));
        let (start, end) = Self::page_range(index.len(), from, limit)?;
        let mut ids = Vec::new(&env);
        let mut cursor = start;
        while cursor < end {
            if let Some(id) = index.get(cursor) {
                let key = DataKey::Risk(tenant_id.clone(), project_id.clone(), id.clone());
                if let Some(record) = env.storage().persistent().get::<DataKey, RiskRecord>(&key) {
                    if Self::visible_to(role, &viewer, &record) {
                        ids.push_back(id);
                    }
                }
            }
            cursor += 1;
        }
        if env.storage().persistent().has(&index_key) {
            Self::extend(&env, &index_key);
        }
        Ok(RiskPage {
            ids,
            next: end,
            has_more: end < index.len(),
        })
    }

    pub fn review_due(
        env: Env,
        viewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        risk_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let record = Self::get_risk(env.clone(), viewer, tenant_id, project_id, risk_id)?;
        Ok(!Self::is_terminal(record.status) && env.ledger().timestamp() >= record.next_review_at)
    }
}

impl CapstoneProjectRisks {
    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
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
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Deny by default: succeeds only when the account holds one of `roles`,
    /// active and unexpired, for exactly this tenant and project.
    fn require_any(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        roles: &[Role],
    ) -> Result<Role, ContractError> {
        let now = env.ledger().timestamp();
        let mut expired = false;
        for role in roles.iter() {
            let key = DataKey::Role(
                account.clone(),
                tenant_id.clone(),
                project_id.clone(),
                *role,
            );
            if let Some(grant) = env.storage().persistent().get::<DataKey, RoleGrant>(&key) {
                if !grant.active {
                    continue;
                }
                if grant.expires_at != 0 && now >= grant.expires_at {
                    expired = true;
                    continue;
                }
                Self::extend(env, &key);
                return Ok(*role);
            }
        }
        if expired {
            Err(ContractError::RoleExpired)
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn hash_input(
        env: &Env,
        domain: &[u8],
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        actor: &Address,
        extra: &Bytes,
    ) -> BytesN<32> {
        let mut input = Bytes::new(env);
        input.append(&Bytes::from_slice(env, domain));
        input.extend_from_array(&tenant_id.to_array());
        input.extend_from_array(&project_id.to_array());
        input.extend_from_array(&request_id.to_array());
        input.append(&actor.clone().to_xdr(env));
        input.append(extra);
        env.crypto().sha256(&input).to_bytes()
    }

    /// Returns the recorded result when this exact request already succeeded.
    /// The same `request_id` with a different payload or actor is rejected.
    fn replay(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        input_hash: &BytesN<32>,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(record) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestRecord>(&key)
        {
            if &record.input_hash != input_hash {
                return Err(ContractError::RequestConflict);
            }
            Self::extend(env, &key);
            return Ok(Some(record.result));
        }
        Ok(None)
    }

    fn remember(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        input_hash: &BytesN<32>,
        result: u32,
    ) {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let record = RequestRecord {
            input_hash: input_hash.clone(),
            result,
        };
        env.storage().persistent().set(&key, &record);
        Self::extend(env, &key);
    }

    fn nonzero(hash: &BytesN<32>) -> bool {
        hash.to_array().iter().any(|byte| *byte != 0)
    }

    fn bump(version: u32) -> Result<u32, ContractError> {
        version.checked_add(1).ok_or(ContractError::ArithmeticError)
    }

    fn page_range(len: u32, from: u32, limit: u32) -> Result<(u32, u32), ContractError> {
        if limit == 0 || limit > MAX_PAGE || from > len {
            return Err(ContractError::InvalidPage);
        }
        let end = from
            .checked_add(limit)
            .ok_or(ContractError::ArithmeticError)?;
        Ok((from, if end > len { len } else { end }))
    }
}

impl CapstoneProjectRisks {
    fn update_assessment_inner(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        risk_id: BytesN<32>,
        expected_version: u32,
        likelihood: Option<u32>,
        impact: Option<u32>,
        mitigation_hash: Option<BytesN<32>>,
        contingency_hash: Option<BytesN<32>>,
    ) -> Result<u32, ContractError> {
        manager.require_auth();
        Self::require_any(&env, &manager, &tenant_id, &project_id, &[Role::Manager])?;
        let mut extra = Bytes::new(&env);
        extra.extend_from_array(&risk_id.to_array());
        extra.extend_from_array(&expected_version.to_be_bytes());
        match likelihood {
            Some(value) => {
                extra.extend_from_array(&[1u8]);
                extra.extend_from_array(&value.to_be_bytes());
            }
            None => extra.extend_from_array(&[0u8]),
        }
        match impact {
            Some(value) => {
                extra.extend_from_array(&[1u8]);
                extra.extend_from_array(&value.to_be_bytes());
            }
            None => extra.extend_from_array(&[0u8]),
        }
        match mitigation_hash.as_ref() {
            Some(hash) => {
                extra.extend_from_array(&[1u8]);
                extra.extend_from_array(&hash.to_array());
            }
            None => extra.extend_from_array(&[0u8]),
        }
        match contingency_hash.as_ref() {
            Some(hash) => {
                extra.extend_from_array(&[1u8]);
                extra.extend_from_array(&hash.to_array());
            }
            None => extra.extend_from_array(&[0u8]),
        }
        let input_hash = Self::hash_input(
            &env,
            b"CAP062_ASSESS",
            &tenant_id,
            &project_id,
            &request_id,
            &manager,
            &extra,
        );
        if let Some(done) = Self::replay(&env, &tenant_id, &project_id, &request_id, &input_hash)? {
            return Ok(done);
        }
        if likelihood.is_none()
            && impact.is_none()
            && mitigation_hash.is_none()
            && contingency_hash.is_none()
        {
            return Err(ContractError::InvalidInput);
        }
        for hash in [mitigation_hash.as_ref(), contingency_hash.as_ref()]
            .into_iter()
            .flatten()
        {
            if !Self::nonzero(hash) {
                return Err(ContractError::InvalidInput);
            }
        }
        let (key, mut record) = Self::load_for_update(
            &env,
            &tenant_id,
            &project_id,
            &risk_id,
            expected_version,
            false,
        )?;
        record.likelihood = likelihood.unwrap_or(record.likelihood);
        record.impact = impact.unwrap_or(record.impact);
        Self::validate_assessment(record.likelihood, record.impact)?;
        record.score = record.likelihood * record.impact;
        if let Some(hash) = mitigation_hash {
            record.mitigation_hash = hash;
        }
        if let Some(hash) = contingency_hash {
            record.contingency_hash = hash;
        }
        record.updated_at = env.ledger().timestamp();
        record.version = Self::bump(record.version)?;
        Self::save(&env, &key, &record);
        Self::remember(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &input_hash,
            record.version,
        );
        env.events().publish(
            (symbol_short!("RSK_UPD"),),
            (tenant_id, project_id, risk_id, record.score, record.version),
        );
        Ok(record.version)
    }

    fn validate_assessment(likelihood: u32, impact: u32) -> Result<(), ContractError> {
        if !(1..=5).contains(&likelihood) || !(1..=5).contains(&impact) {
            return Err(ContractError::InvalidInput);
        }
        Ok(())
    }

    fn validate_review_date(now: u64, next_review_at: u64) -> Result<(), ContractError> {
        let horizon = now
            .checked_add(MAX_REVIEW_HORIZON)
            .ok_or(ContractError::ArithmeticError)?;
        if next_review_at <= now || next_review_at > horizon {
            return Err(ContractError::InvalidInput);
        }
        Ok(())
    }

    fn is_terminal(status: RiskStatus) -> bool {
        matches!(status, RiskStatus::Closed | RiskStatus::Materialized)
    }

    fn transition_allowed(from: RiskStatus, to: RiskStatus) -> bool {
        matches!(
            (from, to),
            (RiskStatus::Identified, RiskStatus::Mitigating)
                | (RiskStatus::Identified, RiskStatus::Accepted)
                | (RiskStatus::Identified, RiskStatus::Closed)
                | (RiskStatus::Identified, RiskStatus::Materialized)
                | (RiskStatus::Mitigating, RiskStatus::Accepted)
                | (RiskStatus::Mitigating, RiskStatus::Closed)
                | (RiskStatus::Mitigating, RiskStatus::Materialized)
                | (RiskStatus::Accepted, RiskStatus::Mitigating)
                | (RiskStatus::Accepted, RiskStatus::Closed)
                | (RiskStatus::Accepted, RiskStatus::Materialized)
        )
    }

    fn visible_to(role: Role, viewer: &Address, record: &RiskRecord) -> bool {
        record.visibility == Visibility::Team || role != Role::Viewer || record.owner == *viewer
    }

    fn load_for_update(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        risk_id: &BytesN<32>,
        expected_version: u32,
        allow_terminal: bool,
    ) -> Result<(DataKey, RiskRecord), ContractError> {
        let key = DataKey::Risk(tenant_id.clone(), project_id.clone(), risk_id.clone());
        let record: RiskRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if !allow_terminal && Self::is_terminal(record.status) {
            return Err(ContractError::TerminalRisk);
        }
        Ok((key, record))
    }

    fn save(env: &Env, key: &DataKey, record: &RiskRecord) {
        env.storage().persistent().set(key, record);
        Self::extend(env, key);
    }
}
