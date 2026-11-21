#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_MILESTONES: u32 = 16;
const MAX_RECEIPTS: u32 = 16;
const MAX_ROLES: u32 = 16;
const MAX_REQUESTS: u32 = 128;
const BPS_DENOMINATOR: i128 = 10_000;
const MAX_FEE_BPS: u32 = 10_000;
const MAX_CHALLENGE_WINDOW: u64 = 2_592_000;

const OP_CONFIGURE: u32 = 1;
const OP_ACCEPT: u32 = 2;
const OP_CHALLENGE: u32 = 3;
const OP_RESOLVE: u32 = 4;
const OP_RELEASE: u32 = 5;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidAmount = 6,
    InvalidFee = 7,
    InvalidWindow = 8,
    InvalidState = 9,
    StaleVersion = 10,
    RequestConflict = 11,
    ChallengeWindowOpen = 12,
    ArithmeticOverflow = 13,
    IndexFull = 14,
    RoleLimitReached = 15,
    RoleAlreadyGranted = 16,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Reviewer = 2,
    Finance = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MilestoneState {
    Configured = 1,
    Accepted = 2,
    Challenged = 3,
    Released = 4,
    Cancelled = 5,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RoleIndex(BytesN<32>, BytesN<32>),
    RequestReceipt(BytesN<32>, BytesN<32>, BytesN<32>),
    RequestIndex(BytesN<32>, BytesN<32>),
    Milestone(BytesN<32>, BytesN<32>, BytesN<32>),
    MilestoneIndex(BytesN<32>, BytesN<32>),
    Receipt(BytesN<32>),
    ReceiptIndex(BytesN<32>, BytesN<32>),
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
pub struct Milestone {
    pub milestone_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub amount: i128,
    pub fee_bps: u32,
    pub acceptance_hash: BytesN<32>,
    pub accepted_at: u64,
    pub challenge_end: u64,
    pub state: MilestoneState,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentReceipt {
    pub receipt_id: BytesN<32>,
    pub milestone_id: BytesN<32>,
    pub gross_amount: i128,
    pub fee_amount: i128,
    pub net_amount: i128,
    pub fee_bps: u32,
    pub released_at: u64,
    pub version: u32,
}

#[contract]
pub struct CapstoneMilestonePayments;

#[contractimpl]
impl CapstoneMilestonePayments {
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

    pub fn configure_milestone(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        amount: i128,
        fee_bps: u32,
        challenge_window_secs: u64,
    ) -> Result<Milestone, ContractError> {
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
            OP_CONFIGURE,
            expected_version,
        )? {
            return Self::load_milestone(&env, &tenant_id, &project_id, &result_id);
        }
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if fee_bps > MAX_FEE_BPS {
            return Err(ContractError::InvalidFee);
        }
        if challenge_window_secs == 0 || challenge_window_secs > MAX_CHALLENGE_WINDOW {
            return Err(ContractError::InvalidWindow);
        }
        let key = DataKey::Milestone(
            tenant_id.clone(),
            project_id.clone(),
            milestone_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::MilestoneIndex(tenant_id.clone(), project_id.clone()),
            &milestone_id,
            MAX_MILESTONES,
        )?;
        let version = expected_version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let milestone = Milestone {
            milestone_id: milestone_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            amount,
            fee_bps,
            acceptance_hash: Self::zero_hash(&env),
            accepted_at: 0,
            challenge_end: 0,
            state: MilestoneState::Configured,
            version,
        };
        Self::write_milestone(&env, &milestone);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &milestone_id,
            OP_CONFIGURE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("MSPCFG"),),
            (milestone_id, amount, fee_bps),
        );
        Ok(milestone)
    }

    pub fn record_acceptance(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        acceptance_hash: BytesN<32>,
    ) -> Result<Milestone, ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_ACCEPT,
            expected_version,
        )? {
            return Self::load_milestone(&env, &tenant_id, &project_id, &result_id);
        }
        let mut milestone =
            Self::load_milestone(&env, &tenant_id, &project_id, &milestone_id)?;
        if milestone.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if milestone.state != MilestoneState::Configured {
            return Err(ContractError::InvalidState);
        }
        let now = env.ledger().timestamp();
        let challenge_end = now
            .checked_add(milestone.challenge_window_secs)
            .ok_or(ContractError::ArithmeticOverflow)?;
        milestone.accepted_at = now;
        milestone.challenge_end = challenge_end;
        milestone.acceptance_hash = acceptance_hash.clone();
        milestone.state = MilestoneState::Accepted;
        milestone.version = milestone
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_milestone(&env, &milestone);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &milestone_id,
            OP_ACCEPT,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("MSPACCP"),),
            (milestone_id, acceptance_hash, milestone.state as u32),
        );
        Ok(milestone)
    }

    pub fn flag_challenge(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        challenge_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &reviewer, &tenant_id, &project_id, Role::Reviewer)?;
        if Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_CHALLENGE,
            expected_version,
        )?
        .is_some()
        {
            return Ok(());
        }
        let mut milestone =
            Self::load_milestone(&env, &tenant_id, &project_id, &milestone_id)?;
        if milestone.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if milestone.state != MilestoneState::Accepted
            || env.ledger().timestamp() >= milestone.challenge_end
        {
            return Err(ContractError::InvalidState);
        }
        milestone.state = MilestoneState::Challenged;
        milestone.acceptance_hash = challenge_hash.clone();
        milestone.version = milestone
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_milestone(&env, &milestone);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &milestone_id,
            OP_CHALLENGE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("MSPCHAL"),),
            (milestone_id, challenge_hash, milestone.state as u32),
        );
        Ok(())
    }

    pub fn resolve_challenge(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        release: bool,
        resolution_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
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
            OP_RESOLVE,
            expected_version,
        )?
        .is_some()
        {
            return Ok(());
        }
        let mut milestone =
            Self::load_milestone(&env, &tenant_id, &project_id, &milestone_id)?;
        if milestone.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if milestone.state != MilestoneState::Challenged {
            return Err(ContractError::InvalidState);
        }
        if release {
            milestone.state = MilestoneState::Accepted;
            milestone.challenge_end = env.ledger().timestamp();
        } else {
            milestone.state = MilestoneState::Cancelled;
        }
        milestone.acceptance_hash = resolution_hash.clone();
        milestone.version = milestone
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_milestone(&env, &milestone);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &milestone_id,
            OP_RESOLVE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("MSPCHRES"),),
            (milestone_id, resolution_hash, milestone.state as u32),
        );
        Ok(())
    }

    pub fn release_payment(
        env: Env,
        finance: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<PaymentReceipt, ContractError> {
        Self::require_role(&env, &finance, &tenant_id, &project_id, Role::Finance)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_RELEASE,
            expected_version,
        )? {
            return Self::load_receipt(&env, &result_id);
        }
        let mut milestone =
            Self::load_milestone(&env, &tenant_id, &project_id, &milestone_id)?;
        if milestone.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if milestone.state != MilestoneState::Accepted {
            return Err(ContractError::InvalidState);
        }
        let now = env.ledger().timestamp();
        if now < milestone.challenge_end {
            return Err(ContractError::ChallengeWindowOpen);
        }
        let fee_amount = milestone
            .amount
            .checked_mul(milestone.fee_bps as i128)
            .ok_or(ContractError::ArithmeticOverflow)?
            / BPS_DENOMINATOR;
        let net_amount = milestone
            .amount
            .checked_sub(fee_amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let receipt_id = request_id.clone();
        let receipt_key = DataKey::Receipt(receipt_id.clone());
        if env.storage().persistent().has(&receipt_key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::ReceiptIndex(tenant_id.clone(), project_id.clone()),
            &receipt_id,
            MAX_RECEIPTS,
        )?;
        let version = milestone
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let receipt = PaymentReceipt {
            receipt_id: receipt_id.clone(),
            milestone_id: milestone_id.clone(),
            gross_amount: milestone.amount,
            fee_amount,
            net_amount,
            fee_bps: milestone.fee_bps,
            released_at: now,
            version,
        };
        env.storage().persistent().set(&receipt_key, &receipt);
        env.storage()
            .persistent()
            .extend_ttl(&receipt_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        milestone.state = MilestoneState::Released;
        milestone.version = version;
        Self::write_milestone(&env, &milestone);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &receipt_id,
            OP_RELEASE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("MSPPAY"),),
            (milestone_id, receipt_id, milestone.amount, fee_amount, net_amount),
        );
        Ok(receipt)
    }

    pub fn get_milestone(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        milestone_id: BytesN<32>,
    ) -> Result<Milestone, ContractError> {
        Self::load_milestone(&env, &tenant_id, &project_id, &milestone_id)
    }

    pub fn get_receipt(env: Env, receipt_id: BytesN<32>) -> Result<PaymentReceipt, ContractError> {
        Self::load_receipt(&env, &receipt_id)
    }

    pub fn milestone_count(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::index_len(
            &env,
            &DataKey::MilestoneIndex(tenant_id, project_id),
        )
    }

    pub fn receipt_count(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::index_len(&env, &DataKey::ReceiptIndex(tenant_id, project_id))
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

    fn load_milestone(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        milestone_id: &BytesN<32>,
    ) -> Result<Milestone, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Milestone(
                tenant_id.clone(),
                project_id.clone(),
                milestone_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_milestone(env: &Env, milestone: &Milestone) {
        let key = DataKey::Milestone(
            milestone.tenant_id.clone(),
            milestone.project_id.clone(),
            milestone.milestone_id.clone(),
        );
        env.storage().persistent().set(&key, milestone);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_receipt(env: &Env, receipt_id: &BytesN<32>) -> Result<PaymentReceipt, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Receipt(receipt_id.clone()))
            .ok_or(ContractError::NotFound)
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

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
