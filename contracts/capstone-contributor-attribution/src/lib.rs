#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_RECORDS: u32 = 16;
const MAX_ROLES: u32 = 16;
const MAX_REQUESTS: u32 = 128;
const MAX_APPROVALS: u32 = 4;

const OP_POLICY: u32 = 1;
const OP_DECLARE: u32 = 2;
const OP_APPROVE: u32 = 3;
const OP_DISPUTE: u32 = 4;
const OP_RESOLVE: u32 = 5;
const OP_LINK: u32 = 6;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidRoleCode = 6,
    InvalidPolicy = 7,
    InvalidState = 8,
    StaleVersion = 9,
    RequestConflict = 10,
    SelfApprovalNotAllowed = 11,
    ArithmeticOverflow = 12,
    IndexFull = 13,
    RoleLimitReached = 14,
    RoleAlreadyGranted = 15,
    InvalidHash = 16,
    AlreadyApproved = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Reviewer = 2,
    Contributor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Taxonomy {
    Conceptualization = 1,
    Methodology = 2,
    Software = 3,
    DataCuration = 4,
    Validation = 5,
    Writing = 6,
    Supervision = 7,
    FundingAcquisition = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributionState {
    Declared = 1,
    Approved = 2,
    Disputed = 3,
    Rejected = 4,
    Linked = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RoleIndex(BytesN<32>, BytesN<32>),
    RequestReceipt(BytesN<32>, BytesN<32>, BytesN<32>),
    RequestIndex(BytesN<32>, BytesN<32>),
    Policy(BytesN<32>, BytesN<32>),
    AttributionRecord(BytesN<32>, BytesN<32>, BytesN<32>),
    RecordIndex(BytesN<32>, BytesN<32>),
    Approval(BytesN<32>, BytesN<32>, BytesN<32>, Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub account: Address,
    pub role: Role,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub result_id: BytesN<32>,
    pub version: u32,
    pub op: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributionPolicy {
    pub required_approvals: u32,
    pub max_records: u32,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributionRecord {
    pub record_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub contributor: Address,
    pub role_code: Taxonomy,
    pub contribution_hash: BytesN<32>,
    pub dispute_hash: BytesN<32>,
    pub output_hash: BytesN<32>,
    pub state: AttributionState,
    pub approvals: u32,
    pub version: u32,
}

#[contract]
pub struct CapstoneContributorAttribution;

#[contractimpl]
impl CapstoneContributorAttribution {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_platform_admin(&env, &admin)?;
        let key = DataKey::RoleIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<RoleGrant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if index.len() >= MAX_ROLES {
            return Err(ContractError::RoleLimitReached);
        }
        for i in 0..index.len() {
            let grant = index.get(i).unwrap();
            if grant.account == account.clone() && grant.role == role {
                return Err(ContractError::RoleAlreadyGranted);
            }
        }
        index.push_back(RoleGrant {
            account: account.clone(),
            role,
        });
        env.storage().persistent().set(&key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("ROLEGRT"),),
            (tenant_id, project_id, role as u32),
        );
        Ok(())
    }

    pub fn has_role(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> bool {
        Self::role_granted(&env, &tenant_id, &project_id, &account, role)
    }

    pub fn configure_policy(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        required_approvals: u32,
        max_records: u32,
    ) -> Result<AttributionPolicy, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_POLICY,
            expected_version,
        )?
        .is_some()
        {
            return Self::load_policy(&env, &tenant_id, &project_id);
        }
        if required_approvals == 0 || required_approvals > MAX_APPROVALS {
            return Err(ContractError::InvalidPolicy);
        }
        if max_records == 0 || max_records > MAX_RECORDS {
            return Err(ContractError::InvalidPolicy);
        }
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let policy = AttributionPolicy {
            required_approvals,
            max_records,
            version: expected_version
                .checked_add(1)
                .ok_or(ContractError::ArithmeticOverflow)?,
        };
        Self::write_policy(&env, &tenant_id, &project_id, &policy);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &tenant_id,
            OP_POLICY,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRCFG"),),
            (tenant_id, project_id, required_approvals, max_records),
        );
        Ok(policy)
    }

    pub fn declare_contribution(
        env: Env,
        contributor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        role_code: Taxonomy,
        contribution_hash: BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        Self::require_role(
            &env,
            &contributor,
            &tenant_id,
            &project_id,
            Role::Contributor,
        )?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_DECLARE,
            expected_version,
        )? {
            return Self::load_record(&env, &tenant_id, &project_id, &result_id);
        }
        let policy = Self::load_policy(&env, &tenant_id, &project_id)?;
        if !Self::valid_role_code(role_code) {
            return Err(ContractError::InvalidRoleCode);
        }
        if contribution_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidHash);
        }
        let key = DataKey::AttributionRecord(
            tenant_id.clone(),
            project_id.clone(),
            record_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::RecordIndex(tenant_id.clone(), project_id.clone()),
            &record_id,
            policy.max_records,
        )?;
        let record = AttributionRecord {
            record_id: record_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            contributor: contributor.clone(),
            role_code,
            contribution_hash,
            dispute_hash: Self::zero_hash(&env),
            output_hash: Self::zero_hash(&env),
            state: AttributionState::Declared,
            approvals: 0,
            version: expected_version
                .checked_add(1)
                .ok_or(ContractError::ArithmeticOverflow)?,
        };
        Self::write_record(&env, &record);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &record_id,
            OP_DECLARE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRDECL"),),
            (record_id, role_code as u32, contribution_hash),
        );
        Ok(record)
    }

    pub fn approve_attribution(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<AttributionRecord, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_APPROVE,
            expected_version,
        )? {
            return Self::load_record(&env, &tenant_id, &project_id, &result_id);
        }
        let mut record = Self::load_record(&env, &tenant_id, &project_id, &record_id)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != AttributionState::Declared {
            return Err(ContractError::InvalidState);
        }
        if record.contributor == reviewer.clone() {
            return Err(ContractError::SelfApprovalNotAllowed);
        }
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            record_id.clone(),
            reviewer.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::AlreadyApproved);
        }
        let policy = Self::load_policy(&env, &tenant_id, &project_id)?;
        let approvals = record
            .approvals
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        env.storage().persistent().set(&approval_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&approval_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        record.approvals = approvals;
        if approvals >= policy.required_approvals {
            record.state = AttributionState::Approved;
        }
        record.version = record
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_record(&env, &record);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &record_id,
            OP_APPROVE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRAPPR"),),
            (record_id, approvals, record.state as u32),
        );
        Ok(record)
    }

    pub fn dispute_attribution(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        dispute_hash: BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_DISPUTE,
            expected_version,
        )? {
            return Self::load_record(&env, &tenant_id, &project_id, &result_id);
        }
        let mut record = Self::load_record(&env, &tenant_id, &project_id, &record_id)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != AttributionState::Declared
            && record.state != AttributionState::Approved
        {
            return Err(ContractError::InvalidState);
        }
        if dispute_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidHash);
        }
        record.state = AttributionState::Disputed;
        record.dispute_hash = dispute_hash.clone();
        record.version = record
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_record(&env, &record);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &record_id,
            OP_DISPUTE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRDSP"),),
            (record_id, dispute_hash, record.state as u32),
        );
        Ok(record)
    }

    pub fn resolve_dispute(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        uphold: bool,
        resolution_hash: BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_RESOLVE,
            expected_version,
        )? {
            return Self::load_record(&env, &tenant_id, &project_id, &result_id);
        }
        let mut record = Self::load_record(&env, &tenant_id, &project_id, &record_id)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != AttributionState::Disputed {
            return Err(ContractError::InvalidState);
        }
        if resolution_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidHash);
        }
        record.state = if uphold {
            AttributionState::Rejected
        } else {
            AttributionState::Approved
        };
        record.dispute_hash = resolution_hash.clone();
        record.version = record
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_record(&env, &record);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &record_id,
            OP_RESOLVE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRRES"),),
            (record_id, resolution_hash, record.state as u32),
        );
        Ok(record)
    }

    pub fn link_output(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        output_hash: BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_LINK,
            expected_version,
        )? {
            return Self::load_record(&env, &tenant_id, &project_id, &result_id);
        }
        let mut record = Self::load_record(&env, &tenant_id, &project_id, &record_id)?;
        if record.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if record.state != AttributionState::Approved {
            return Err(ContractError::InvalidState);
        }
        if output_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidHash);
        }
        record.state = AttributionState::Linked;
        record.output_hash = output_hash.clone();
        record.version = record
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_record(&env, &record);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &record_id,
            OP_LINK,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("ATTRLINK"),),
            (record_id, output_hash, record.state as u32),
        );
        Ok(record)
    }

    pub fn get_policy(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<AttributionPolicy, ContractError> {
        Self::load_policy(&env, &tenant_id, &project_id)
    }

    pub fn get_record(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        Self::load_record(&env, &tenant_id, &project_id, &record_id)
    }

    pub fn has_approved(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        record_id: BytesN<32>,
        reviewer: Address,
    ) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Approval(
                tenant_id,
                project_id,
                record_id,
                reviewer,
            ))
            .unwrap_or(false)
    }

    pub fn record_count(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::load_index(&env, &DataKey::RecordIndex(tenant_id, project_id)).len()
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn require_platform_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if stored != admin.clone() {
            return Err(ContractError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn role_granted(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        account: &Address,
        role: Role,
    ) -> bool {
        let key = DataKey::RoleIndex(tenant_id.clone(), project_id.clone());
        let index: Vec<RoleGrant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..index.len() {
            let grant = index.get(i).unwrap();
            if grant.account == account.clone() && grant.role == role {
                return true;
            }
        }
        false
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if Self::role_granted(env, tenant_id, project_id, caller, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn request_result(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        op: u32,
        expected_version: u32,
    ) -> Result<Option<BytesN<32>>, ContractError> {
        let key = DataKey::RequestReceipt(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(receipt) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestReceipt>(&key)
        {
            if receipt.op != op || receipt.version != expected_version {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            return Ok(Some(receipt.result_id));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        result_id: &BytesN<32>,
        op: u32,
        version: u32,
    ) -> Result<(), ContractError> {
        let key = DataKey::RequestReceipt(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let index_key = DataKey::RequestIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(env));
        if index.len() >= MAX_REQUESTS {
            return Err(ContractError::IndexFull);
        }
        index.push_back(request_id.clone());
        env.storage().persistent().set(&index_key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let receipt = RequestReceipt {
            result_id: result_id.clone(),
            version,
            op,
        };
        env.storage().persistent().set(&key, &receipt);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn load_policy(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
    ) -> Result<AttributionPolicy, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Policy(tenant_id.clone(), project_id.clone()))
            .ok_or(ContractError::NotFound)
    }

    fn write_policy(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        policy: &AttributionPolicy,
    ) {
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone());
        env.storage().persistent().set(&key, policy);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_record(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        record_id: &BytesN<32>,
    ) -> Result<AttributionRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::AttributionRecord(
                tenant_id.clone(),
                project_id.clone(),
                record_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_record(env: &Env, record: &AttributionRecord) {
        let key = DataKey::AttributionRecord(
            record.tenant_id.clone(),
            record.project_id.clone(),
            record.record_id.clone(),
        );
        env.storage().persistent().set(&key, record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn push_index(
        env: &Env,
        key: &DataKey,
        id: &BytesN<32>,
        cap: u32,
    ) -> Result<(), ContractError> {
        let mut index = Self::load_index(env, key);
        if index.len() >= cap {
            return Err(ContractError::IndexFull);
        }
        for i in 0..index.len() {
            if index.get(i).unwrap() == id.clone() {
                return Err(ContractError::AlreadyExists);
            }
        }
        index.push_back(id.clone());
        env.storage().persistent().set(key, &index);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn load_index(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn valid_role_code(role_code: Taxonomy) -> bool {
        matches!(
            role_code,
            Taxonomy::Conceptualization
                | Taxonomy::Methodology
                | Taxonomy::Software
                | Taxonomy::DataCuration
                | Taxonomy::Validation
                | Taxonomy::Writing
                | Taxonomy::Supervision
                | Taxonomy::FundingAcquisition
        )
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
