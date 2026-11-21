#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_CONFIRMATIONS_PER_PROJECT: u32 = 16;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    WrongTenant = 4,
    NotFound = 5,
    InvalidAmount = 6,
    InvalidExpiry = 7,
    NotFinal = 8,
    Underpayment = 9,
    Overpayment = 10,
    AssetMismatch = 11,
    DestinationMismatch = 12,
    AgreementMismatch = 13,
    VersionConflict = 14,
    RequestConflict = 15,
    InvalidState = 16,
    CapacityExceeded = 17,
    Overflow = 18,
    Expired = 19,
    InvalidOutcome = 20,
    AlreadyConfirmed = 21,
    AlreadyRejected = 22,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Attestor = 1,
    SettlementAuditor = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Finality {
    Pending = 1,
    Final = 2,
    Failed = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmountOutcome {
    Exact = 1,
    Under = 2,
    Over = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reconciliation {
    Match = 1,
    Mismatch = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfirmationState {
    Confirmed = 1,
    Rejected = 2,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    NotFinal = 1,
    Underpayment = 2,
    Overpayment = 3,
    AssetMismatch = 4,
    DestinationMismatch = 5,
    AgreementMismatch = 6,
    Expired = 7,
    InvalidOutcome = 8,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowConfirmation {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub intent_hash: BytesN<32>,
    pub request_id: BytesN<32>,
    pub finality_hash: BytesN<32>,
    pub expected_amount: i128,
    pub observed_amount: i128,
    pub asset: BytesN<32>,
    pub destination_hash: BytesN<32>,
    pub agreement_hash: BytesN<32>,
    pub finality: Finality,
    pub amount_outcome: AmountOutcome,
    pub asset_outcome: Reconciliation,
    pub destination_outcome: Reconciliation,
    pub agreement_outcome: Reconciliation,
    pub state: ConfirmationState,
    pub funds_available: bool,
    pub version: u32,
    pub confirmed_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowRejection {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub intent_hash: BytesN<32>,
    pub reason: ReasonCode,
    pub state: ConfirmationState,
    pub version: u32,
    pub rejected_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestMarker {
    pub base_version: u32,
    pub target_version: u32,
    pub request_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmFundingRequest {
    pub intent_id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub intent_hash: BytesN<32>,
    pub finality_hash: BytesN<32>,
    pub expected_amount: i128,
    pub observed_amount: i128,
    pub asset: BytesN<32>,
    pub destination_hash: BytesN<32>,
    pub agreement_hash: BytesN<32>,
    pub finality: Finality,
    pub amount_outcome: AmountOutcome,
    pub asset_outcome: Reconciliation,
    pub destination_outcome: Reconciliation,
    pub agreement_outcome: Reconciliation,
    pub expires_at: u64,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    Confirmation(BytesN<32>, BytesN<32>, BytesN<32>),
    ConfirmationRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    Rejection(BytesN<32>, BytesN<32>, BytesN<32>),
    RejectionRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    Index(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneEscrowConfirmationsContract;

#[contractimpl]
impl CapstoneEscrowConfirmationsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .extend_ttl(RECORD_MIN_TTL, RECORD_MAX_TTL);
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
        let key = DataKey::Role(tenant_id.clone(), account.clone(), role);
        env.storage().persistent().set(&key, &true);
        Self::extend(&env, &key);
        env.events()
            .publish((symbol_short!("CBPROLG"),), (tenant_id, account, role));
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
        env.storage().persistent().set(&key, &false);
        Self::extend(&env, &key);
        env.events()
            .publish((symbol_short!("CBPRVRK"),), (tenant_id, account, role));
        Ok(())
    }

    pub fn has_role(env: Env, tenant_id: BytesN<32>, account: Address, role: Role) -> bool {
        let key = DataKey::Role(tenant_id, account, role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend(&env, &key);
        }
        active
    }

    pub fn confirm_funding(
        env: Env,
        attestor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request: ConfirmFundingRequest,
    ) -> Result<EscrowConfirmation, ContractError> {
        Self::require_role(&env, &attestor, &tenant_id, Role::Attestor)?;
        if request.expected_amount <= 0 || request.observed_amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if request.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::Expired);
        }
        if request.expected_version != 0 {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::ConfirmationRequest(
            tenant_id.clone(),
            project_id.clone(),
            request.request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != request.expected_version
                || marker.request_hash != request.intent_hash
            {
                return Err(ContractError::RequestConflict);
            }
            let existing_key = DataKey::Confirmation(
                tenant_id.clone(),
                project_id.clone(),
                request.intent_id.clone(),
            );
            let existing: EscrowConfirmation = env
                .storage()
                .persistent()
                .get(&existing_key)
                .ok_or(ContractError::NotFound)?;
            Self::extend(&env, &existing_key);
            return Ok(existing);
        }
        let key = DataKey::Confirmation(
            tenant_id.clone(),
            project_id.clone(),
            request.intent_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyConfirmed);
        }
        if env.storage().persistent().has(&DataKey::Rejection(
            tenant_id.clone(),
            project_id.clone(),
            request.intent_id.clone(),
        )) {
            return Err(ContractError::InvalidState);
        }
        if request.finality != Finality::Final {
            return Err(ContractError::NotFinal);
        }
        if request.observed_amount < request.expected_amount {
            return Err(ContractError::Underpayment);
        }
        if request.observed_amount > request.expected_amount {
            return Err(ContractError::Overpayment);
        }
        if request.amount_outcome != AmountOutcome::Exact {
            return Err(ContractError::InvalidOutcome);
        }
        if request.asset_outcome != Reconciliation::Match {
            return Err(ContractError::AssetMismatch);
        }
        if request.destination_outcome != Reconciliation::Match {
            return Err(ContractError::DestinationMismatch);
        }
        if request.agreement_outcome != Reconciliation::Match {
            return Err(ContractError::AgreementMismatch);
        }
        Self::push_index(&env, &tenant_id, &project_id, &request.intent_id)?;
        let version = 0u32.checked_add(1).ok_or(ContractError::Overflow)?;
        let confirmation = EscrowConfirmation {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            intent_id: request.intent_id.clone(),
            intent_hash: request.intent_hash.clone(),
            request_id: request.request_id.clone(),
            finality_hash: request.finality_hash.clone(),
            expected_amount: request.expected_amount,
            observed_amount: request.observed_amount,
            asset: request.asset.clone(),
            destination_hash: request.destination_hash.clone(),
            agreement_hash: request.agreement_hash.clone(),
            finality: request.finality,
            amount_outcome: request.amount_outcome,
            asset_outcome: request.asset_outcome,
            destination_outcome: request.destination_outcome,
            agreement_outcome: request.agreement_outcome,
            state: ConfirmationState::Confirmed,
            funds_available: true,
            version,
            confirmed_at: env.ledger().timestamp(),
            expires_at: request.expires_at,
        };
        env.storage().persistent().set(&key, &confirmation);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: request.expected_version,
            target_version: version,
            request_hash: request.intent_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBECCD"),),
            (
                tenant_id,
                project_id,
                request.intent_id,
                request.observed_amount,
                request.asset,
            ),
        );
        Ok(confirmation)
    }

    pub fn reject_funding(
        env: Env,
        auditor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        intent_hash: BytesN<32>,
        reason: ReasonCode,
        expected_version: u32,
    ) -> Result<EscrowRejection, ContractError> {
        Self::require_role(&env, &auditor, &tenant_id, Role::SettlementAuditor)?;
        if expected_version != 0 {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::RejectionRequest(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != expected_version || marker.request_hash != intent_hash {
                return Err(ContractError::RequestConflict);
            }
            let existing_key = DataKey::Rejection(
                tenant_id.clone(),
                project_id.clone(),
                intent_id.clone(),
            );
            let existing: EscrowRejection = env
                .storage()
                .persistent()
                .get(&existing_key)
                .ok_or(ContractError::NotFound)?;
            Self::extend(&env, &existing_key);
            return Ok(existing);
        }
        let key = DataKey::Rejection(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyRejected);
        }
        if env.storage().persistent().has(&DataKey::Confirmation(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        )) {
            return Err(ContractError::InvalidState);
        }
        Self::push_index(&env, &tenant_id, &project_id, &intent_id)?;
        let version = 0u32.checked_add(1).ok_or(ContractError::Overflow)?;
        let rejection = EscrowRejection {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            intent_id: intent_id.clone(),
            request_id: request_id.clone(),
            intent_hash: intent_hash.clone(),
            reason,
            state: ConfirmationState::Rejected,
            version,
            rejected_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&key, &rejection);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: version,
            request_hash: intent_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBEREJ"),),
            (tenant_id, project_id, intent_id, reason),
        );
        Ok(rejection)
    }

    pub fn get_confirmation(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
    ) -> Result<EscrowConfirmation, ContractError> {
        let key = DataKey::Confirmation(tenant_id, project_id, intent_id);
        let confirmation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(confirmation)
    }

    pub fn get_rejection(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
    ) -> Result<EscrowRejection, ContractError> {
        let key = DataKey::Rejection(tenant_id, project_id, intent_id);
        let rejection = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(rejection)
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
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        let key = DataKey::Role(tenant_id.clone(), caller.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if active {
            Self::extend(env, &key);
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn push_index(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if ids.len() >= MAX_CONFIRMATIONS_PER_PROJECT {
            return Err(ContractError::CapacityExceeded);
        }
        ids.push_back(intent_id.clone());
        env.storage().persistent().set(&key, &ids);
        Self::extend(env, &key);
        Ok(())
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
