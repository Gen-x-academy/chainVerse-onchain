#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_CLAIMS: u32 = 16;
const MAX_EVIDENCE: u32 = 8;
const MAX_ROLES: u32 = 16;
const MAX_REQUESTS: u32 = 128;

const OP_POLICY: u32 = 1;
const OP_SUBMIT: u32 = 2;
const OP_APPROVE: u32 = 3;
const OP_REJECT: u32 = 4;
const OP_PAYOUT: u32 = 5;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidAmount = 6,
    InvalidCategory = 7,
    InvalidEvidence = 8,
    InvalidPolicy = 9,
    InvalidState = 10,
    StaleVersion = 11,
    RequestConflict = 12,
    SelfReviewNotAllowed = 13,
    ArithmeticOverflow = 14,
    IndexFull = 15,
    RoleLimitReached = 16,
    RoleAlreadyGranted = 17,
    InvalidReason = 18,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Reviewer = 2,
    Finance = 3,
    Claimant = 4,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    Travel = 1,
    Materials = 2,
    Software = 3,
    Training = 4,
    Other = 5,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimState {
    Submitted = 1,
    Approved = 2,
    Rejected = 3,
    Paid = 4,
    Cancelled = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RoleIndex(BytesN<32>, BytesN<32>),
    RequestReceipt(BytesN<32>, BytesN<32>, BytesN<32>),
    RequestIndex(BytesN<32>, BytesN<32>),
    Policy(BytesN<32>, BytesN<32>),
    Claim(BytesN<32>, BytesN<32>, BytesN<32>),
    ClaimIndex(BytesN<32>, BytesN<32>),
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
pub struct ClaimPolicy {
    pub max_amount: i128,
    pub max_evidence_items: u32,
    pub required_approvals: u32,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpenseClaim {
    pub claim_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub claimant: Address,
    pub amount: i128,
    pub category: Category,
    pub evidence_hashes: Vec<BytesN<32>>,
    pub state: ClaimState,
    pub approvals: u32,
    pub payout_hash: BytesN<32>,
    pub version: u32,
}

#[contract]
pub struct CapstoneExpenseClaims;

#[contractimpl]
impl CapstoneExpenseClaims {
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
        max_amount: i128,
        max_evidence_items: u32,
        required_approvals: u32,
    ) -> Result<ClaimPolicy, ContractError> {
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
        if max_amount <= 0 {
            return Err(ContractError::InvalidPolicy);
        }
        if max_evidence_items == 0 || max_evidence_items > MAX_EVIDENCE {
            return Err(ContractError::InvalidPolicy);
        }
        if required_approvals == 0 || required_approvals > 2 {
            return Err(ContractError::InvalidPolicy);
        }
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let policy = ClaimPolicy {
            max_amount,
            max_evidence_items,
            required_approvals,
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
            (symbol_short!("EXPCFG"),),
            (tenant_id, project_id, max_amount, required_approvals),
        );
        Ok(policy)
    }

    pub fn submit_claim(
        env: Env,
        claimant: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        amount: i128,
        category: Category,
        evidence_hashes: Vec<BytesN<32>>,
    ) -> Result<ExpenseClaim, ContractError> {
        Self::require_role(&env, &claimant, &tenant_id, &project_id, Role::Claimant)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_SUBMIT,
            expected_version,
        )? {
            return Self::load_claim(&env, &tenant_id, &project_id, &result_id);
        }
        let policy = Self::load_policy(&env, &tenant_id, &project_id)?;
        if amount <= 0 || amount > policy.max_amount {
            return Err(ContractError::InvalidAmount);
        }
        if !Self::valid_category(category) {
            return Err(ContractError::InvalidCategory);
        }
        if evidence_hashes.is_empty()
            || evidence_hashes.len() > policy.max_evidence_items
            || evidence_hashes.len() > MAX_EVIDENCE
        {
            return Err(ContractError::InvalidEvidence);
        }
        let key = DataKey::Claim(
            tenant_id.clone(),
            project_id.clone(),
            claim_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::ClaimIndex(tenant_id.clone(), project_id.clone()),
            &claim_id,
            MAX_CLAIMS,
        )?;
        let claim = ExpenseClaim {
            claim_id: claim_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            claimant: claimant.clone(),
            amount,
            category,
            evidence_hashes,
            state: ClaimState::Submitted,
            approvals: 0,
            payout_hash: Self::zero_hash(&env),
            version: expected_version
                .checked_add(1)
                .ok_or(ContractError::ArithmeticOverflow)?,
        };
        Self::write_claim(&env, &claim);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &claim_id,
            OP_SUBMIT,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("EXPSUB"),),
            (claim_id, amount, category as u32),
        );
        Ok(claim)
    }

    pub fn approve_claim(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<ExpenseClaim, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_APPROVE,
            expected_version,
        )? {
            return Self::load_claim(&env, &tenant_id, &project_id, &result_id);
        }
        let mut claim = Self::load_claim(&env, &tenant_id, &project_id, &claim_id)?;
        if claim.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if claim.state != ClaimState::Submitted {
            return Err(ContractError::InvalidState);
        }
        if claim.claimant == reviewer.clone() {
            return Err(ContractError::SelfReviewNotAllowed);
        }
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            claim_id.clone(),
            reviewer.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::AlreadyExists);
        }
        let policy = Self::load_policy(&env, &tenant_id, &project_id)?;
        let approvals = claim
            .approvals
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        env.storage().persistent().set(&approval_key, &true);
        env.storage()
            .persistent()
            .extend_ttl(&approval_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        claim.approvals = approvals;
        if approvals >= policy.required_approvals {
            claim.state = ClaimState::Approved;
        }
        claim.version = claim
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_claim(&env, &claim);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &claim_id,
            OP_APPROVE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("EXPAPPR"),),
            (claim_id, approvals, claim.state as u32),
        );
        Ok(claim)
    }

    pub fn reject_claim(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        reason_code: u32,
    ) -> Result<ExpenseClaim, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_REJECT,
            expected_version,
        )? {
            return Self::load_claim(&env, &tenant_id, &project_id, &result_id);
        }
        let mut claim = Self::load_claim(&env, &tenant_id, &project_id, &claim_id)?;
        if claim.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if claim.state != ClaimState::Submitted {
            return Err(ContractError::InvalidState);
        }
        if claim.claimant == reviewer.clone() {
            return Err(ContractError::SelfReviewNotAllowed);
        }
        if reason_code == 0 || reason_code > 4 {
            return Err(ContractError::InvalidReason);
        }
        claim.state = ClaimState::Rejected;
        claim.version = claim
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_claim(&env, &claim);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &claim_id,
            OP_REJECT,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("EXPREJ"),),
            (claim_id, reason_code, claim.state as u32),
        );
        Ok(claim)
    }

    pub fn link_payout(
        env: Env,
        finance: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        payout_hash: BytesN<32>,
    ) -> Result<ExpenseClaim, ContractError> {
        Self::require_role(&env, &finance, &tenant_id, &project_id, Role::Finance)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_PAYOUT,
            expected_version,
        )? {
            return Self::load_claim(&env, &tenant_id, &project_id, &result_id);
        }
        let mut claim = Self::load_claim(&env, &tenant_id, &project_id, &claim_id)?;
        if claim.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if claim.state != ClaimState::Approved {
            return Err(ContractError::InvalidState);
        }
        if payout_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidEvidence);
        }
        claim.state = ClaimState::Paid;
        claim.payout_hash = payout_hash.clone();
        claim.version = claim
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_claim(&env, &claim);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &claim_id,
            OP_PAYOUT,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("EXCPAID"),),
            (claim_id, payout_hash, claim.amount),
        );
        Ok(claim)
    }

    pub fn get_policy(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<ClaimPolicy, ContractError> {
        Self::load_policy(&env, &tenant_id, &project_id)
    }

    pub fn get_claim(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
    ) -> Result<ExpenseClaim, ContractError> {
        Self::load_claim(&env, &tenant_id, &project_id, &claim_id)
    }

    pub fn has_approved(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        claim_id: BytesN<32>,
        reviewer: Address,
    ) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Approval(
                tenant_id,
                project_id,
                claim_id,
                reviewer,
            ))
            .unwrap_or(false)
    }

    pub fn claim_count(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::index_len(&env, &DataKey::ClaimIndex(tenant_id, project_id))
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
    ) -> Result<ClaimPolicy, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Policy(tenant_id.clone(), project_id.clone()))
            .ok_or(ContractError::NotFound)
    }

    fn write_policy(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        policy: &ClaimPolicy,
    ) {
        let key = DataKey::Policy(tenant_id.clone(), project_id.clone());
        env.storage().persistent().set(&key, policy);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_claim(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        claim_id: &BytesN<32>,
    ) -> Result<ExpenseClaim, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Claim(
                tenant_id.clone(),
                project_id.clone(),
                claim_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_claim(env: &Env, claim: &ExpenseClaim) {
        let key = DataKey::Claim(
            claim.tenant_id.clone(),
            claim.project_id.clone(),
            claim.claim_id.clone(),
        );
        env.storage().persistent().set(&key, claim);
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
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env));
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

    fn index_len(env: &Env, key: &DataKey) -> u32 {
        let index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env));
        index.len()
    }

    fn valid_category(category: Category) -> bool {
        matches!(
            category,
            Category::Travel
                | Category::Materials
                | Category::Software
                | Category::Training
                | Category::Other
        )
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
