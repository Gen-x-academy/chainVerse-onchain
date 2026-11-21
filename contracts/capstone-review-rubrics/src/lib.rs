#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_CRITERIA: u32 = 16;
const WEIGHT_TOTAL: u32 = 100;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const REQUEST_MIN_TTL: u32 = 1_555_200;
const REQUEST_MAX_TTL: u32 = 3_110_400;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidTransition = 6,
    StaleVersion = 7,
    VersionOverflow = 8,
    RequestConflict = 9,
    CollectionFull = 10,
    InvalidRange = 11,
    InvalidWeight = 12,
    ArithmeticOverflow = 13,
    InvalidHash = 14,
    MissingPrerequisite = 15,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Rubric(BytesN<32>, BytesN<32>, BytesN<32>),
    Criteria(BytesN<32>, BytesN<32>, BytesN<32>),
    Criterion(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Admin = 1,
    Editor = 2,
    Reviewer = 3,
    Auditor = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Aggregation {
    WeightedMean = 1,
    Median = 2,
    Consensus = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldVisibility {
    Public = 1,
    ReviewerOnly = 2,
    Hidden = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RubricStatus {
    Draft = 1,
    Published = 2,
    Archived = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rubric {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub rubric_id: BytesN<32>,
    pub version: u32,
    pub status: RubricStatus,
    pub aggregation: Aggregation,
    pub visibility: FieldVisibility,
    pub weight_total: u32,
    pub criterion_count: u32,
    pub content_hash: BytesN<32>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Criterion {
    pub criterion_id: BytesN<32>,
    pub weight: u32,
    pub min_score: u32,
    pub max_score: u32,
    pub requires_comment: bool,
    pub visibility: FieldVisibility,
    pub guidance_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub payload_hash: BytesN<32>,
    pub result_id: BytesN<32>,
    pub result_version: u32,
    pub recorded_at: u64,
}

#[contract]
pub struct CapstoneReviewRubricsContract;

#[contractimpl]
impl CapstoneReviewRubricsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events()
            .publish((symbol_short!("RBGRANT"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        subject: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(subject, tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().remove(&key);
        env.events()
            .publish((symbol_short!("RBRVOK"),), (tenant_id, project_id, role));
        Ok(())
    }

    pub fn create_rubric(
        env: Env,
        editor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        aggregation: Aggregation,
        visibility: FieldVisibility,
        content_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &editor, &tenant_id, &project_id, Role::Editor)?;
        if content_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Rubric(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let now = env.ledger().timestamp();
        let rubric = Rubric {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            rubric_id: rubric_id.clone(),
            version: 1,
            status: RubricStatus::Draft,
            aggregation,
            visibility,
            weight_total: 0,
            criterion_count: 0,
            content_hash,
            created_at: now,
            updated_at: now,
        };
        Self::store_rubric(&env, &key, &rubric);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
            1,
            now,
        );
        env.events().publish(
            (symbol_short!("RBNEW"),),
            (tenant_id, project_id, rubric_id, 1u32),
        );
        Ok(1)
    }

    pub fn add_criterion(
        env: Env,
        editor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
        criterion: Criterion,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &editor, &tenant_id, &project_id, Role::Editor)?;
        if criterion.guidance_hash == Self::zero_id(&env) {
            return Err(ContractError::InvalidHash);
        }
        if criterion.max_score <= criterion.min_score {
            return Err(ContractError::InvalidRange);
        }
        if criterion.weight == 0 || criterion.weight > WEIGHT_TOTAL {
            return Err(ContractError::InvalidWeight);
        }
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Rubric(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        let mut rubric: Rubric = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if rubric.status != RubricStatus::Draft {
            return Err(ContractError::InvalidTransition);
        }
        if rubric.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let ids_key = DataKey::Criteria(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&ids_key)
            .unwrap_or_else(|| Vec::new(&env));
        if ids.len() >= MAX_CRITERIA {
            return Err(ContractError::CollectionFull);
        }
        if Self::contains_id(&ids, &criterion.criterion_id) {
            return Err(ContractError::AlreadyExists);
        }
        ids.push_back(criterion.criterion_id.clone());
        let weight_total = rubric
            .weight_total
            .checked_add(criterion.weight)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if weight_total > WEIGHT_TOTAL {
            return Err(ContractError::InvalidWeight);
        }
        if rubric.criterion_count.checked_add(1) != Some(ids.len()) {
            return Err(ContractError::ArithmeticOverflow);
        }
        let version = rubric
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        rubric.version = version;
        rubric.criterion_count = ids.len();
        rubric.weight_total = weight_total;
        rubric.updated_at = now;
        Self::store_rubric(&env, &key, &rubric);
        env.storage().persistent().set(&ids_key, &ids);
        env.storage()
            .persistent()
            .extend_ttl(&ids_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let criterion_key = DataKey::Criterion(
            tenant_id.clone(),
            project_id.clone(),
            rubric_id.clone(),
            criterion.criterion_id.clone(),
        );
        env.storage().persistent().set(&criterion_key, &criterion);
        env.storage()
            .persistent()
            .extend_ttl(&criterion_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("RBCRIT"),),
            (tenant_id, project_id, rubric_id, version),
        );
        Ok(version)
    }

    pub fn publish_rubric(
        env: Env,
        editor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &editor, &tenant_id, &project_id, Role::Editor)?;
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Rubric(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        let mut rubric: Rubric = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if rubric.status != RubricStatus::Draft {
            return Err(ContractError::InvalidTransition);
        }
        if rubric.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if rubric.criterion_count == 0 {
            return Err(ContractError::MissingPrerequisite);
        }
        if rubric.weight_total != WEIGHT_TOTAL {
            return Err(ContractError::InvalidWeight);
        }
        let version = rubric
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        rubric.version = version;
        rubric.status = RubricStatus::Published;
        rubric.updated_at = now;
        Self::store_rubric(&env, &key, &rubric);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("RBPUB"),),
            (tenant_id, project_id, rubric_id, version),
        );
        Ok(version)
    }

    pub fn archive_rubric(
        env: Env,
        editor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
        request_id: BytesN<32>,
        payload_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &editor, &tenant_id, &project_id, Role::Editor)?;
        if let Some(version) = Self::replay_version(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
        )? {
            return Ok(version);
        }
        let key = DataKey::Rubric(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        let mut rubric: Rubric = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if rubric.status != RubricStatus::Published {
            return Err(ContractError::InvalidTransition);
        }
        if rubric.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let version = rubric
            .version
            .checked_add(1)
            .ok_or(ContractError::VersionOverflow)?;
        let now = env.ledger().timestamp();
        rubric.version = version;
        rubric.status = RubricStatus::Archived;
        rubric.updated_at = now;
        Self::store_rubric(&env, &key, &rubric);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &payload_hash,
            &rubric_id,
            version,
            now,
        );
        env.events().publish(
            (symbol_short!("RBARCH"),),
            (tenant_id, project_id, rubric_id, version),
        );
        Ok(version)
    }

    pub fn get_rubric(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
    ) -> Option<Rubric> {
        env.storage()
            .persistent()
            .get(&DataKey::Rubric(tenant_id, project_id, rubric_id))
    }

    pub fn get_criteria(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        rubric_id: BytesN<32>,
    ) -> Vec<Criterion> {
        let ids = Self::load_ids(&env, &tenant_id, &project_id, &rubric_id);
        let mut out = Vec::new(&env);
        let mut i = 0;
        while i < ids.len() {
            if let Some(id) = ids.get(i) {
                let key = DataKey::Criterion(
                    tenant_id.clone(),
                    project_id.clone(),
                    rubric_id.clone(),
                    id,
                );
                if let Some(criterion) = env.storage().persistent().get::<_, Criterion>(&key) {
                    out.push_back(criterion);
                }
            }
            i += 1;
        }
        out
    }

    pub fn has_role(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        role: Role,
    ) -> bool {
        Self::has_stored_role(&env, &subject, &tenant_id, &project_id, role)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let configured: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &configured != admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if let Some(configured) = env.storage().instance().get::<_, Address>(&DataKey::Admin) {
            if &configured == caller {
                return Ok(());
            }
        }
        let zero = Self::zero_id(env);
        if Self::has_stored_role(env, caller, tenant_id, &zero, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, Role::Admin)
            || Self::has_stored_role(env, caller, tenant_id, project_id, role)
        {
            return Ok(());
        }
        Err(ContractError::Unauthorized)
    }

    fn has_stored_role(
        env: &Env,
        subject: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> bool {
        let key = DataKey::Role(subject.clone(), tenant_id.clone(), project_id.clone(), role);
        env.storage()
            .persistent()
            .get::<_, bool>(&key)
            .unwrap_or(false)
    }

    fn zero_id(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }

    fn replay_version(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
    ) -> Result<Option<u32>, ContractError> {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        if let Some(receipt) = env.storage().temporary().get::<_, RequestReceipt>(&key) {
            if &receipt.payload_hash != payload_hash || &receipt.result_id != result_id {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .temporary()
                .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
            return Ok(Some(receipt.result_version));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        payload_hash: &BytesN<32>,
        result_id: &BytesN<32>,
        version: u32,
        now: u64,
    ) {
        let key = DataKey::Request(tenant_id.clone(), project_id.clone(), request_id.clone());
        let receipt = RequestReceipt {
            payload_hash: payload_hash.clone(),
            result_id: result_id.clone(),
            result_version: version,
            recorded_at: now,
        };
        env.storage().temporary().set(&key, &receipt);
        env.storage()
            .temporary()
            .extend_ttl(&key, REQUEST_MIN_TTL, REQUEST_MAX_TTL);
    }

    fn store_rubric(env: &Env, key: &DataKey, rubric: &Rubric) {
        env.storage().persistent().set(key, rubric);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_ids(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        rubric_id: &BytesN<32>,
    ) -> Vec<BytesN<32>> {
        let key = DataKey::Criteria(tenant_id.clone(), project_id.clone(), rubric_id.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn contains_id(ids: &Vec<BytesN<32>>, id: &BytesN<32>) -> bool {
        let mut i = 0;
        while i < ids.len() {
            if ids.get(i).as_ref() == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }
}
