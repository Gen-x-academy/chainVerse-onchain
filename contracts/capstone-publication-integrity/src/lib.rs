#![no_std]

//! Privacy-minimized on-chain primitives for the capstone publishing and
//! originality work items (CAPSTONE-094..097).
//!
//! Only opaque ids, addresses, bounded 32-byte commitments, u32 codes,
//! counters and timestamps are stored. Credential text, output payloads,
//! export archives, similarity reports and citation text stay off-chain.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_DURATION: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_ROLE_CODE: u32 = 64;
const BPS: u32 = 10_000;

const KIND_CREDENTIAL: u32 = 1;
const KIND_OUTPUT: u32 = 2;
const KIND_EXPORT: u32 = 3;
const KIND_ORIGINALITY: u32 = 4;

const ROLE_ISSUER: u32 = 1;
const ROLE_VERIFIER: u32 = 2;
const ROLE_CURATOR: u32 = 3;
const ROLE_REVIEWER: u32 = 4;

const CREDENTIAL_ACTIVE: u32 = 1;
const CREDENTIAL_REVOKED: u32 = 2;

const OUTPUT_VALID: u32 = 1;
const OUTPUT_REVOKED: u32 = 2;
const OUTPUT_SUPERSEDED: u32 = 3;
const OUTPUT_EMBARGOED: u32 = 4;
const OUTPUT_MALFORMED: u32 = 5;

const EXPORT_PREPARED: u32 = 1;
const EXPORT_ACCESS: u32 = 2;
const EXPORT_REVOKED: u32 = 3;

const CHECK_REQUESTED: u32 = 1;
const CHECK_COMPLETED: u32 = 2;
const CHECK_HUMAN_REVIEW: u32 = 3;
const CHECK_ACCEPTED: u32 = 4;
const CHECK_REJECTED: u32 = 5;

const CRED_TOPIC: Symbol = symbol_short!("CAPCRED");
const OUT_TOPIC: Symbol = symbol_short!("CAPOUT");
const EXP_TOPIC: Symbol = symbol_short!("CAPEXP");
const ORG_TOPIC: Symbol = symbol_short!("CAPORG");

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidInput = 6,
    Expired = 7,
    InvalidState = 8,
    StaleVersion = 9,
    RequestConflict = 10,
    ArithmeticOverflow = 11,
    InvalidCommitment = 12,
    RoleLimitReached = 13,
    WrongKind = 14,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    SchemaVersion,
    Role(BytesN<32>, Address, u32),
    RoleCount(BytesN<32>, u32),
    Record(u32, BytesN<32>, BytesN<32>, BytesN<32>),
    Request(BytesN<32>, BytesN<32>, BytesN<32>),
}

/// All bounded inputs for a mutation, bundled so contract entrypoints stay
/// within Soroban's 10-parameter ABI limit. Fields not used by an operation
/// must be zero/empty.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapstoneInput {
    pub reference: BytesN<32>,
    pub commitment: BytesN<32>,
    pub aux: BytesN<32>,
    pub score_bps: u32,
    pub threshold_bps: u32,
    pub reason_code: u32,
    pub duration_seconds: u32,
    pub request_id: BytesN<32>,
    pub input_hash: BytesN<32>,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapstoneRecord {
    pub kind: u32,
    pub state: u32,
    pub subject: Address,
    pub reference: BytesN<32>,
    pub commitment: BytesN<32>,
    pub aux: BytesN<32>,
    pub score_bps: u32,
    pub threshold_bps: u32,
    pub reason_code: u32,
    pub created_at: u64,
    pub expires_at: u64,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub input_hash: BytesN<32>,
    pub record_id: BytesN<32>,
    pub version: u32,
}

#[contract]
pub struct CapstonePublicationIntegrityContract;

#[contractimpl]
impl CapstonePublicationIntegrityContract {
    // Shared administration: deny-by-default, tenant-scoped roles.

    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::SchemaVersion, &1u32);
        Ok(())
    }

    pub fn version(env: Env) -> Result<u32, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::SchemaVersion)
            .ok_or(ContractError::NotInitialized)
    }

    /// The global admin only manages roles; capability writes still require
    /// an explicitly granted role for the tenant.
    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        actor: Address,
        role: u32,
    ) -> Result<(), ContractError> {
        require_admin(&env, &admin)?;
        require_nonzero(&env, &tenant_id)?;
        if !(1..=MAX_ROLE_CODE).contains(&role) {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::Role(tenant_id.clone(), actor.clone(), role);
        if env.storage().persistent().has(&key) {
            return Ok(());
        }
        let count_key = DataKey::RoleCount(tenant_id.clone(), role);
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_ROLES {
            return Err(ContractError::RoleLimitReached);
        }
        env.storage().persistent().set(&key, &true);
        env.storage().persistent().set(&count_key, &(count + 1));
        extend(&env, &key);
        extend(&env, &count_key);
        Ok(())
    }

    pub fn revoke_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        actor: Address,
        role: u32,
    ) -> Result<(), ContractError> {
        require_admin(&env, &admin)?;
        require_nonzero(&env, &tenant_id)?;
        let key = DataKey::Role(tenant_id.clone(), actor.clone(), role);
        if !env.storage().persistent().has(&key) {
            return Ok(());
        }
        let count_key = DataKey::RoleCount(tenant_id.clone(), role);
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        env.storage().persistent().remove(&key);
        if count <= 1 {
            env.storage().persistent().remove(&count_key);
        } else {
            env.storage().persistent().set(&count_key, &(count - 1));
            extend(&env, &count_key);
        }
        Ok(())
    }

    pub fn has_role(env: Env, tenant_id: BytesN<32>, actor: Address, role: u32) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Role(tenant_id, actor, role))
    }

    // CAPSTONE-094 — completion and contribution credentials.

    /// `commitment` commits to completed rules, `aux` to contributor approvals.
    /// `reason_code` is the credential role code; both commitments are required.
    pub fn mint_credential(
        env: Env,
        issuer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        credential_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &issuer, &tenant_id, ROLE_ISSUER)?;
        require_nonzero(&env, &credential_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        require_nonzero(&env, &input.aux)?;
        if !(1..=MAX_ROLE_CODE).contains(&input.reason_code) || input.expected_version != 0 {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &credential_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CREDENTIAL, &tenant_id, &project_id, &credential_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = CapstoneRecord {
            kind: KIND_CREDENTIAL,
            state: CREDENTIAL_ACTIVE,
            subject: issuer,
            reference: input.reference.clone(),
            commitment: input.commitment.clone(),
            aux: input.aux.clone(),
            score_bps: 0,
            threshold_bps: 0,
            reason_code: input.reason_code,
            created_at: env.ledger().timestamp(),
            expires_at: 0,
            version: 1,
        };
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &credential_id, &input, 1);
        emit(
            &env,
            CRED_TOPIC,
            &tenant_id,
            &project_id,
            &credential_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn revoke_credential(
        env: Env,
        issuer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        credential_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &issuer, &tenant_id, ROLE_ISSUER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &credential_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CREDENTIAL, &tenant_id, &project_id, &credential_id);
        let mut record = load_record(&env, &key, KIND_CREDENTIAL)?;
        if record.state != CREDENTIAL_ACTIVE {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = CREDENTIAL_REVOKED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &project_id,
            &credential_id,
            &input,
            version,
        );
        emit(
            &env,
            CRED_TOPIC,
            &tenant_id,
            &project_id,
            &credential_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_credential(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        credential_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_CREDENTIAL, &tenant_id, &project_id, &credential_id),
            KIND_CREDENTIAL,
        )
    }

    // CAPSTONE-095 — public output verification states.

    /// `reason_code` is the new state: 1 Valid, 2 Revoked, 3 Superseded,
    /// 4 Embargoed, 5 Malformed. Absent records read as `0` (Unknown).
    pub fn set_output_state(
        env: Env,
        verifier: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        output_hash: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &verifier, &tenant_id, ROLE_VERIFIER)?;
        require_nonzero(&env, &output_hash)?;
        require_nonzero(&env, &input.aux)?;
        let state = input.reason_code;
        if !matches!(
            state,
            OUTPUT_VALID | OUTPUT_REVOKED | OUTPUT_SUPERSEDED | OUTPUT_EMBARGOED | OUTPUT_MALFORMED
        ) {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &output_hash, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_OUTPUT, &tenant_id, &project_id, &output_hash);
        let version;
        let mut record =
            if let Some(existing) = env.storage().persistent().get::<_, CapstoneRecord>(&key) {
                if !allowed_output_transition(existing.state, state) {
                    return Err(ContractError::InvalidState);
                }
                version = next_version(existing.version, input.expected_version)?;
                existing
            } else {
                if input.expected_version != 0
                    || !matches!(state, OUTPUT_VALID | OUTPUT_EMBARGOED | OUTPUT_MALFORMED)
                {
                    return Err(ContractError::InvalidInput);
                }
                version = 1;
                CapstoneRecord {
                    kind: KIND_OUTPUT,
                    state: 0,
                    subject: verifier,
                    reference: output_hash.clone(),
                    commitment: zero_hash(&env),
                    aux: zero_hash(&env),
                    score_bps: 0,
                    threshold_bps: 0,
                    reason_code: 0,
                    created_at: env.ledger().timestamp(),
                    expires_at: 0,
                    version: 0,
                }
            };
        record.state = state;
        record.aux = input.aux.clone();
        record.version = version;
        env.storage().persistent().set(&key, &record);
        extend(&env, &key);
        finish_request(&env, &tenant_id, &project_id, &output_hash, &input, version);
        emit(
            &env,
            OUT_TOPIC,
            &tenant_id,
            &project_id,
            &output_hash,
            state,
            version,
        );
        Ok(version)
    }

    pub fn get_output_state(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        output_hash: BytesN<32>,
    ) -> Result<u32, ContractError> {
        let key = record_key(KIND_OUTPUT, &tenant_id, &project_id, &output_hash);
        match env.storage().persistent().get::<_, CapstoneRecord>(&key) {
            Some(record) => {
                extend(&env, &key);
                Ok(record.state)
            }
            None => Ok(0),
        }
    }

    // CAPSTONE-096 — reproducible research package exports.

    /// `reference` commits to outputs, metadata, citations, dependency
    /// manifest, licenses and instructions; `commitment` is the checksum and
    /// `aux` the license commitment. `duration_seconds` bounds access.
    pub fn prepare_export(
        env: Env,
        curator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        export_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &curator, &tenant_id, ROLE_CURATOR)?;
        require_nonzero(&env, &export_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        require_nonzero(&env, &input.aux)?;
        if input.duration_seconds == 0
            || input.duration_seconds > MAX_DURATION
            || input.expected_version != 0
        {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &export_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EXPORT, &tenant_id, &project_id, &export_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let expires_at = env
            .ledger()
            .timestamp()
            .checked_add(input.duration_seconds as u64)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let record = CapstoneRecord {
            kind: KIND_EXPORT,
            state: EXPORT_PREPARED,
            subject: curator,
            reference: input.reference.clone(),
            commitment: input.commitment.clone(),
            aux: input.aux.clone(),
            score_bps: 0,
            threshold_bps: 0,
            reason_code: 0,
            created_at: env.ledger().timestamp(),
            expires_at,
            version: 1,
        };
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &export_id, &input, 1);
        emit(
            &env,
            EXP_TOPIC,
            &tenant_id,
            &project_id,
            &export_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn grant_export_access(
        env: Env,
        curator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        export_id: BytesN<32>,
        accessor: Address,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &curator, &tenant_id, ROLE_CURATOR)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &export_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EXPORT, &tenant_id, &project_id, &export_id);
        let mut record = load_record(&env, &key, KIND_EXPORT)?;
        require_live(&record, env.ledger().timestamp())?;
        if record.state != EXPORT_PREPARED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = EXPORT_ACCESS;
        record.subject = accessor;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &export_id, &input, version);
        emit(
            &env,
            EXP_TOPIC,
            &tenant_id,
            &project_id,
            &export_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn revoke_export_access(
        env: Env,
        curator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        export_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &curator, &tenant_id, ROLE_CURATOR)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &export_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EXPORT, &tenant_id, &project_id, &export_id);
        let mut record = load_record(&env, &key, KIND_EXPORT)?;
        if record.state != EXPORT_ACCESS {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = EXPORT_REVOKED;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &export_id, &input, version);
        emit(
            &env,
            EXP_TOPIC,
            &tenant_id,
            &project_id,
            &export_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn export_access_active(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        export_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let record = load_record(
            &env,
            &record_key(KIND_EXPORT, &tenant_id, &project_id, &export_id),
            KIND_EXPORT,
        )?;
        Ok(record.state == EXPORT_ACCESS && record.expires_at > env.ledger().timestamp())
    }

    pub fn get_export(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        export_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_EXPORT, &tenant_id, &project_id, &export_id),
            KIND_EXPORT,
        )
    }

    // CAPSTONE-097 — consent-aware originality and citation checks.

    /// `reference` is the submission hash, `commitment` the citation hash,
    /// `aux` a restricted off-chain report reference. `threshold_bps` is the
    /// provider-independent review threshold.
    pub fn request_similarity_check(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        check_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        require_nonzero(&env, &check_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        require_nonzero(&env, &input.aux)?;
        if input.threshold_bps > BPS || input.expected_version != 0 {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &check_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_ORIGINALITY, &tenant_id, &project_id, &check_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = CapstoneRecord {
            kind: KIND_ORIGINALITY,
            state: CHECK_REQUESTED,
            subject: reviewer,
            reference: input.reference.clone(),
            commitment: input.commitment.clone(),
            aux: input.aux.clone(),
            score_bps: 0,
            threshold_bps: input.threshold_bps,
            reason_code: 0,
            created_at: env.ledger().timestamp(),
            expires_at: 0,
            version: 1,
        };
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &check_id, &input, 1);
        emit(
            &env,
            ORG_TOPIC,
            &tenant_id,
            &project_id,
            &check_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn record_similarity_score(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        check_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        if input.score_bps > BPS {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &check_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_ORIGINALITY, &tenant_id, &project_id, &check_id);
        let mut record = load_record(&env, &key, KIND_ORIGINALITY)?;
        if record.state != CHECK_REQUESTED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.score_bps = input.score_bps;
        record.state = if input.score_bps >= record.threshold_bps {
            CHECK_HUMAN_REVIEW
        } else {
            CHECK_COMPLETED
        };
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &check_id, &input, version);
        emit(
            &env,
            ORG_TOPIC,
            &tenant_id,
            &project_id,
            &check_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// `reason_code`: 1 accepted, 2 rejected. Only a human reviewer may
    /// finalize a check that crossed the threshold.
    pub fn finalize_similarity_check(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        check_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        require_nonzero(&env, &input.aux)?;
        if !matches!(input.reason_code, 1 | 2) {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &check_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_ORIGINALITY, &tenant_id, &project_id, &check_id);
        let mut record = load_record(&env, &key, KIND_ORIGINALITY)?;
        if record.state != CHECK_HUMAN_REVIEW {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = if input.reason_code == 1 {
            CHECK_ACCEPTED
        } else {
            CHECK_REJECTED
        };
        record.reason_code = input.reason_code;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &check_id, &input, version);
        emit(
            &env,
            ORG_TOPIC,
            &tenant_id,
            &project_id,
            &check_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_similarity_check(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        check_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_ORIGINALITY, &tenant_id, &project_id, &check_id),
            KIND_ORIGINALITY,
        )
    }
}

// Shared helpers.

fn zero_hash(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0u8; 32])
}

fn require_nonzero(env: &Env, value: &BytesN<32>) -> Result<(), ContractError> {
    if value == &zero_hash(env) {
        Err(ContractError::InvalidCommitment)
    } else {
        Ok(())
    }
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
    actor: &Address,
    tenant_id: &BytesN<32>,
    role: u32,
) -> Result<(), ContractError> {
    actor.require_auth();
    require_nonzero(env, tenant_id)?;
    if env
        .storage()
        .persistent()
        .has(&DataKey::Role(tenant_id.clone(), actor.clone(), role))
    {
        Ok(())
    } else {
        Err(ContractError::Unauthorized)
    }
}

fn record_key(
    kind: u32,
    tenant_id: &BytesN<32>,
    project_id: &BytesN<32>,
    id: &BytesN<32>,
) -> DataKey {
    DataKey::Record(kind, tenant_id.clone(), project_id.clone(), id.clone())
}

fn extend(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

fn save_record(env: &Env, key: &DataKey, record: &CapstoneRecord) {
    env.storage().persistent().set(key, record);
    extend(env, key);
}

fn load_record(env: &Env, key: &DataKey, kind: u32) -> Result<CapstoneRecord, ContractError> {
    let record: CapstoneRecord = env
        .storage()
        .persistent()
        .get(key)
        .ok_or(ContractError::NotFound)?;
    if record.kind != kind {
        return Err(ContractError::WrongKind);
    }
    extend(env, key);
    Ok(record)
}

fn require_live(record: &CapstoneRecord, now: u64) -> Result<(), ContractError> {
    if record.expires_at != 0 && now >= record.expires_at {
        return Err(ContractError::Expired);
    }
    Ok(())
}

fn next_version(current: u32, expected: u32) -> Result<u32, ContractError> {
    if current != expected {
        return Err(ContractError::StaleVersion);
    }
    current
        .checked_add(1)
        .ok_or(ContractError::ArithmeticOverflow)
}

/// `Some(version)` means the request was already applied; `None` means new.
fn begin_request(
    env: &Env,
    tenant_id: &BytesN<32>,
    project_id: &BytesN<32>,
    record_id: &BytesN<32>,
    input: &CapstoneInput,
) -> Result<Option<u32>, ContractError> {
    require_nonzero(env, &input.request_id)?;
    require_nonzero(env, &input.input_hash)?;
    let key = DataKey::Request(
        tenant_id.clone(),
        project_id.clone(),
        input.request_id.clone(),
    );
    match env.storage().temporary().get::<_, RequestReceipt>(&key) {
        Some(receipt) => {
            if receipt.input_hash != input.input_hash || &receipt.record_id != record_id {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .temporary()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            Ok(Some(receipt.version))
        }
        None => Ok(None),
    }
}

fn finish_request(
    env: &Env,
    tenant_id: &BytesN<32>,
    project_id: &BytesN<32>,
    record_id: &BytesN<32>,
    input: &CapstoneInput,
    version: u32,
) {
    let key = DataKey::Request(
        tenant_id.clone(),
        project_id.clone(),
        input.request_id.clone(),
    );
    env.storage().temporary().set(
        &key,
        &RequestReceipt {
            input_hash: input.input_hash.clone(),
            record_id: record_id.clone(),
            version,
        },
    );
    env.storage()
        .temporary()
        .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
}

fn allowed_output_transition(from: u32, to: u32) -> bool {
    match from {
        OUTPUT_VALID => matches!(to, OUTPUT_REVOKED | OUTPUT_SUPERSEDED | OUTPUT_EMBARGOED),
        OUTPUT_EMBARGOED | OUTPUT_MALFORMED => matches!(to, OUTPUT_VALID | OUTPUT_REVOKED),
        _ => false,
    }
}

fn emit(
    env: &Env,
    topic: Symbol,
    tenant_id: &BytesN<32>,
    project_id: &BytesN<32>,
    id: &BytesN<32>,
    state: u32,
    version: u32,
) {
    env.events().publish(
        (topic,),
        (
            tenant_id.clone(),
            project_id.clone(),
            id.clone(),
            state,
            version,
        ),
    );
}
