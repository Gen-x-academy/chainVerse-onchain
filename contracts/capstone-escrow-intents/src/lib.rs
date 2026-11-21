#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_INTENTS_PER_PROJECT: u32 = 16;

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
    VersionConflict = 8,
    RequestConflict = 9,
    InvalidState = 10,
    CapacityExceeded = 11,
    Overflow = 12,
    Expired = 13,
    HashMismatch = 14,
    AlreadyExists = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    FinanceOfficer = 1,
    Payer = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentState {
    Pending = 1,
    Cancelled = 2,
    Expired = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowIntent {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub intent_id: BytesN<32>,
    pub intent_hash: BytesN<32>,
    pub payer: Address,
    pub charter_hash: BytesN<32>,
    pub conditions_hash: BytesN<32>,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub state: IntentState,
    pub version: u32,
    pub created_at: u64,
    pub expires_at: u64,
    pub request_id: BytesN<32>,
    pub request_hash: BytesN<32>,
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
pub struct CreateIntentRequest {
    pub intent_id: BytesN<32>,
    pub intent_hash: BytesN<32>,
    pub charter_hash: BytesN<32>,
    pub conditions_hash: BytesN<32>,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub expires_at: u64,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(BytesN<32>, Address, Role),
    Intent(BytesN<32>, BytesN<32>, BytesN<32>),
    IntentRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    CancelRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    ExpiryRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    Index(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneEscrowIntentsContract;

#[contractimpl]
impl CapstoneEscrowIntentsContract {
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

    pub fn create_intent(
        env: Env,
        officer: Address,
        payer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request: CreateIntentRequest,
    ) -> Result<EscrowIntent, ContractError> {
        Self::require_role(&env, &officer, &tenant_id, Role::FinanceOfficer)?;
        Self::require_role(&env, &payer, &tenant_id, Role::Payer)?;
        if request.amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if request.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        let request_key = DataKey::IntentRequest(
            tenant_id.clone(),
            project_id.clone(),
            request.intent_id.clone(),
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
            let existing_key = DataKey::Intent(
                tenant_id.clone(),
                project_id.clone(),
                request.intent_id.clone(),
            );
            let existing: EscrowIntent = env
                .storage()
                .persistent()
                .get(&existing_key)
                .ok_or(ContractError::NotFound)?;
            Self::extend(&env, &existing_key);
            return Ok(existing);
        }
        if request.expected_version != 0 {
            return Err(ContractError::VersionConflict);
        }
        let key = DataKey::Intent(
            tenant_id.clone(),
            project_id.clone(),
            request.intent_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(&env, &tenant_id, &project_id, &request.intent_id)?;
        let next_version = 0u32.checked_add(1).ok_or(ContractError::Overflow)?;
        let intent = EscrowIntent {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            intent_id: request.intent_id.clone(),
            intent_hash: request.intent_hash.clone(),
            payer: payer.clone(),
            charter_hash: request.charter_hash.clone(),
            conditions_hash: request.conditions_hash.clone(),
            asset: request.asset.clone(),
            amount: request.amount,
            state: IntentState::Pending,
            version: next_version,
            created_at: env.ledger().timestamp(),
            expires_at: request.expires_at,
            request_id: request.intent_id.clone(),
            request_hash: request.intent_hash.clone(),
        };
        env.storage().persistent().set(&key, &intent);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: request.expected_version,
            target_version: next_version,
            request_hash: request.intent_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBEINT"),),
            (
                tenant_id,
                project_id,
                request.intent_id,
                request.amount,
                request.asset,
            ),
        );
        Ok(intent)
    }

    pub fn cancel_intent(
        env: Env,
        officer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        request_hash: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &officer, &tenant_id, Role::FinanceOfficer)?;
        let key = DataKey::Intent(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        );
        let mut intent: EscrowIntent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if intent.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if intent.version != expected_version {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::CancelRequest(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != expected_version || marker.request_hash != request_hash {
                return Err(ContractError::RequestConflict);
            }
            return Ok(());
        }
        if intent.state != IntentState::Pending {
            return Err(ContractError::InvalidState);
        }
        intent.state = IntentState::Cancelled;
        env.storage().persistent().set(&key, &intent);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: expected_version,
            request_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBECAN"),),
            (tenant_id, project_id, intent_id, IntentState::Cancelled),
        );
        Ok(())
    }

    pub fn mark_expired(
        env: Env,
        auditor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &auditor, &tenant_id, Role::Auditor)?;
        let key = DataKey::Intent(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        );
        let mut intent: EscrowIntent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if intent.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if intent.version != expected_version {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::ExpiryRequest(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&request_key) {
            return Ok(());
        }
        if intent.state != IntentState::Pending {
            return Err(ContractError::InvalidState);
        }
        if intent.expires_at > env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        intent.state = IntentState::Expired;
        env.storage().persistent().set(&key, &intent);
        Self::extend(&env, &key);
        env.storage().persistent().set(&request_key, &true);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBEEXP"),),
            (tenant_id, project_id, intent_id, IntentState::Expired),
        );
        Ok(())
    }

    pub fn get_intent(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
    ) -> Result<EscrowIntent, ContractError> {
        let key = DataKey::Intent(tenant_id, project_id, intent_id);
        let intent = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(intent)
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
        if ids.len() >= MAX_INTENTS_PER_PROJECT {
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
