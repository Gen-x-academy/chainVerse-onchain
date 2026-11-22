#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val,
};

const RECORD_MIN_TTL: u32 = 100_000;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_ACCEPTANCES: u32 = 8;
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
    TermsNotFound = 8,
    InvalidState = 9,
    InvalidPolicy = 10,
    Overflow = 11,
    NotExpired = 12,
    AlreadyAccepted = 13,
    AcceptanceLimit = 14,
    AlreadyHasRole = 15,
    InvalidCode = 16,
    AlreadyExpired = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Administrator,
    Operator,
    IpManager,
    Participant,
    Auditor,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IpTermsState {
    Draft,
    Accepted,
    Active,
    Superseded,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentMarker {
    pub fingerprint: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IpTerms {
    pub terms_id: BytesN<32>,
    pub version: u32,
    pub license_code: u32,
    pub ownership_code: u32,
    pub sponsor_rights_code: u32,
    pub commercialization_code: u32,
    pub reuse_code: u32,
    pub embargo_until: u64,
    pub terms_hash: BytesN<32>,
    pub required_acceptances: u32,
    pub acceptance_count: u32,
    pub state: IpTermsState,
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
    Terms(BytesN<32>, BytesN<32>, BytesN<32>),
    Acceptance(BytesN<32>, BytesN<32>, BytesN<32>, Address),
    Latest(BytesN<32>, BytesN<32>),
}

#[contract]
pub struct CapstoneIpTermsContract;

#[contractimpl]
impl CapstoneIpTermsContract {
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

    pub fn propose_terms(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        version: u32,
        license_code: u32,
        ownership_code: u32,
        sponsor_rights_code: u32,
        commercialization_code: u32,
        reuse_code: u32,
        embargo_until: u64,
        terms_hash: BytesN<32>,
        required_acceptances: u32,
        valid_for_secs: u64,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::IpManager)?;
        if license_code == 0
            || ownership_code == 0
            || sponsor_rights_code == 0
            || commercialization_code == 0
            || reuse_code == 0
        {
            return Err(ContractError::InvalidCode);
        }
        if required_acceptances == 0 || required_acceptances > MAX_ACCEPTANCES {
            return Err(ContractError::InvalidPolicy);
        }
        if valid_for_secs == 0 || valid_for_secs > MAX_VALIDITY_SECS {
            return Err(ContractError::InvalidPolicy);
        }
        let payload_hash = Self::policy_hash(
            &env,
            license_code,
            ownership_code,
            sponsor_rights_code,
            commercialization_code,
            reuse_code,
            embargo_until,
            &terms_hash,
            required_acceptances,
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
        let terms_key = DataKey::Terms(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&terms_key) {
            return Err(ContractError::RequestAlreadyUsed);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(valid_for_secs)
            .ok_or(ContractError::Overflow)?;
        let terms = IpTerms {
            terms_id: request_id.clone(),
            version,
            license_code,
            ownership_code,
            sponsor_rights_code,
            commercialization_code,
            reuse_code,
            embargo_until,
            terms_hash,
            required_acceptances,
            acceptance_count: 0,
            state: IpTermsState::Draft,
            created_at: now,
            expires_at,
        };
        Self::write(&env, &terms_key, &terms);
        let latest_key = DataKey::Latest(tenant_id.clone(), project_id.clone());
        Self::write(&env, &latest_key, &request_id);
        env.events().publish(
            (symbol_short!("IPNEW"),),
            (tenant_id, project_id, request_id, version, required_acceptances),
        );
        Ok(request_id)
    }

    pub fn accept_terms(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        terms_id: BytesN<32>,
        expected_version: u32,
        acceptance_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Participant)?;
        let acceptance_key = DataKey::Acceptance(
            tenant_id.clone(),
            project_id.clone(),
            terms_id.clone(),
            actor.clone(),
        );
        if env.storage().persistent().has(&acceptance_key) {
            return Err(ContractError::AlreadyAccepted);
        }
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &terms_id,
            &actor,
            2,
            expected_version,
            &acceptance_hash,
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
            let (_, existing) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
            return Ok(existing.acceptance_count);
        }
        let (terms_key, mut terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        if env.ledger().timestamp() > terms.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if terms.state != IpTermsState::Draft && terms.state != IpTermsState::Accepted {
            return Err(ContractError::InvalidState);
        }
        if terms.acceptance_count >= terms.required_acceptances {
            return Err(ContractError::AcceptanceLimit);
        }
        let count = terms
            .acceptance_count
            .checked_add(1)
            .ok_or(ContractError::Overflow)?;
        terms.acceptance_count = count;
        if count >= terms.required_acceptances {
            terms.state = IpTermsState::Accepted;
        }
        Self::write(&env, &terms_key, &terms);
        Self::write(&env, &acceptance_key, &true);
        env.events().publish(
            (symbol_short!("IPACPT"),),
            (tenant_id, project_id, terms_id, expected_version, count),
        );
        Ok(count)
    }

    pub fn activate_terms(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        terms_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Administrator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &terms_id,
            &actor,
            3,
            expected_version,
            &terms_id,
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
        let (terms_key, mut terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        if env.ledger().timestamp() > terms.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if terms.state != IpTermsState::Accepted {
            return Err(ContractError::InvalidState);
        }
        let required = terms.required_acceptances;
        if terms.acceptance_count < required {
            return Err(ContractError::InvalidState);
        }
        terms.state = IpTermsState::Active;
        Self::write(&env, &terms_key, &terms);
        env.events().publish(
            (symbol_short!("IPACT"),),
            (tenant_id, project_id, terms_id, expected_version),
        );
        Ok(())
    }

    pub fn set_embargo(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        terms_id: BytesN<32>,
        expected_version: u32,
        embargo_until: u64,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::IpManager)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &terms_id,
            &actor,
            4,
            expected_version,
            &terms_id,
            0,
            embargo_until,
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
        let (terms_key, mut terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        if env.ledger().timestamp() > terms.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if terms.state != IpTermsState::Accepted && terms.state != IpTermsState::Active {
            return Err(ContractError::InvalidState);
        }
        if embargo_until <= env.ledger().timestamp() {
            return Err(ContractError::InvalidPolicy);
        }
        terms.embargo_until = embargo_until;
        Self::write(&env, &terms_key, &terms);
        env.events().publish(
            (symbol_short!("IPEMB"),),
            (tenant_id, project_id, terms_id, expected_version, embargo_until),
        );
        Ok(())
    }

    pub fn supersede_terms(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        terms_id: BytesN<32>,
        expected_version: u32,
        replacement_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::IpManager)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &terms_id,
            &actor,
            5,
            expected_version,
            &replacement_id,
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
        let (terms_key, mut terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        if terms.state != IpTermsState::Active {
            return Err(ContractError::InvalidState);
        }
        terms.state = IpTermsState::Superseded;
        Self::write(&env, &terms_key, &terms);
        env.events().publish(
            (symbol_short!("IPSUP"),),
            (tenant_id, project_id, terms_id, expected_version, replacement_id),
        );
        Ok(())
    }

    pub fn expire_terms(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        terms_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Operator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &terms_id,
            &actor,
            6,
            expected_version,
            &terms_id,
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
        let (terms_key, mut terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        if env.ledger().timestamp() <= terms.expires_at {
            return Err(ContractError::NotExpired);
        }
        if terms.state == IpTermsState::Expired {
            return Err(ContractError::InvalidState);
        }
        terms.state = IpTermsState::Expired;
        Self::write(&env, &terms_key, &terms);
        env.events().publish(
            (symbol_short!("IPEXP"),),
            (tenant_id, project_id, terms_id, expected_version),
        );
        Ok(())
    }

    pub fn get_terms(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        terms_id: BytesN<32>,
    ) -> Result<IpTerms, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let (_, terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        Ok(terms)
    }

    pub fn get_latest_terms(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
    ) -> Result<IpTerms, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let key = DataKey::Latest(tenant_id.clone(), project_id.clone());
        let terms_id = env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&key)
            .ok_or(ContractError::TermsNotFound)?;
        Self::extend(&env, &key);
        let (_, terms) = Self::load_terms(&env, &tenant_id, &project_id, &terms_id)?;
        Ok(terms)
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

    fn load_terms(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        terms_id: &BytesN<32>,
    ) -> Result<(DataKey, IpTerms), ContractError> {
        let key = DataKey::Terms(
            tenant_id.clone(),
            project_id.clone(),
            terms_id.clone(),
        );
        let terms = env
            .storage()
            .persistent()
            .get::<_, IpTerms>(&key)
            .ok_or(ContractError::TermsNotFound)?;
        Self::extend(env, &key);
        Ok((key, terms))
    }

    fn policy_hash(
        env: &Env,
        license_code: u32,
        ownership_code: u32,
        sponsor_rights_code: u32,
        commercialization_code: u32,
        reuse_code: u32,
        embargo_until: u64,
        terms_hash: &BytesN<32>,
        required_acceptances: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&license_code.to_be_bytes());
        preimage.extend_from_array(&ownership_code.to_be_bytes());
        preimage.extend_from_array(&sponsor_rights_code.to_be_bytes());
        preimage.extend_from_array(&commercialization_code.to_be_bytes());
        preimage.extend_from_array(&reuse_code.to_be_bytes());
        preimage.extend_from_array(&embargo_until.to_be_bytes());
        preimage.extend_from_array(&required_acceptances.to_be_bytes());
        preimage.extend_from_array(&terms_hash.to_array());
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
