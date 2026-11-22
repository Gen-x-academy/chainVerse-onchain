#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val, Vec,
};

const RECORD_MIN_TTL: u32 = 100_000;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_REQUIRED_ROLES: u32 = 5;
const MAX_VALIDITY_SECS: u64 = 31_536_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    WrongTenant = 4,
    RequestAlreadyUsed = 5,
    IntentAlreadyUsed = 6,
    StaleVersion = 7,
    ApprovalNotFound = 8,
    InvalidState = 9,
    InvalidRoleSet = 10,
    TooManyRoles = 11,
    NotExpired = 12,
    AlreadyVoted = 13,
    Overflow = 14,
    InvalidValidity = 15,
    AlreadyHasRole = 16,
    AlreadyExpired = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Administrator,
    Operator,
    Author,
    Supervisor,
    Sponsor,
    Ethics,
    Organization,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalState {
    Requested,
    Approved,
    Rejected,
    Released,
    Expired,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Approve,
    Reject,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseCode {
    PublicRepository,
    ExternalSubmission,
    SponsorPortal,
    Demonstration,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentMarker {
    pub fingerprint: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequest {
    pub request_id: BytesN<32>,
    pub version: u32,
    pub release_code: ReleaseCode,
    pub target_hash: BytesN<32>,
    pub required_roles: Vec<Role>,
    pub approvals: u32,
    pub rejections: u32,
    pub state: ApprovalState,
    pub created_at: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    TenantAdmin(BytesN<32>),
    Role(BytesN<32>, Address, Role),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
    Intent(BytesN<32>, BytesN<32>, BytesN<32>),
    Approval(BytesN<32>, BytesN<32>, BytesN<32>),
    Vote(BytesN<32>, BytesN<32>, BytesN<32>, Address),
}

#[contract]
pub struct CapstonePublicationApprovalsContract;

#[contractimpl]
impl CapstonePublicationApprovalsContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        admin.require_auth();
        let admin_key = DataKey::TenantAdmin(tenant_id.clone());
        if env.storage().persistent().has(&admin_key) {
            return Err(ContractError::AlreadyInitialized);
        }
        Self::write(&env, &admin_key, &admin);
        let role_key = DataKey::Role(tenant_id.clone(), admin, Role::Administrator);
        Self::write(&env, &role_key, &true);
        env.events().publish((symbol_short!("CAPTNT"),), (tenant_id,));
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &admin, &tenant_id, Role::Administrator)?;
        let key = DataKey::Role(tenant_id.clone(), account, role);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyHasRole);
        }
        Self::write(&env, &key, &true);
        env.events()
            .publish((symbol_short!("CAPROL"),), (tenant_id, role));
        Ok(())
    }

    pub fn request_approval(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        version: u32,
        release_code: ReleaseCode,
        target_hash: BytesN<32>,
        required_roles: Vec<Role>,
        valid_for_secs: u64,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Author)?;
        Self::validate_required_roles(&required_roles)?;
        if valid_for_secs == 0 || valid_for_secs > MAX_VALIDITY_SECS {
            return Err(ContractError::InvalidValidity);
        }
        let payload_hash = Self::request_payload_hash(
            &env,
            release_code as u32,
            &target_hash,
            &required_roles,
            valid_for_secs,
        );
        let fingerprint = Self::request_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &actor,
            &payload_hash,
            1,
            version,
        );
        if Self::check_request(&env, &tenant_id, &project_id, &request_id, &fingerprint)? {
            return Ok(request_id);
        }
        let approval_key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&approval_key) {
            return Err(ContractError::RequestAlreadyUsed);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(valid_for_secs)
            .ok_or(ContractError::Overflow)?;
        let request = ApprovalRequest {
            request_id: request_id.clone(),
            version,
            release_code,
            target_hash,
            required_roles,
            approvals: 0,
            rejections: 0,
            state: ApprovalState::Requested,
            created_at: now,
            expires_at,
        };
        Self::write(&env, &approval_key, &request);
        env.events().publish(
            (symbol_short!("PUBNEW"),),
            (tenant_id, project_id, request_id, version, release_code),
        );
        Ok(request_id)
    }

    pub fn decide(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        decision: Decision,
        decision_hash: BytesN<32>,
    ) -> Result<ApprovalState, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let (approval_key, mut request) =
            Self::load_request(&env, &tenant_id, &project_id, &request_id)?;
        Self::require_approval_role(&env, &actor, &tenant_id, &request.required_roles)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &request_id,
            &actor,
            2,
            expected_version,
            &decision_hash,
            decision as u32,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            return Ok(request.state);
        }
        if request.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if request.state != ApprovalState::Requested {
            return Err(ContractError::InvalidState);
        }
        if env.ledger().timestamp() > request.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        let vote_key = DataKey::Vote(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
            actor,
        );
        if env.storage().persistent().has(&vote_key) {
            return Err(ContractError::AlreadyVoted);
        }
        match decision {
            Decision::Approve => {
                let approvals = request
                    .approvals
                    .checked_add(1)
                    .ok_or(ContractError::Overflow)?;
                request.approvals = approvals;
                if approvals >= request.required_roles.len() {
                    request.state = ApprovalState::Approved;
                }
            }
            Decision::Reject => {
                let rejections = request
                    .rejections
                    .checked_add(1)
                    .ok_or(ContractError::Overflow)?;
                request.rejections = rejections;
                request.state = ApprovalState::Rejected;
            }
        }
        Self::write(&env, &approval_key, &request);
        Self::write(&env, &vote_key, &true);
        env.events().publish(
            (symbol_short!("PUBDEC"),),
            (tenant_id, project_id, request_id, expected_version, decision),
        );
        Ok(request.state)
    }

    pub fn finalize_approval(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        release_hash: BytesN<32>,
    ) -> Result<ApprovalState, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Administrator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &request_id,
            &actor,
            3,
            expected_version,
            &release_hash,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            let (_, existing) = Self::load_request(&env, &tenant_id, &project_id, &request_id)?;
            return Ok(existing.state);
        }
        let (approval_key, mut request) =
            Self::load_request(&env, &tenant_id, &project_id, &request_id)?;
        if request.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if request.state != ApprovalState::Approved {
            return Err(ContractError::InvalidState);
        }
        let required = request.required_roles.len();
        if request.approvals != required || request.rejections != 0 {
            return Err(ContractError::InvalidState);
        }
        request.state = ApprovalState::Released;
        Self::write(&env, &approval_key, &request);
        env.events().publish(
            (symbol_short!("PUBFIN"),),
            (tenant_id, project_id, request_id, expected_version, release_hash),
        );
        Ok(request.state)
    }

    pub fn expire_approval(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Operator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &request_id,
            &actor,
            4,
            expected_version,
            &request_id,
            0,
            0,
        );
        if Self::check_intent(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &fingerprint,
            expected_version,
        )? {
            return Ok(());
        }
        let (approval_key, mut request) =
            Self::load_request(&env, &tenant_id, &project_id, &request_id)?;
        if request.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if env.ledger().timestamp() <= request.expires_at {
            return Err(ContractError::NotExpired);
        }
        if request.state != ApprovalState::Requested && request.state != ApprovalState::Approved {
            return Err(ContractError::InvalidState);
        }
        request.state = ApprovalState::Expired;
        Self::write(&env, &approval_key, &request);
        env.events().publish(
            (symbol_short!("PUBEXP"),),
            (tenant_id, project_id, request_id, expected_version),
        );
        Ok(())
    }

    pub fn get_approval(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
    ) -> Result<ApprovalRequest, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let (_, request) = Self::load_request(&env, &tenant_id, &project_id, &request_id)?;
        Ok(request)
    }

    fn require_tenant(env: &Env, tenant_id: &BytesN<32>) -> Result<(), ContractError> {
        let key = DataKey::TenantAdmin(tenant_id.clone());
        if !env.storage().persistent().has(&key) {
            return Err(ContractError::WrongTenant);
        }
        Self::extend(env, &key);
        Ok(())
    }

    fn require_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_tenant(env, tenant_id)?;
        actor.require_auth();
        let admin_key = DataKey::Role(tenant_id.clone(), actor.clone(), Role::Administrator);
        if env.storage().persistent().get::<_, bool>(&admin_key).unwrap_or(false) {
            Self::extend(env, &admin_key);
            return Ok(());
        }
        let key = DataKey::Role(tenant_id.clone(), actor.clone(), role);
        if env.storage().persistent().get::<_, bool>(&key).unwrap_or(false) {
            Self::extend(env, &key);
            return Ok(());
        }
        Err(ContractError::NotAuthorized)
    }

    fn require_approval_role(
        env: &Env,
        actor: &Address,
        tenant_id: &BytesN<32>,
        required_roles: &Vec<Role>,
    ) -> Result<(), ContractError> {
        Self::require_tenant(env, tenant_id)?;
        actor.require_auth();
        for i in 0..required_roles.len() {
            if let Some(role) = required_roles.get(i) {
                let key = DataKey::Role(tenant_id.clone(), actor.clone(), role);
                if env.storage().persistent().get::<_, bool>(&key).unwrap_or(false) {
                    Self::extend(env, &key);
                    return Ok(());
                }
            }
        }
        Err(ContractError::NotAuthorized)
    }

    fn validate_required_roles(roles: &Vec<Role>) -> Result<(), ContractError> {
        let len = roles.len();
        if len == 0 || len > MAX_REQUIRED_ROLES {
            return Err(ContractError::TooManyRoles);
        }
        for i in 0..len {
            let role = roles.get(i).ok_or(ContractError::InvalidRoleSet)?;
            if !Self::is_approval_role(role) {
                return Err(ContractError::InvalidRoleSet);
            }
            for j in 0..i {
                if roles.get(j) == Some(role) {
                    return Err(ContractError::InvalidRoleSet);
                }
            }
        }
        Ok(())
    }

    fn is_approval_role(role: Role) -> bool {
        match role {
            Role::Author
            | Role::Supervisor
            | Role::Sponsor
            | Role::Ethics
            | Role::Organization => true,
            _ => false,
        }
    }

    fn check_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        fingerprint: &BytesN<32>,
    ) -> Result<bool, ContractError> {
        let key = DataKey::Request(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(stored) = env.storage().persistent().get::<_, BytesN<32>>(&key) {
            Self::extend(env, &key);
            if stored == *fingerprint {
                return Ok(true);
            }
            return Err(ContractError::RequestAlreadyUsed);
        }
        Self::write(env, &key, fingerprint);
        Ok(false)
    }

    fn check_intent(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
        fingerprint: &BytesN<32>,
        version: u32,
    ) -> Result<bool, ContractError> {
        let key = DataKey::Intent(
            tenant_id.clone(),
            project_id.clone(),
            intent_id.clone(),
        );
        if let Some(marker) = env.storage().persistent().get::<_, IntentMarker>(&key) {
            Self::extend(env, &key);
            if marker.fingerprint == *fingerprint && marker.version == version {
                return Ok(true);
            }
            return Err(ContractError::IntentAlreadyUsed);
        }
        Self::write(
            env,
            &key,
            &IntentMarker {
                fingerprint: fingerprint.clone(),
                version,
            },
        );
        Ok(false)
    }

    fn load_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
    ) -> Result<(DataKey, ApprovalRequest), ContractError> {
        let key = DataKey::Approval(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let request = env
            .storage()
            .persistent()
            .get::<_, ApprovalRequest>(&key)
            .ok_or(ContractError::ApprovalNotFound)?;
        Self::extend(env, &key);
        Ok((key, request))
    }

    fn request_payload_hash(
        env: &Env,
        release_code: u32,
        target_hash: &BytesN<32>,
        required_roles: &Vec<Role>,
        valid_for_secs: u64,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&release_code.to_be_bytes());
        preimage.extend_from_array(&valid_for_secs.to_be_bytes());
        preimage.extend_from_array(&required_roles.len().to_be_bytes());
        preimage.extend_from_array(&target_hash.to_array());
        for i in 0..required_roles.len() {
            if let Some(role) = required_roles.get(i) {
                preimage.extend_from_array(&(role as u32).to_be_bytes());
            }
        }
        env.crypto().sha256(&preimage)
    }

    fn request_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        actor: &Address,
        payload_hash: &BytesN<32>,
        tag: u32,
        version: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&request_id.to_array());
        preimage.extend_from_array(&payload_hash.to_array());
        preimage.append(&actor.clone().to_xdr(env));
        env.crypto().sha256(&preimage)
    }

    fn intent_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
        target_id: &BytesN<32>,
        actor: &Address,
        tag: u32,
        version: u32,
        payload_hash: &BytesN<32>,
        code: u32,
        aux: u64,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&code.to_be_bytes());
        preimage.extend_from_array(&aux.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&intent_id.to_array());
        preimage.extend_from_array(&target_id.to_array());
        preimage.extend_from_array(&payload_hash.to_array());
        preimage.append(&actor.clone().to_xdr(env));
        env.crypto().sha256(&preimage)
    }

    fn write<T>(env: &Env, key: &DataKey, value: &T)
    where
        T: IntoVal<Env, Val>,
    {
        env.storage().persistent().set(key, value);
        Self::extend(env, key);
    }

    fn extend(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}

#[cfg(test)]
mod tests;
