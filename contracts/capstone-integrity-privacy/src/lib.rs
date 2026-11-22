#![no_std]

//! Privacy-minimized on-chain primitives for capstone dispute resolution,
//! participant blocking, evidence retention and consent (CAPSTONE-102..105).
//!
//! Reports, evidence files, contact preferences and consent notices stay
//! off-chain. Only opaque ids, addresses, bounded commitments, status codes,
//! counters and timestamps are stored here.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_DURATION: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_ROLE_CODE: u32 = 64;

const KIND_DISPUTE: u32 = 1;
const KIND_BLOCK: u32 = 2;
const KIND_EVIDENCE: u32 = 3;
const KIND_CONSENT: u32 = 4;

const ROLE_RESPONDENT: u32 = 1;
const ROLE_CUSTODIAN: u32 = 2;
const ROLE_PARTICIPANT: u32 = 3;

const DISPUTE_OPEN: u32 = 1;
const DISPUTE_RESPONDED: u32 = 2;
const DISPUTE_EVIDENCE: u32 = 3;
const DISPUTE_RESOLVED: u32 = 4;
const DISPUTE_APPEALED: u32 = 5;
const DISPUTE_FINAL: u32 = 6;
const DISPUTE_REMEDIED: u32 = 7;

const BLOCK_ACTIVE: u32 = 1;
const BLOCK_WITHDRAWN: u32 = 2;

const EVIDENCE_ACTIVE: u32 = 1;
const EVIDENCE_HOLD: u32 = 2;
const EVIDENCE_DISPOSED: u32 = 3;

const CONSENT_GRANTED: u32 = 1;
const CONSENT_WITHDRAWN: u32 = 2;

const DIS_TOPIC: Symbol = symbol_short!("CAPDSP");
const BLK_TOPIC: Symbol = symbol_short!("CAPBLK");
const RET_TOPIC: Symbol = symbol_short!("CAPRET");
const CON_TOPIC: Symbol = symbol_short!("CAPCON");

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
    LegalHold = 15,
    NoPolicy = 16,
    SelfBlock = 17,
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
    Block(BytesN<32>, Address, Address),
    RetentionPolicy(BytesN<32>, u32),
}

/// Bounded mutation fields bundled to stay within Soroban's 10-parameter ABI.
/// Unused fields must be zero/empty.
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
    pub access_count: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub input_hash: BytesN<32>,
    pub record_id: BytesN<32>,
    pub version: u32,
}

#[contract]
pub struct CapstoneIntegrityPrivacyContract;

#[contractimpl]
impl CapstoneIntegrityPrivacyContract {
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

    // CAPSTONE-102 — project disputes.

    /// `reference` is the claim commitment, `aux` the evidence commitment and
    /// `duration_seconds` the response deadline. No claim text is stored.
    pub fn open_dispute(
        env: Env,
        claimant: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &claimant, &tenant_id, ROLE_PARTICIPANT)?;
        require_nonzero(&env, &dispute_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.aux)?;
        if input.duration_seconds == 0 || input.duration_seconds > MAX_DURATION {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = new_record(
            &env,
            KIND_DISPUTE,
            DISPUTE_OPEN,
            claimant,
            &input,
            Some(input.duration_seconds),
        )?;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, 1);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn respond_to_dispute(
        env: Env,
        respondent: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &respondent, &tenant_id, ROLE_RESPONDENT)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        if record.state != DISPUTE_OPEN {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_RESPONDED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn attach_dispute_evidence(
        env: Env,
        respondent: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &respondent, &tenant_id, ROLE_RESPONDENT)?;
        require_nonzero(&env, &input.reference)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        if !matches!(record.state, DISPUTE_RESPONDED | DISPUTE_EVIDENCE) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_EVIDENCE;
        record.commitment = input.reference.clone();
        record.access_count = record
            .access_count
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// `reason_code` is the outcome code; `aux` is the reasoned decision hash.
    /// `duration_seconds` bounds the appeal window.
    pub fn resolve_dispute(
        env: Env,
        respondent: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &respondent, &tenant_id, ROLE_RESPONDENT)?;
        require_nonzero(&env, &input.aux)?;
        if input.duration_seconds == 0 || input.duration_seconds > MAX_DURATION {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        if !matches!(record.state, DISPUTE_RESPONDED | DISPUTE_EVIDENCE) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_RESOLVED;
        record.reason_code = input.reason_code;
        record.aux = input.aux.clone();
        record.expires_at = env
            .ledger()
            .timestamp()
            .checked_add(input.duration_seconds as u64)
            .ok_or(ContractError::ArithmeticOverflow)?;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn appeal_dispute(
        env: Env,
        claimant: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &claimant, &tenant_id, ROLE_PARTICIPANT)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        require_live(&record, env.ledger().timestamp())?;
        if record.state != DISPUTE_RESOLVED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_APPEALED;
        record.aux = input.aux.clone();
        record.expires_at = 0;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn finalize_appeal(
        env: Env,
        respondent: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &respondent, &tenant_id, ROLE_RESPONDENT)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        if record.state != DISPUTE_APPEALED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_FINAL;
        record.reason_code = input.reason_code;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// Remedies are idempotent: a replayed request returns the same version
    /// and cannot pay or notify twice.
    pub fn apply_remedy(
        env: Env,
        respondent: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &respondent, &tenant_id, ROLE_RESPONDENT)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &dispute_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id);
        let mut record = load_record(&env, &key, KIND_DISPUTE)?;
        if record.state != DISPUTE_FINAL {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = DISPUTE_REMEDIED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &dispute_id, &input, version);
        emit(
            &env,
            DIS_TOPIC,
            &tenant_id,
            &project_id,
            &dispute_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_dispute(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        dispute_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_DISPUTE, &tenant_id, &project_id, &dispute_id),
            KIND_DISPUTE,
        )
    }

    // CAPSTONE-103 — bidirectional participant blocking.

    /// Writes both directions so discovery, invitations, matching, messaging
    /// and collaboration are blocked regardless of who initiates. Existing
    /// safety and financial records are untouched.
    pub fn set_block(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        counterpart: Address,
        pair_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &actor, &tenant_id, ROLE_PARTICIPANT)?;
        require_nonzero(&env, &pair_id)?;
        if actor == counterpart {
            return Err(ContractError::SelfBlock);
        }
        if input.duration_seconds > MAX_DURATION {
            return Err(ContractError::InvalidInput);
        }
        let forward = DataKey::Block(tenant_id.clone(), actor.clone(), counterpart.clone());
        let reverse = DataKey::Block(tenant_id.clone(), counterpart.clone(), actor.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, CapstoneRecord>(&forward)
        {
            if existing.state == BLOCK_ACTIVE && !is_expired(&existing, env.ledger().timestamp()) {
                return Ok(existing.version);
            }
        }
        if let Some(v) = begin_request(&env, &tenant_id, &pair_id, &pair_id, &input)? {
            return Ok(v);
        }
        let expires_at = expiry_from(&env, input.duration_seconds)?;
        let record = CapstoneRecord {
            kind: KIND_BLOCK,
            state: BLOCK_ACTIVE,
            subject: actor.clone(),
            reference: pair_id.clone(),
            commitment: input.reference.clone(),
            aux: input.aux.clone(),
            score_bps: 0,
            threshold_bps: 0,
            reason_code: input.reason_code,
            created_at: env.ledger().timestamp(),
            expires_at,
            version: 1,
            access_count: 0,
        };
        let mut mirrored = record.clone();
        mirrored.subject = counterpart.clone();
        env.storage().persistent().set(&forward, &record);
        env.storage().persistent().set(&reverse, &mirrored);
        extend(&env, &forward);
        extend(&env, &reverse);
        finish_request(&env, &tenant_id, &pair_id, &pair_id, &input, 1);
        env.events().publish(
            (BLK_TOPIC,),
            (tenant_id.clone(), pair_id.clone(), BLOCK_ACTIVE, 1u32),
        );
        Ok(1)
    }

    pub fn withdraw_block(
        env: Env,
        actor: Address,
        tenant_id: BytesN<32>,
        counterpart: Address,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &actor, &tenant_id, ROLE_PARTICIPANT)?;
        if let Some(v) = begin_request(&env, &tenant_id, &tenant_id, &tenant_id, &input)? {
            return Ok(v);
        }
        let forward = DataKey::Block(tenant_id.clone(), actor.clone(), counterpart.clone());
        let reverse = DataKey::Block(tenant_id.clone(), counterpart.clone(), actor.clone());
        let mut record: CapstoneRecord = env
            .storage()
            .persistent()
            .get(&forward)
            .ok_or(ContractError::NotFound)?;
        if record.state != BLOCK_ACTIVE {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = BLOCK_WITHDRAWN;
        record.version = version;
        let mut mirrored = record.clone();
        mirrored.subject = counterpart;
        env.storage().persistent().set(&forward, &record);
        env.storage().persistent().set(&reverse, &mirrored);
        extend(&env, &forward);
        extend(&env, &reverse);
        finish_request(&env, &tenant_id, &tenant_id, &tenant_id, &input, version);
        env.events().publish(
            (BLK_TOPIC,),
            (tenant_id, record.reference, BLOCK_WITHDRAWN, version),
        );
        Ok(version)
    }

    pub fn is_blocked(env: Env, tenant_id: BytesN<32>, a: Address, b: Address) -> bool {
        let key = DataKey::Block(tenant_id, a, b);
        match env.storage().persistent().get::<_, CapstoneRecord>(&key) {
            Some(record) => {
                record.state == BLOCK_ACTIVE && !is_expired(&record, env.ledger().timestamp())
            }
            None => false,
        }
    }

    pub fn get_block(
        env: Env,
        tenant_id: BytesN<32>,
        a: Address,
        b: Address,
    ) -> Result<CapstoneRecord, ContractError> {
        let key = DataKey::Block(tenant_id, a, b);
        let record: CapstoneRecord = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::NotFound)?;
        extend(&env, &key);
        Ok(record)
    }

    // CAPSTONE-104 — category-specific evidence retention.

    /// `reason_code` is the evidence category; `duration_seconds` is the
    /// category retention period (bounded). `aux` is the policy hash.
    pub fn set_retention_policy(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<(), ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        require_nonzero(&env, &input.aux)?;
        if input.duration_seconds == 0
            || input.duration_seconds > MAX_DURATION
            || input.reason_code == 0
        {
            return Err(ContractError::InvalidInput);
        }
        let key = DataKey::RetentionPolicy(tenant_id.clone(), input.reason_code);
        env.storage()
            .persistent()
            .set(&key, &input.duration_seconds);
        extend(&env, &key);
        env.events().publish(
            (RET_TOPIC,),
            (tenant_id, input.reason_code, input.duration_seconds),
        );
        Ok(())
    }

    pub fn get_retention_policy(env: Env, tenant_id: BytesN<32>, category: u32) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::RetentionPolicy(tenant_id, category))
            .unwrap_or(0)
    }

    pub fn attach_evidence(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        require_nonzero(&env, &evidence_id)?;
        require_nonzero(&env, &input.reference)?;
        let retention: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::RetentionPolicy(
                tenant_id.clone(),
                input.reason_code,
            ))
            .ok_or(ContractError::NoPolicy)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &evidence_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = new_record(
            &env,
            KIND_EVIDENCE,
            EVIDENCE_ACTIVE,
            custodian,
            &input,
            Some(retention),
        )?;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &evidence_id, &input, 1);
        emit(
            &env,
            RET_TOPIC,
            &tenant_id,
            &project_id,
            &evidence_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn place_legal_hold(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &evidence_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id);
        let mut record = load_record(&env, &key, KIND_EVIDENCE)?;
        if record.state != EVIDENCE_ACTIVE {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = EVIDENCE_HOLD;
        record.expires_at = 0;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &evidence_id, &input, version);
        emit(
            &env,
            RET_TOPIC,
            &tenant_id,
            &project_id,
            &evidence_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn release_legal_hold(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &evidence_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id);
        let mut record = load_record(&env, &key, KIND_EVIDENCE)?;
        if record.state != EVIDENCE_HOLD {
            return Err(ContractError::InvalidState);
        }
        let retention: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::RetentionPolicy(
                tenant_id.clone(),
                record.reason_code,
            ))
            .ok_or(ContractError::NoPolicy)?;
        let version = next_version(record.version, input.expected_version)?;
        record.state = EVIDENCE_ACTIVE;
        record.expires_at = expiry_from(&env, retention)?;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &evidence_id, &input, version);
        emit(
            &env,
            RET_TOPIC,
            &tenant_id,
            &project_id,
            &evidence_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn log_evidence_access(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        require_nonzero(&env, &input.reference)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &evidence_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id);
        let mut record = load_record(&env, &key, KIND_EVIDENCE)?;
        if record.state == EVIDENCE_DISPOSED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.access_count = record
            .access_count
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        record.aux = input.reference.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &evidence_id, &input, version);
        emit(
            &env,
            RET_TOPIC,
            &tenant_id,
            &project_id,
            &evidence_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn dispose_evidence(
        env: Env,
        custodian: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &custodian, &tenant_id, ROLE_CUSTODIAN)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &evidence_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id);
        let mut record = load_record(&env, &key, KIND_EVIDENCE)?;
        if record.state == EVIDENCE_HOLD {
            return Err(ContractError::LegalHold);
        }
        if record.state != EVIDENCE_ACTIVE || !is_expired(&record, env.ledger().timestamp()) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = EVIDENCE_DISPOSED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &evidence_id, &input, version);
        emit(
            &env,
            RET_TOPIC,
            &tenant_id,
            &project_id,
            &evidence_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_evidence(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        evidence_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_EVIDENCE, &tenant_id, &project_id, &evidence_id),
            KIND_EVIDENCE,
        )
    }

    // CAPSTONE-105 — purpose-specific consent ledger.

    /// `subject_ref` is a pseudonymous off-chain reference; `reference` is the
    /// policy hash and `aux` a notice commitment. `reason_code` is the purpose:
    /// 1 matching, 2 collaboration, 3 provider access, 4 integrity checks,
    /// 5 analytics, 6 publication, 7 credentials.
    pub fn grant_consent(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        subject_ref: BytesN<32>,
        purpose: u32,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &subject, &tenant_id, ROLE_PARTICIPANT)?;
        require_nonzero(&env, &subject_ref)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.aux)?;
        if !(1..=7).contains(&purpose) || input.duration_seconds > MAX_DURATION {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &subject_ref, &subject_ref, &input)? {
            return Ok(v);
        }
        let id = consent_id(&subject_ref, purpose);
        let key = record_key(KIND_CONSENT, &tenant_id, &subject_ref, &id);
        if let Some(existing) = env.storage().persistent().get::<_, CapstoneRecord>(&key) {
            if existing.state == CONSENT_GRANTED && !is_expired(&existing, env.ledger().timestamp())
            {
                return Ok(existing.version);
            }
        }
        let mut input_with_purpose = input.clone();
        input_with_purpose.reason_code = purpose;
        let record = new_record(
            &env,
            KIND_CONSENT,
            CONSENT_GRANTED,
            subject,
            &input_with_purpose,
            Some(input.duration_seconds),
        )?;
        let version = record.version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &subject_ref,
            &subject_ref,
            &input,
            version,
        );
        env.events().publish(
            (CON_TOPIC,),
            (tenant_id, subject_ref, purpose, CONSENT_GRANTED, version),
        );
        Ok(version)
    }

    pub fn withdraw_consent(
        env: Env,
        subject: Address,
        tenant_id: BytesN<32>,
        subject_ref: BytesN<32>,
        purpose: u32,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &subject, &tenant_id, ROLE_PARTICIPANT)?;
        if let Some(v) = begin_request(&env, &tenant_id, &subject_ref, &subject_ref, &input)? {
            return Ok(v);
        }
        let id = consent_id(&subject_ref, purpose);
        let key = record_key(KIND_CONSENT, &tenant_id, &subject_ref, &id);
        let mut record = load_record(&env, &key, KIND_CONSENT)?;
        if record.state != CONSENT_GRANTED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = CONSENT_WITHDRAWN;
        record.expires_at = 0;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &subject_ref,
            &subject_ref,
            &input,
            version,
        );
        env.events().publish(
            (CON_TOPIC,),
            (tenant_id, subject_ref, purpose, CONSENT_WITHDRAWN, version),
        );
        Ok(version)
    }

    pub fn has_consent(
        env: Env,
        tenant_id: BytesN<32>,
        subject_ref: BytesN<32>,
        purpose: u32,
    ) -> bool {
        let id = consent_id(&subject_ref, purpose);
        let key = record_key(KIND_CONSENT, &tenant_id, &subject_ref, &id);
        match env.storage().persistent().get::<_, CapstoneRecord>(&key) {
            Some(record) => {
                record.state == CONSENT_GRANTED && !is_expired(&record, env.ledger().timestamp())
            }
            None => false,
        }
    }

    pub fn get_consent(
        env: Env,
        tenant_id: BytesN<32>,
        subject_ref: BytesN<32>,
        purpose: u32,
    ) -> Result<CapstoneRecord, ContractError> {
        let id = consent_id(&subject_ref, purpose);
        load_record(
            &env,
            &record_key(KIND_CONSENT, &tenant_id, &subject_ref, &id),
            KIND_CONSENT,
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

fn new_record(
    env: &Env,
    kind: u32,
    state: u32,
    subject: Address,
    input: &CapstoneInput,
    duration: Option<u32>,
) -> Result<CapstoneRecord, ContractError> {
    Ok(CapstoneRecord {
        kind,
        state,
        subject,
        reference: input.reference.clone(),
        commitment: input.commitment.clone(),
        aux: input.aux.clone(),
        score_bps: input.score_bps,
        threshold_bps: input.threshold_bps,
        reason_code: input.reason_code,
        created_at: env.ledger().timestamp(),
        expires_at: expiry_from(env, duration.unwrap_or(0))?,
        version: 1,
        access_count: 0,
    })
}

fn expiry_from(env: &Env, duration_seconds: u32) -> Result<u64, ContractError> {
    if duration_seconds == 0 {
        return Ok(0);
    }
    if duration_seconds > MAX_DURATION {
        return Err(ContractError::InvalidInput);
    }
    env.ledger()
        .timestamp()
        .checked_add(duration_seconds as u64)
        .ok_or(ContractError::ArithmeticOverflow)
}

fn is_expired(record: &CapstoneRecord, now: u64) -> bool {
    record.expires_at != 0 && now >= record.expires_at
}

fn require_live(record: &CapstoneRecord, now: u64) -> Result<(), ContractError> {
    if is_expired(record, now) {
        Err(ContractError::Expired)
    } else {
        Ok(())
    }
}

fn next_version(current: u32, expected: u32) -> Result<u32, ContractError> {
    if current != expected {
        return Err(ContractError::StaleVersion);
    }
    current
        .checked_add(1)
        .ok_or(ContractError::ArithmeticOverflow)
}

fn consent_id(subject_ref: &BytesN<32>, purpose: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[..28].copy_from_slice(&subject_ref.to_array()[..28]);
    bytes[28..].copy_from_slice(&purpose.to_be_bytes());
    BytesN::from_array(subject_ref.env(), &bytes)
}

/// `Some(version)` means the request was already applied; `None` means new.
fn begin_request(
    env: &Env,
    tenant_id: &BytesN<32>,
    scope_id: &BytesN<32>,
    record_id: &BytesN<32>,
    input: &CapstoneInput,
) -> Result<Option<u32>, ContractError> {
    require_nonzero(env, &input.request_id)?;
    require_nonzero(env, &input.input_hash)?;
    let key = DataKey::Request(
        tenant_id.clone(),
        scope_id.clone(),
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
    scope_id: &BytesN<32>,
    record_id: &BytesN<32>,
    input: &CapstoneInput,
    version: u32,
) {
    let key = DataKey::Request(
        tenant_id.clone(),
        scope_id.clone(),
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
