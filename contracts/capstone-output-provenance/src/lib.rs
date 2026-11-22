#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    IntoVal, Val, Vec,
};

const RECORD_MIN_TTL: u32 = 100_000;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_HASHES_PER_FIELD: u32 = 16;
const MAX_TOTAL_HASHES: u32 = 64;
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
    RecordNotFound = 8,
    InvalidState = 9,
    TooManyHashes = 10,
    InvalidValidity = 11,
    Overflow = 12,
    AlreadyExpired = 13,
    AlreadyHasRole = 14,
    NotExpired = 15,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Administrator,
    Operator,
    ResearchLead,
    Verifier,
    Curator,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceState {
    Recorded,
    Verified,
    Rejected,
    Sealed,
    Expired,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Valid,
    Invalid,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentMarker {
    pub fingerprint: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceRecord {
    pub output_id: BytesN<32>,
    pub version: u32,
    pub sources: Vec<BytesN<32>>,
    pub methods: Vec<BytesN<32>>,
    pub tools: Vec<BytesN<32>>,
    pub transformations: Vec<BytesN<32>>,
    pub authorship: Vec<BytesN<32>>,
    pub commit_hash: BytesN<32>,
    pub license_hash: BytesN<32>,
    pub integrity_hash: BytesN<32>,
    pub state: ProvenanceState,
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
    Record(BytesN<32>, BytesN<32>, BytesN<32>),
    ByOutput(BytesN<32>, BytesN<32>, BytesN<32>, u32),
}

#[contract]
pub struct CapstoneOutputProvenanceContract;

#[contractimpl]
impl CapstoneOutputProvenanceContract {
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

    pub fn record_provenance(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
        output_id: BytesN<32>,
        version: u32,
        sources: Vec<BytesN<32>>,
        methods: Vec<BytesN<32>>,
        tools: Vec<BytesN<32>>,
        transformations: Vec<BytesN<32>>,
        authorship: Vec<BytesN<32>>,
        commit_hash: BytesN<32>,
        license_hash: BytesN<32>,
        integrity_hash: BytesN<32>,
        valid_for_secs: u64,
    ) -> Result<BytesN<32>, ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::ResearchLead)?;
        Self::validate_hashes(
            &sources,
            &methods,
            &tools,
            &transformations,
            &authorship,
        )?;
        if valid_for_secs == 0 || valid_for_secs > MAX_VALIDITY_SECS {
            return Err(ContractError::InvalidValidity);
        }
        let payload_hash = Self::payload_hash(
            &env,
            &sources,
            &methods,
            &tools,
            &transformations,
            &authorship,
            &commit_hash,
            &license_hash,
            &integrity_hash,
            valid_for_secs,
        );
        let fingerprint = Self::request_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &output_id,
            version,
            &actor,
            &payload_hash,
            1,
        );
        if Self::check_request(&env, &tenant_id, &project_id, &request_id, &fingerprint)? {
            return Ok(request_id);
        }
        let record_key = DataKey::Record(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if env.storage().persistent().has(&record_key) {
            return Err(ContractError::RequestAlreadyUsed);
        }
        let now = env.ledger().timestamp();
        let expires_at = now
            .checked_add(valid_for_secs)
            .ok_or(ContractError::Overflow)?;
        let record = ProvenanceRecord {
            output_id: output_id.clone(),
            version,
            sources,
            methods,
            tools,
            transformations,
            authorship,
            commit_hash,
            license_hash,
            integrity_hash,
            state: ProvenanceState::Recorded,
            created_at: now,
            expires_at,
        };
        Self::write(&env, &record_key, &record);
        let output_key = DataKey::ByOutput(
            tenant_id.clone(),
            project_id.clone(),
            output_id.clone(),
            version,
        );
        Self::write(&env, &output_key, &request_id);
        env.events().publish(
            (symbol_short!("PROVNEW"),),
            (tenant_id, project_id, request_id, output_id, version),
        );
        Ok(request_id)
    }

    pub fn attest_provenance(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        output_id: BytesN<32>,
        expected_version: u32,
        attestation_hash: BytesN<32>,
        verdict: Verdict,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Verifier)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &output_id,
            &actor,
            2,
            expected_version,
            &attestation_hash,
            verdict as u32,
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
        let (record_key, mut record) = Self::load_by_output(
            &env,
            &tenant_id,
            &project_id,
            &output_id,
            expected_version,
        )?;
        if env.ledger().timestamp() > record.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if record.state != ProvenanceState::Recorded {
            return Err(ContractError::InvalidState);
        }
        record.state = match verdict {
            Verdict::Valid => ProvenanceState::Verified,
            Verdict::Invalid => ProvenanceState::Rejected,
        };
        Self::write(&env, &record_key, &record);
        env.events().publish(
            (symbol_short!("PROVATT"),),
            (tenant_id, project_id, output_id, expected_version, verdict),
        );
        Ok(())
    }

    pub fn seal_provenance(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        output_id: BytesN<32>,
        expected_version: u32,
        seal_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Curator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &output_id,
            &actor,
            3,
            expected_version,
            &seal_hash,
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
        let (record_key, mut record) = Self::load_by_output(
            &env,
            &tenant_id,
            &project_id,
            &output_id,
            expected_version,
        )?;
        if env.ledger().timestamp() > record.expires_at {
            return Err(ContractError::AlreadyExpired);
        }
        if record.state != ProvenanceState::Verified {
            return Err(ContractError::InvalidState);
        }
        record.state = ProvenanceState::Sealed;
        Self::write(&env, &record_key, &record);
        env.events().publish(
            (symbol_short!("PROVSEA"),),
            (tenant_id, project_id, output_id, expected_version),
        );
        Ok(())
    }

    pub fn expire_provenance(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        intent_id: BytesN<32>,
        output_id: BytesN<32>,
        expected_version: u32,
    ) -> Result<(), ContractError> {
        Self::require_role(&env, &actor, &tenant_id, Role::Operator)?;
        let fingerprint = Self::intent_fingerprint(
            &env,
            &tenant_id,
            &project_id,
            &intent_id,
            &output_id,
            &actor,
            4,
            expected_version,
            &output_id,
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
        let (record_key, mut record) = Self::load_by_output(
            &env,
            &tenant_id,
            &project_id,
            &output_id,
            expected_version,
        )?;
        if env.ledger().timestamp() <= record.expires_at {
            return Err(ContractError::NotExpired);
        }
        if record.state == ProvenanceState::Expired {
            return Err(ContractError::InvalidState);
        }
        record.state = ProvenanceState::Expired;
        Self::write(&env, &record_key, &record);
        env.events().publish(
            (symbol_short!("PROVEXP"),),
            (tenant_id, project_id, output_id, expected_version),
        );
        Ok(())
    }

    pub fn get_provenance(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        output_id: BytesN<32>,
        version: u32,
    ) -> Result<ProvenanceRecord, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let (_, record) = Self::load_by_output(
            &env,
            &tenant_id,
            &project_id,
            &output_id,
            version,
        )?;
        Ok(record)
    }

    pub fn get_by_request(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        request_id: BytesN<32>,
    ) -> Result<ProvenanceRecord, ContractError> {
        Self::require_tenant(&env, &tenant_id)?;
        let key = DataKey::Record(tenant_id, project_id, request_id);
        env.storage()
            .persistent()
            .get::<_, ProvenanceRecord>(&key)
            .ok_or(ContractError::RecordNotFound)
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

    fn load_by_output(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        output_id: &BytesN<32>,
        version: u32,
    ) -> Result<(DataKey, ProvenanceRecord), ContractError> {
        let output_key = DataKey::ByOutput(
            tenant_id.clone(),
            project_id.clone(),
            output_id.clone(),
            version,
        );
        let request_id = env
            .storage()
            .persistent()
            .get::<_, BytesN<32>>(&output_key)
            .ok_or(ContractError::RecordNotFound)?;
        Self::extend(env, &output_key);
        let record_key = DataKey::Record(
            tenant_id.clone(),
            project_id.clone(),
            request_id,
        );
        let record = env
            .storage()
            .persistent()
            .get::<_, ProvenanceRecord>(&record_key)
            .ok_or(ContractError::RecordNotFound)?;
        Self::extend(env, &record_key);
        if record.version != version {
            return Err(ContractError::StaleVersion);
        }
        Ok((record_key, record))
    }

    fn validate_hashes(
        sources: &Vec<BytesN<32>>,
        methods: &Vec<BytesN<32>>,
        tools: &Vec<BytesN<32>>,
        transformations: &Vec<BytesN<32>>,
        authorship: &Vec<BytesN<32>>,
    ) -> Result<(), ContractError> {
        let source_len = sources.len();
        let method_len = methods.len();
        let tool_len = tools.len();
        let transformation_len = transformations.len();
        let authorship_len = authorship.len();
        if source_len > MAX_HASHES_PER_FIELD
            || method_len > MAX_HASHES_PER_FIELD
            || tool_len > MAX_HASHES_PER_FIELD
            || transformation_len > MAX_HASHES_PER_FIELD
            || authorship_len > MAX_HASHES_PER_FIELD
        {
            return Err(ContractError::TooManyHashes);
        }
        let total = source_len
            .checked_add(method_len)
            .and_then(|v| v.checked_add(tool_len))
            .and_then(|v| v.checked_add(transformation_len))
            .and_then(|v| v.checked_add(authorship_len))
            .ok_or(ContractError::Overflow)?;
        if total > MAX_TOTAL_HASHES {
            return Err(ContractError::TooManyHashes);
        }
        Ok(())
    }

    fn payload_hash(
        env: &Env,
        sources: &Vec<BytesN<32>>,
        methods: &Vec<BytesN<32>>,
        tools: &Vec<BytesN<32>>,
        transformations: &Vec<BytesN<32>>,
        authorship: &Vec<BytesN<32>>,
        commit_hash: &BytesN<32>,
        license_hash: &BytesN<32>,
        integrity_hash: &BytesN<32>,
        valid_for_secs: u64,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&valid_for_secs.to_be_bytes());
        Self::append_hashes(env, &mut preimage, sources);
        Self::append_hashes(env, &mut preimage, methods);
        Self::append_hashes(env, &mut preimage, tools);
        Self::append_hashes(env, &mut preimage, transformations);
        Self::append_hashes(env, &mut preimage, authorship);
        preimage.extend_from_array(&commit_hash.to_array());
        preimage.extend_from_array(&license_hash.to_array());
        preimage.extend_from_array(&integrity_hash.to_array());
        env.crypto().sha256(&preimage)
    }

    fn append_hashes(env: &Env, preimage: &mut Bytes, values: &Vec<BytesN<32>>) {
        preimage.extend_from_array(&values.len().to_be_bytes());
        for i in 0..values.len() {
            if let Some(value) = values.get(i) {
                preimage.extend_from_array(&value.to_array());
            }
        }
        let _ = env;
    }

    fn request_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        output_id: &BytesN<32>,
        version: u32,
        actor: &Address,
        payload_hash: &BytesN<32>,
        tag: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&request_id.to_array());
        preimage.extend_from_array(&output_id.to_array());
        preimage.extend_from_array(&payload_hash.to_array());
        preimage.append(&actor.clone().to_xdr(env));
        env.crypto().sha256(&preimage)
    }

    fn intent_fingerprint(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        intent_id: &BytesN<32>,
        output_id: &BytesN<32>,
        actor: &Address,
        tag: u32,
        version: u32,
        payload_hash: &BytesN<32>,
        code: u32,
    ) -> BytesN<32> {
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&tag.to_be_bytes());
        preimage.extend_from_array(&version.to_be_bytes());
        preimage.extend_from_array(&code.to_be_bytes());
        preimage.extend_from_array(&tenant_id.to_array());
        preimage.extend_from_array(&project_id.to_array());
        preimage.extend_from_array(&intent_id.to_array());
        preimage.extend_from_array(&output_id.to_array());
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
