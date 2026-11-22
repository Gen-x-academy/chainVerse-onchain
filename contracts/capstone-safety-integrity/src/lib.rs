#![no_std]

//! Privacy-minimized on-chain primitives for capstone research ethics,
//! integrity reports, case triage and protective measures (CAPSTONE-098..101).
//!
//! Protocols, report narratives, case notes and measure reasons stay off-chain;
//! the contract stores only bounded commitments, status codes, counters and
//! timestamps.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_DURATION: u32 = 6_220_800;
const MAX_ROLES: u32 = 32;
const MAX_ROLE_CODE: u32 = 64;
const SECONDS_PER_DAY: u32 = 86_400;

const KIND_SCREENING: u32 = 1;
const KIND_REPORT: u32 = 2;
const KIND_CASE: u32 = 3;
const KIND_MEASURE: u32 = 4;

const ROLE_SUBMITTER: u32 = 1;
const ROLE_REVIEWER: u32 = 2;
const ROLE_RESPONDER: u32 = 3;
const ROLE_OPERATOR: u32 = 4;
const ROLE_AUDITOR: u32 = 5;

const SCREENING_SUBMITTED: u32 = 1;
const SCREENING_APPROVED: u32 = 2;
const SCREENING_REJECTED: u32 = 3;
const SCREENING_CHANGES: u32 = 4;

const REPORT_FILED: u32 = 1;
const REPORT_TRIAGED: u32 = 2;
const REPORT_ESCALATED: u32 = 3;
const REPORT_CLOSED: u32 = 4;

const CASE_NEW: u32 = 1;
const CASE_TRIAGED: u32 = 2;
const CASE_ESCALATED: u32 = 3;
const CASE_CLOSED: u32 = 4;

const MEASURE_ACTIVE: u32 = 1;
const MEASURE_LIFTED: u32 = 2;

const SCR_TOPIC: Symbol = symbol_short!("CAPSCR");
const REP_TOPIC: Symbol = symbol_short!("CAPREP");
const CAS_TOPIC: Symbol = symbol_short!("CAPCAS");
const MEA_TOPIC: Symbol = symbol_short!("CAPMEA");

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
    NoClearance = 15,
    ReportNotTriaged = 16,
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
pub struct CapstoneSafetyIntegrityContract;

#[contractimpl]
impl CapstoneSafetyIntegrityContract {
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

    // CAPSTONE-098 — research ethics screening.

    /// `reference` commits to the protocol, `commitment` to the consent notice,
    /// `aux` to risk acknowledgements. `reason_code` is the pathway (1 exempt,
    /// 2 low, 3 moderate, 4 high); `score_bps` is the category bitmask and
    /// `duration_seconds` bounds clearance validity.
    pub fn submit_screening(
        env: Env,
        submitter: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &submitter, &tenant_id, ROLE_SUBMITTER)?;
        require_nonzero(&env, &screening_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        require_nonzero(&env, &input.aux)?;
        if !(1..=4).contains(&input.reason_code)
            || input.score_bps > 255
            || input.duration_seconds > MAX_DURATION
        {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &screening_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id);
        if let Some(existing) = env.storage().persistent().get::<_, CapstoneRecord>(&key) {
            if existing.state == SCREENING_CHANGES {
                let version = next_version(existing.version, input.expected_version)?;
                let mut record = existing;
                record.state = SCREENING_SUBMITTED;
                record.commitment = input.commitment.clone();
                record.aux = input.aux.clone();
                record.reason_code = input.reason_code;
                record.threshold_bps = input.score_bps;
                record.expires_at = 0;
                record.version = version;
                save_record(&env, &key, &record);
                finish_request(
                    &env,
                    &tenant_id,
                    &project_id,
                    &screening_id,
                    &input,
                    version,
                );
                emit(
                    &env,
                    SCR_TOPIC,
                    &tenant_id,
                    &project_id,
                    &screening_id,
                    record.state,
                    version,
                );
                return Ok(version);
            }
            return Err(ContractError::InvalidState);
        }
        let record = new_record(
            &env,
            KIND_SCREENING,
            SCREENING_SUBMITTED,
            submitter,
            &input,
            None,
        )?;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &screening_id, &input, 1);
        emit(
            &env,
            SCR_TOPIC,
            &tenant_id,
            &project_id,
            &screening_id,
            record.state,
            1,
        );
        Ok(1)
    }

    /// `duration_seconds` on approval is the clearance lifetime. Material
    /// changes invalidate it via `record_material_change`.
    pub fn approve_screening(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        require_nonzero(&env, &input.aux)?;
        if input.duration_seconds == 0 || input.duration_seconds > MAX_DURATION {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &screening_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id);
        let mut record = load_record(&env, &key, KIND_SCREENING)?;
        if record.state != SCREENING_SUBMITTED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = SCREENING_APPROVED;
        record.aux = input.aux.clone();
        record.expires_at = expiry_from(&env, input.duration_seconds)?;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &project_id,
            &screening_id,
            &input,
            version,
        );
        emit(
            &env,
            SCR_TOPIC,
            &tenant_id,
            &project_id,
            &screening_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn reject_screening(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &screening_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id);
        let mut record = load_record(&env, &key, KIND_SCREENING)?;
        if record.state != SCREENING_SUBMITTED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = SCREENING_REJECTED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &project_id,
            &screening_id,
            &input,
            version,
        );
        emit(
            &env,
            SCR_TOPIC,
            &tenant_id,
            &project_id,
            &screening_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn request_screening_changes(
        env: Env,
        reviewer: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reviewer, &tenant_id, ROLE_REVIEWER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &screening_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id);
        let mut record = load_record(&env, &key, KIND_SCREENING)?;
        if record.state != SCREENING_SUBMITTED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = SCREENING_CHANGES;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &project_id,
            &screening_id,
            &input,
            version,
        );
        emit(
            &env,
            SCR_TOPIC,
            &tenant_id,
            &project_id,
            &screening_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// Invalidates an approval after a material change, so activation and
    /// downstream checks must wait for a fresh clearance.
    pub fn record_material_change(
        env: Env,
        submitter: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &submitter, &tenant_id, ROLE_SUBMITTER)?;
        require_nonzero(&env, &input.reference)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &screening_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id);
        let mut record = load_record(&env, &key, KIND_SCREENING)?;
        if record.state != SCREENING_APPROVED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = SCREENING_CHANGES;
        record.commitment = input.reference.clone();
        record.expires_at = 0;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(
            &env,
            &tenant_id,
            &project_id,
            &screening_id,
            &input,
            version,
        );
        emit(
            &env,
            SCR_TOPIC,
            &tenant_id,
            &project_id,
            &screening_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn ethics_clearance_active(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let record = load_record(
            &env,
            &record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id),
            KIND_SCREENING,
        )?;
        Ok(record.state == SCREENING_APPROVED && !is_expired(&record, env.ledger().timestamp()))
    }

    pub fn get_screening(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        screening_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_SCREENING, &tenant_id, &project_id, &screening_id),
            KIND_SCREENING,
        )
    }

    // CAPSTONE-099 — integrity concern reports.

    /// `reference` commits to evidence, `commitment` to safe contact
    /// preferences and `aux` to the concern category. `score_bps` is the
    /// contact preference code and `reason_code` the concern category code.
    pub fn file_report(
        env: Env,
        reporter: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        report_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &reporter, &tenant_id, ROLE_SUBMITTER)?;
        require_nonzero(&env, &report_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        require_nonzero(&env, &input.aux)?;
        if input.reason_code == 0 || !(1..=3).contains(&input.score_bps) {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &report_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_REPORT, &tenant_id, &project_id, &report_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = new_record(&env, KIND_REPORT, REPORT_FILED, reporter, &input, None)?;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &report_id, &input, 1);
        emit(
            &env,
            REP_TOPIC,
            &tenant_id,
            &project_id,
            &report_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn triage_report(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        report_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if !(1..=4).contains(&input.reason_code) {
            return Err(ContractError::InvalidInput);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &report_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_REPORT, &tenant_id, &project_id, &report_id);
        let mut record = load_record(&env, &key, KIND_REPORT)?;
        if record.state != REPORT_FILED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = REPORT_TRIAGED;
        record.reason_code = input.reason_code;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &report_id, &input, version);
        emit(
            &env,
            REP_TOPIC,
            &tenant_id,
            &project_id,
            &report_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn escalate_report(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        report_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &report_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_REPORT, &tenant_id, &project_id, &report_id);
        let mut record = load_record(&env, &key, KIND_REPORT)?;
        if record.state != REPORT_TRIAGED {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = REPORT_ESCALATED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &report_id, &input, version);
        emit(
            &env,
            REP_TOPIC,
            &tenant_id,
            &project_id,
            &report_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn close_report(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        report_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &report_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_REPORT, &tenant_id, &project_id, &report_id);
        let mut record = load_record(&env, &key, KIND_REPORT)?;
        if !matches!(record.state, REPORT_TRIAGED | REPORT_ESCALATED) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = REPORT_CLOSED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &report_id, &input, version);
        emit(
            &env,
            REP_TOPIC,
            &tenant_id,
            &project_id,
            &report_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_report(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        report_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_REPORT, &tenant_id, &project_id, &report_id),
            KIND_REPORT,
        )
    }

    // CAPSTONE-100 — audited case triage and priority.

    /// Input mapping: `reason_code` is severity (1..4), `threshold_bps` is case
    /// age in seconds, `score_bps` is a flags bitmask (1 conflict of interest,
    /// 2 funding impact), and `duration_seconds` is the SLA deadline. Priority
    /// is computed on-chain and stored in `score_bps`.
    pub fn open_case(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &case_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.commitment)?;
        if !(1..=4).contains(&input.reason_code)
            || input.score_bps > 3
            || input.duration_seconds == 0
            || input.duration_seconds > MAX_DURATION
        {
            return Err(ContractError::InvalidInput);
        }
        let report = load_record(
            &env,
            &record_key(KIND_REPORT, &tenant_id, &project_id, &input.reference),
            KIND_REPORT,
        )?;
        if !matches!(report.state, REPORT_TRIAGED | REPORT_ESCALATED) {
            return Err(ContractError::ReportNotTriaged);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &case_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CASE, &tenant_id, &project_id, &case_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let mut record = new_record(
            &env,
            KIND_CASE,
            CASE_NEW,
            responder,
            &input,
            Some(input.duration_seconds),
        )?;
        record.score_bps =
            compute_priority(input.reason_code, input.threshold_bps, input.score_bps);
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &case_id, &input, 1);
        emit(
            &env,
            CAS_TOPIC,
            &tenant_id,
            &project_id,
            &case_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn escalate_case(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &case_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CASE, &tenant_id, &project_id, &case_id);
        let mut record = load_record(&env, &key, KIND_CASE)?;
        if !matches!(record.state, CASE_NEW | CASE_TRIAGED) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = CASE_ESCALATED;
        record.reason_code = input.reason_code;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &case_id, &input, version);
        emit(
            &env,
            CAS_TOPIC,
            &tenant_id,
            &project_id,
            &case_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn close_case(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &case_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CASE, &tenant_id, &project_id, &case_id);
        let mut record = load_record(&env, &key, KIND_CASE)?;
        if !matches!(record.state, CASE_TRIAGED | CASE_ESCALATED) {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = CASE_CLOSED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &case_id, &input, version);
        emit(
            &env,
            CAS_TOPIC,
            &tenant_id,
            &project_id,
            &case_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// Audited access: only the hash of an access note and a counter are kept.
    pub fn log_case_access(
        env: Env,
        auditor: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &auditor, &tenant_id, ROLE_AUDITOR)?;
        require_nonzero(&env, &input.reference)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &case_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CASE, &tenant_id, &project_id, &case_id);
        let mut record = load_record(&env, &key, KIND_CASE)?;
        let version = next_version(record.version, input.expected_version)?;
        record.access_count = record
            .access_count
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        record.aux = input.reference.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &case_id, &input, version);
        emit(
            &env,
            CAS_TOPIC,
            &tenant_id,
            &project_id,
            &case_id,
            record.state,
            version,
        );
        Ok(version)
    }

    /// Moves a newly opened case into triaged state with an assigned owner.
    pub fn mark_case_triaged(
        env: Env,
        responder: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &responder, &tenant_id, ROLE_RESPONDER)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &case_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_CASE, &tenant_id, &project_id, &case_id);
        let mut record = load_record(&env, &key, KIND_CASE)?;
        if record.state != CASE_NEW {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = CASE_TRIAGED;
        record.aux = input.aux.clone();
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &case_id, &input, version);
        emit(
            &env,
            CAS_TOPIC,
            &tenant_id,
            &project_id,
            &case_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn get_case(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        case_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_CASE, &tenant_id, &project_id, &case_id),
            KIND_CASE,
        )
    }

    // CAPSTONE-101 — time-bound protective measures.

    /// `reference` links a triaged case, `aux` is the reason commitment and
    /// `reason_code` is the scope: 1 publishing, 2 messaging, 3 file access,
    /// 4 funding, 5 review, 6 participation.
    pub fn apply_measure(
        env: Env,
        operator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        measure_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &operator, &tenant_id, ROLE_OPERATOR)?;
        require_nonzero(&env, &measure_id)?;
        require_nonzero(&env, &input.reference)?;
        require_nonzero(&env, &input.aux)?;
        if !(1..=6).contains(&input.reason_code)
            || input.duration_seconds == 0
            || input.duration_seconds > MAX_DURATION
        {
            return Err(ContractError::InvalidInput);
        }
        let case = load_record(
            &env,
            &record_key(KIND_CASE, &tenant_id, &project_id, &input.reference),
            KIND_CASE,
        )?;
        if case.state == CASE_NEW {
            return Err(ContractError::InvalidState);
        }
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &measure_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_MEASURE, &tenant_id, &project_id, &measure_id);
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        let record = new_record(
            &env,
            KIND_MEASURE,
            MEASURE_ACTIVE,
            operator,
            &input,
            Some(input.duration_seconds),
        )?;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &measure_id, &input, 1);
        emit(
            &env,
            MEA_TOPIC,
            &tenant_id,
            &project_id,
            &measure_id,
            record.state,
            1,
        );
        Ok(1)
    }

    pub fn lift_measure(
        env: Env,
        operator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        measure_id: BytesN<32>,
        input: CapstoneInput,
    ) -> Result<u32, ContractError> {
        require_role(&env, &operator, &tenant_id, ROLE_OPERATOR)?;
        require_nonzero(&env, &input.aux)?;
        if let Some(v) = begin_request(&env, &tenant_id, &project_id, &measure_id, &input)? {
            return Ok(v);
        }
        let key = record_key(KIND_MEASURE, &tenant_id, &project_id, &measure_id);
        let mut record = load_record(&env, &key, KIND_MEASURE)?;
        if record.state != MEASURE_ACTIVE {
            return Err(ContractError::InvalidState);
        }
        let version = next_version(record.version, input.expected_version)?;
        record.state = MEASURE_LIFTED;
        record.aux = input.aux.clone();
        record.expires_at = 0;
        record.version = version;
        save_record(&env, &key, &record);
        finish_request(&env, &tenant_id, &project_id, &measure_id, &input, version);
        emit(
            &env,
            MEA_TOPIC,
            &tenant_id,
            &project_id,
            &measure_id,
            record.state,
            version,
        );
        Ok(version)
    }

    pub fn is_measure_active(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        measure_id: BytesN<32>,
    ) -> Result<bool, ContractError> {
        let record = load_record(
            &env,
            &record_key(KIND_MEASURE, &tenant_id, &project_id, &measure_id),
            KIND_MEASURE,
        )?;
        Ok(record.state == MEASURE_ACTIVE && !is_expired(&record, env.ledger().timestamp()))
    }

    pub fn get_measure(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        measure_id: BytesN<32>,
    ) -> Result<CapstoneRecord, ContractError> {
        load_record(
            &env,
            &record_key(KIND_MEASURE, &tenant_id, &project_id, &measure_id),
            KIND_MEASURE,
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

fn next_version(current: u32, expected: u32) -> Result<u32, ContractError> {
    if current != expected {
        return Err(ContractError::StaleVersion);
    }
    current
        .checked_add(1)
        .ok_or(ContractError::ArithmeticOverflow)
}

fn compute_priority(severity: u32, age_seconds: u32, flags: u32) -> u32 {
    let age_days = age_seconds / SECONDS_PER_DAY;
    let age_bonus = if age_days > 30 { 30 } else { age_days };
    let mut priority = severity * 20 + age_bonus;
    if flags & 1 != 0 {
        priority += 20;
    }
    if flags & 2 != 0 {
        priority += 15;
    }
    if priority > 100 {
        100
    } else {
        priority
    }
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
