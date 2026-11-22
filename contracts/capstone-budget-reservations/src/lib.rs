#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_CAP_VERSIONS: u32 = 8;
const MAX_RESERVATIONS_PER_PROJECT: u32 = 8;

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
    InvalidLimit = 8,
    CapExpired = 9,
    AssetMismatch = 10,
    CapExceeded = 11,
    AlreadyReserved = 12,
    VersionConflict = 13,
    RequestConflict = 14,
    InvalidState = 15,
    CapacityExceeded = 16,
    Overflow = 17,
    Underflow = 18,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    ProgramManager = 1,
    Sponsor = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReservationState {
    Active = 1,
    Released = 2,
    Expired = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetCap {
    pub tenant_id: BytesN<32>,
    pub cap_id: BytesN<32>,
    pub asset: BytesN<32>,
    pub limit: i128,
    pub reserved: i128,
    pub version: u32,
    pub expires_at: u64,
    pub request_id: BytesN<32>,
    pub request_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reservation {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub cap_id: BytesN<32>,
    pub plan_hash: BytesN<32>,
    pub asset: BytesN<32>,
    pub amount: i128,
    pub state: ReservationState,
    pub version: u32,
    pub reserved_at: u64,
    pub expires_at: u64,
    pub release_request_id: BytesN<32>,
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
pub struct ReserveRequest {
    pub cap_id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub request_hash: BytesN<32>,
    pub plan_hash: BytesN<32>,
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
    Cap(BytesN<32>, BytesN<32>),
    CapRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    Reservation(BytesN<32>, BytesN<32>, BytesN<32>),
    ProjectReservation(BytesN<32>, BytesN<32>),
    Index(BytesN<32>, BytesN<32>),
    ReleaseRequest(BytesN<32>, BytesN<32>, BytesN<32>),
    ExpiryRequest(BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneBudgetReservationsContract;

#[contractimpl]
impl CapstoneBudgetReservationsContract {
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

    pub fn configure_cap(
        env: Env,
        manager: Address,
        tenant_id: BytesN<32>,
        cap_id: BytesN<32>,
        request_id: BytesN<32>,
        request_hash: BytesN<32>,
        asset: BytesN<32>,
        limit: i128,
        expires_at: u64,
        expected_version: u32,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &manager, &tenant_id, Role::ProgramManager)?;
        if limit <= 0 {
            return Err(ContractError::InvalidLimit);
        }
        if expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        let request_key = DataKey::CapRequest(
            tenant_id.clone(),
            cap_id.clone(),
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
            return Ok(marker.target_version);
        }
        let key = DataKey::Cap(tenant_id.clone(), cap_id.clone());
        let current: Option<BudgetCap> = env.storage().persistent().get(&key);
        let next_version = match current.as_ref() {
            None => {
                if expected_version != 0 {
                    return Err(ContractError::VersionConflict);
                }
                1u32
            }
            Some(cap) => {
                if cap.tenant_id != tenant_id {
                    return Err(ContractError::WrongTenant);
                }
                if cap.version != expected_version {
                    return Err(ContractError::VersionConflict);
                }
                if limit < cap.reserved {
                    return Err(ContractError::InvalidLimit);
                }
                let next = cap.version.checked_add(1).ok_or(ContractError::Overflow)?;
                if next > MAX_CAP_VERSIONS {
                    return Err(ContractError::CapacityExceeded);
                }
                next
            }
        };
        let cap = BudgetCap {
            tenant_id: tenant_id.clone(),
            cap_id: cap_id.clone(),
            asset: asset.clone(),
            limit,
            reserved: current
                .as_ref()
                .map(|value| value.reserved)
                .unwrap_or(0),
            version: next_version,
            expires_at,
            request_id: request_id.clone(),
            request_hash: request_hash.clone(),
        };
        env.storage().persistent().set(&key, &cap);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: next_version,
            request_hash,
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBRGCAP"),),
            (tenant_id, cap_id, next_version, limit),
        );
        Ok(next_version)
    }

    pub fn reserve(
        env: Env,
        sponsor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request: ReserveRequest,
    ) -> Result<Reservation, ContractError> {
        Self::require_role(&env, &sponsor, &tenant_id, Role::Sponsor)?;
        if request.amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        if request.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        if request.expected_version != 0 {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::Reservation(
            tenant_id.clone(),
            project_id.clone(),
            request.request_id.clone(),
        );
        if let Some(reservation) = env
            .storage()
            .persistent()
            .get::<DataKey, Reservation>(&request_key)
        {
            Self::extend(&env, &request_key);
            return Ok(reservation);
        }
        let project_key = DataKey::ProjectReservation(tenant_id.clone(), project_id.clone());
        if let Some(active_request) = env
            .storage()
            .persistent()
            .get::<DataKey, BytesN<32>>(&project_key)
        {
            let existing: Reservation = env
                .storage()
                .persistent()
                .get(&DataKey::Reservation(
                    tenant_id.clone(),
                    project_id.clone(),
                    active_request,
                ))
                .ok_or(ContractError::NotFound)?;
            if existing.state == ReservationState::Active {
                return Err(ContractError::AlreadyReserved);
            }
        }
        let cap_key = DataKey::Cap(tenant_id.clone(), request.cap_id.clone());
        let mut cap: BudgetCap = env
            .storage()
            .persistent()
            .get(&cap_key)
            .ok_or(ContractError::NotFound)?;
        if cap.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if cap.asset != request.asset {
            return Err(ContractError::AssetMismatch);
        }
        if cap.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::CapExpired);
        }
        let reserved = cap
            .reserved
            .checked_add(request.amount)
            .ok_or(ContractError::Overflow)?;
        if reserved > cap.limit {
            return Err(ContractError::CapExceeded);
        }
        Self::push_index(&env, &tenant_id, &project_id, &request.request_id)?;
        let reservation = Reservation {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            request_id: request.request_id.clone(),
            cap_id: request.cap_id.clone(),
            plan_hash: request.plan_hash.clone(),
            asset: request.asset.clone(),
            amount: request.amount,
            state: ReservationState::Active,
            version: 1,
            reserved_at: env.ledger().timestamp(),
            expires_at: request.expires_at,
            release_request_id: BytesN::from_array(&env, &[0u8; 32]),
        };
        cap.reserved = reserved;
        env.storage().persistent().set(&cap_key, &cap);
        Self::extend(&env, &cap_key);
        env.storage()
            .persistent()
            .set(&request_key, &reservation);
        Self::extend(&env, &request_key);
        env.storage()
            .persistent()
            .set(&project_key, &request.request_id);
        Self::extend(&env, &project_key);
        env.events().publish(
            (symbol_short!("CBRRES"),),
            (
                tenant_id,
                project_id,
                request.request_id,
                request.amount,
                request.asset,
            ),
        );
        Ok(reservation)
    }

    pub fn release(
        env: Env,
        sponsor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        release_request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &sponsor, &tenant_id, Role::Sponsor)?;
        let key = DataKey::Reservation(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let mut reservation: Reservation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if reservation.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if reservation.version != expected_version {
            return Err(ContractError::VersionConflict);
        }
        let request_key = DataKey::ReleaseRequest(
            tenant_id.clone(),
            project_id.clone(),
            release_request_id.clone(),
        );
        if let Some(marker) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestMarker>(&request_key)
        {
            if marker.base_version != expected_version || marker.request_hash != reservation.cap_id {
                return Err(ContractError::RequestConflict);
            }
            return Ok(());
        }
        if reservation.state != ReservationState::Active {
            return Err(ContractError::InvalidState);
        }
        let cap_key = DataKey::Cap(tenant_id.clone(), reservation.cap_id.clone());
        let mut cap: BudgetCap = env
            .storage()
            .persistent()
            .get(&cap_key)
            .ok_or(ContractError::NotFound)?;
        cap.reserved = cap
            .reserved
            .checked_sub(reservation.amount)
            .ok_or(ContractError::Underflow)?;
        reservation.state = ReservationState::Released;
        reservation.release_request_id = release_request_id.clone();
        env.storage().persistent().set(&cap_key, &cap);
        Self::extend(&env, &cap_key);
        env.storage().persistent().set(&key, &reservation);
        Self::extend(&env, &key);
        let marker = RequestMarker {
            base_version: expected_version,
            target_version: expected_version,
            request_hash: reservation.cap_id.clone(),
        };
        env.storage().persistent().set(&request_key, &marker);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBRREL"),),
            (tenant_id, project_id, request_id, reservation.amount),
        );
        Ok(())
    }

    pub fn mark_expired(
        env: Env,
        auditor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &auditor, &tenant_id, Role::Auditor)?;
        let key = DataKey::Reservation(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let mut reservation: Reservation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        if reservation.tenant_id != tenant_id {
            return Err(ContractError::WrongTenant);
        }
        if reservation.version != expected_version {
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
        if reservation.state != ReservationState::Active {
            return Err(ContractError::InvalidState);
        }
        if reservation.expires_at > env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }
        let cap_key = DataKey::Cap(tenant_id.clone(), reservation.cap_id.clone());
        let mut cap: BudgetCap = env
            .storage()
            .persistent()
            .get(&cap_key)
            .ok_or(ContractError::NotFound)?;
        cap.reserved = cap
            .reserved
            .checked_sub(reservation.amount)
            .ok_or(ContractError::Underflow)?;
        reservation.state = ReservationState::Expired;
        env.storage().persistent().set(&cap_key, &cap);
        Self::extend(&env, &cap_key);
        env.storage().persistent().set(&key, &reservation);
        Self::extend(&env, &key);
        env.storage().persistent().set(&request_key, &true);
        Self::extend(&env, &request_key);
        env.events().publish(
            (symbol_short!("CBREXP"),),
            (tenant_id, project_id, request_id, reservation.amount),
        );
        Ok(())
    }

    pub fn get_cap(
        env: Env,
        tenant_id: BytesN<32>,
        cap_id: BytesN<32>,
    ) -> Result<BudgetCap, ContractError> {
        let key = DataKey::Cap(tenant_id, cap_id);
        let cap = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(cap)
    }

    pub fn get_reservation(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
    ) -> Result<Reservation, ContractError> {
        let key = DataKey::Reservation(tenant_id, project_id, request_id);
        let reservation = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        Self::extend(&env, &key);
        Ok(reservation)
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
        request_id: &BytesN<32>,
    ) -> Result<(), ContractError> {
        let key = DataKey::Index(tenant_id.clone(), project_id.clone());
        let mut ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if ids.len() >= MAX_RESERVATIONS_PER_PROJECT {
            return Err(ContractError::CapacityExceeded);
        }
        ids.push_back(request_id.clone());
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
