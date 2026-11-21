#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const MAX_RECIPIENTS: u32 = 16;
const MAX_MODELS: u32 = 64;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const BPS_DENOMINATOR: u32 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ModelExists = 4,
    ModelNotFound = 5,
    VersionMismatch = 6,
    InvalidAmount = 7,
    InvalidFee = 8,
    TooManyRecipients = 9,
    InvalidShare = 10,
    LimitReached = 11,
    InvalidModel = 12,
    ArithmeticError = 13,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Configurer = 1,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FundingModelKind {
    Unfunded = 1,
    Grant = 2,
    Milestone = 3,
    Reimbursement = 4,
    Stipend = 5,
    Bounty = 6,
    SponsorFunded = 7,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientShare {
    pub recipient: Address,
    pub share_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FundingModel {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub model_kind: FundingModelKind,
    pub asset: Address,
    pub payer: Option<Address>,
    pub recipients: Vec<RecipientShare>,
    pub fee_recipient: Option<Address>,
    pub fee_bps: u32,
    pub total_amount: i128,
    pub expected_version: u32,
    pub configured_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Count(BytesN<32>, BytesN<32>),
    Model(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneFundingModelsContract;

#[contractimpl]
impl CapstoneFundingModelsContract {
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
        Self::extend(&env, &key);
        env.events().publish(
            (symbol_short!("CFM_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn configure_model(
        env: Env,
        caller: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        model_kind: FundingModelKind,
        asset: Address,
        payer: Option<Address>,
        recipients: Vec<RecipientShare>,
        fee_recipient: Option<Address>,
        fee_bps: u32,
        total_amount: i128,
        expected_version: u32,
    ) -> Result<FundingModel, ContractError> {
        caller.require_auth();
        Self::require_role(&env, &caller, &tenant_id, &project_id, Role::Configurer)?;
        Self::validate_inputs(
            model_kind,
            &payer,
            &recipients,
            &fee_recipient,
            fee_bps,
            total_amount,
        )?;
        let key = DataKey::Model(tenant_id.clone(), project_id.clone(), intent_id.clone());
        if let Some(existing) = env.storage().persistent().get::<_, FundingModel>(&key) {
            if existing.model_kind == model_kind
                && existing.asset == asset
                && existing.payer == payer
                && existing.recipients == recipients
                && existing.fee_recipient == fee_recipient
                && existing.fee_bps == fee_bps
                && existing.total_amount == total_amount
                && existing.expected_version == expected_version
            {
                Self::extend(&env, &key);
                return Ok(existing);
            }
            return Err(ContractError::ModelExists);
        }
        if expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let count_key = DataKey::Count(tenant_id.clone(), project_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_MODELS {
            return Err(ContractError::LimitReached);
        }
        let model = FundingModel {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            intent_id: intent_id.clone(),
            model_kind,
            asset,
            payer,
            recipients,
            fee_recipient,
            fee_bps,
            total_amount,
            expected_version: 1,
            configured_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &model);
        env.storage()
            .persistent()
            .set(&count_key, &(count.checked_add(1).ok_or(ContractError::LimitReached)?));
        Self::extend(&env, &key);
        Self::extend(&env, &count_key);
        env.events().publish(
            (symbol_short!("CFM_CFG"),),
            (tenant_id, project_id, intent_id, model_kind, total_amount, fee_bps),
        );
        Ok(model)
    }

    pub fn get_model(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
    ) -> Option<FundingModel> {
        env.storage()
            .persistent()
            .get(&DataKey::Model(tenant_id, project_id, intent_id))
    }

    fn validate_inputs(
        model_kind: FundingModelKind,
        payer: &Option<Address>,
        recipients: &Vec<RecipientShare>,
        fee_recipient: &Option<Address>,
        fee_bps: u32,
        total_amount: i128,
    ) -> Result<(), ContractError> {
        if fee_bps > BPS_DENOMINATOR {
            return Err(ContractError::InvalidFee);
        }
        match model_kind {
            FundingModelKind::Unfunded => {
                if payer.is_some()
                    || fee_recipient.is_some()
                    || fee_bps != 0
                    || total_amount != 0
                    || !recipients.is_empty()
                {
                    return Err(ContractError::InvalidModel);
                }
            }
            _ => {
                if total_amount <= 0 {
                    return Err(ContractError::InvalidAmount);
                }
                if recipients.is_empty() || recipients.len() > MAX_RECIPIENTS {
                    return Err(ContractError::TooManyRecipients);
                }
                let mut sum: u32 = 0;
                for r in recipients.iter() {
                    let r: RecipientShare = r;
                    if r.share_bps == 0 || r.share_bps > BPS_DENOMINATOR {
                        return Err(ContractError::InvalidShare);
                    }
                    sum = sum
                        .checked_add(r.share_bps)
                        .ok_or(ContractError::ArithmeticError)?;
                }
                if sum != BPS_DENOMINATOR {
                    return Err(ContractError::InvalidShare);
                }
                if fee_bps > 0 && fee_recipient.is_none() {
                    return Err(ContractError::InvalidFee);
                }
                if fee_bps == 0 && fee_recipient.is_some() {
                    return Err(ContractError::InvalidFee);
                }
            }
        }
        Ok(())
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
        Self::extend(env, &key);
        Ok(())
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
