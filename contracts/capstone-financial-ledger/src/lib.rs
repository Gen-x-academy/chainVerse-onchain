#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_PERIODS: u32 = 16;
const MAX_ENTRIES_PER_PERIOD: u32 = 32;
const MAX_ROLES: u32 = 16;
const MAX_REQUESTS: u32 = 128;
const MAX_TOLERANCE: i128 = 1_000_000;

const OP_OPEN: u32 = 1;
const OP_ENTRY: u32 = 2;
const OP_MATCH: u32 = 3;
const OP_RESOLVE: u32 = 4;
const OP_CLOSE: u32 = 5;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    NotFound = 4,
    AlreadyExists = 5,
    InvalidPeriod = 6,
    InvalidEntry = 7,
    InvalidState = 8,
    StaleVersion = 9,
    RequestConflict = 10,
    ArithmeticOverflow = 11,
    IndexFull = 12,
    RoleLimitReached = 13,
    RoleAlreadyGranted = 14,
    MismatchOpen = 15,
    InvalidTolerance = 16,
    CounterpartNotFound = 17,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Coordinator = 1,
    Finance = 2,
    Auditor = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Intent = 1,
    Reservation = 2,
    Hold = 3,
    Release = 4,
    Refund = 5,
    Reimbursement = 6,
    Fee = 7,
    External = 8,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryState {
    Pending = 1,
    Matched = 2,
    Mismatch = 3,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeriodState {
    Open = 1,
    Closed = 2,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    RoleIndex(BytesN<32>, BytesN<32>),
    RequestReceipt(BytesN<32>, BytesN<32>, BytesN<32>),
    RequestIndex(BytesN<32>, BytesN<32>),
    Period(BytesN<32>, BytesN<32>, BytesN<32>),
    PeriodIndex(BytesN<32>, BytesN<32>),
    Entry(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
    EntryIndex(BytesN<32>, BytesN<32>, BytesN<32>),
    Match(BytesN<32>, BytesN<32>, BytesN<32>, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGrant {
    pub account: Address,
    pub role: Role,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestReceipt {
    pub result_id: BytesN<32>,
    pub version: u32,
    pub op: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationPeriod {
    pub period_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub starts_at: u64,
    pub ends_at: u64,
    pub entry_limit: u32,
    pub entry_count: u32,
    pub state: PeriodState,
    pub attestation_hash: BytesN<32>,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    pub entry_id: BytesN<32>,
    pub period_id: BytesN<32>,
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub kind: EntryKind,
    pub amount: i128,
    pub reference_hash: BytesN<32>,
    pub recorded_at: u64,
    pub counterpart_id: BytesN<32>,
    pub delta: i128,
    pub state: EntryState,
    pub version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchRecord {
    pub entry_id: BytesN<32>,
    pub counterpart_id: BytesN<32>,
    pub delta: i128,
    pub reason_code: u32,
    pub resolution_hash: BytesN<32>,
    pub state: EntryState,
    pub version: u32,
}

#[contract]
pub struct CapstoneFinancialLedger;

#[contractimpl]
impl CapstoneFinancialLedger {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn grant_role(
        env: Env,
        admin: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> Result<(), ContractError> {
        Self::require_platform_admin(&env, &admin)?;
        let key = DataKey::RoleIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<RoleGrant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        if index.len() >= MAX_ROLES {
            return Err(ContractError::RoleLimitReached);
        }
        for i in 0..index.len() {
            let grant = index.get(i).unwrap();
            if grant.account == account.clone() && grant.role == role {
                return Err(ContractError::RoleAlreadyGranted);
            }
        }
        index.push_back(RoleGrant {
            account: account.clone(),
            role,
        });
        env.storage().persistent().set(&key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("ROLEGRT"),),
            (tenant_id, project_id, role as u32),
        );
        Ok(())
    }

    pub fn has_role(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        account: Address,
        role: Role,
    ) -> bool {
        Self::role_granted(&env, &tenant_id, &project_id, &account, role)
    }

    pub fn open_period(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        starts_at: u64,
        ends_at: u64,
        entry_limit: u32,
    ) -> Result<ReconciliationPeriod, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_OPEN,
            expected_version,
        )? {
            return Self::load_period(&env, &tenant_id, &project_id, &result_id);
        }
        let now = env.ledger().timestamp();
        if starts_at >= ends_at || ends_at <= now {
            return Err(ContractError::InvalidPeriod);
        }
        if entry_limit == 0 || entry_limit > MAX_ENTRIES_PER_PERIOD {
            return Err(ContractError::InvalidPeriod);
        }
        let key = DataKey::Period(
            tenant_id.clone(),
            project_id.clone(),
            period_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::PeriodIndex(tenant_id.clone(), project_id.clone()),
            &period_id,
            MAX_PERIODS,
        )?;
        let period = ReconciliationPeriod {
            period_id: period_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            starts_at,
            ends_at,
            entry_limit,
            entry_count: 0,
            state: PeriodState::Open,
            attestation_hash: Self::zero_hash(&env),
            version: expected_version
                .checked_add(1)
                .ok_or(ContractError::ArithmeticOverflow)?,
        };
        Self::write_period(&env, &period);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &period_id,
            OP_OPEN,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("LEDOPEN"),),
            (period_id, entry_limit, period.state as u32),
        );
        Ok(period)
    }

    pub fn record_entry(
        env: Env,
        finance: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        entry_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        kind: EntryKind,
        amount: i128,
        reference_hash: BytesN<32>,
    ) -> Result<LedgerEntry, ContractError> {
        Self::require_role(&env, &finance, &tenant_id, &project_id, Role::Finance)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_ENTRY,
            expected_version,
        )? {
            return Self::load_entry(&env, &tenant_id, &project_id, &period_id, &result_id);
        }
        let mut period =
            Self::load_period(&env, &tenant_id, &project_id, &period_id)?;
        if period.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if period.state != PeriodState::Open {
            return Err(ContractError::InvalidState);
        }
        if amount <= 0 || reference_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidEntry);
        }
        let entry_count = period
            .entry_count
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        if entry_count > period.entry_limit {
            return Err(ContractError::IndexFull);
        }
        let key = DataKey::Entry(
            tenant_id.clone(),
            project_id.clone(),
            period_id.clone(),
            entry_id.clone(),
        );
        if env.storage().persistent().has(&key) {
            return Err(ContractError::AlreadyExists);
        }
        Self::push_index(
            &env,
            &DataKey::EntryIndex(
                tenant_id.clone(),
                project_id.clone(),
                period_id.clone(),
            ),
            &entry_id,
            period.entry_limit,
        )?;
        let version = period
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let entry = LedgerEntry {
            entry_id: entry_id.clone(),
            period_id: period_id.clone(),
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            kind,
            amount,
            reference_hash,
            recorded_at: env.ledger().timestamp(),
            counterpart_id: Self::zero_hash(&env),
            delta: 0,
            state: EntryState::Pending,
            version,
        };
        Self::write_entry(&env, &entry);
        period.entry_count = entry_count;
        period.version = version;
        Self::write_period(&env, &period);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &entry_id,
            OP_ENTRY,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("LEDENT"),),
            (entry_id, kind as u32, amount),
        );
        Ok(entry)
    }

    pub fn match_entry(
        env: Env,
        finance: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        entry_id: BytesN<32>,
        counterpart_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        tolerance: i128,
    ) -> Result<MatchRecord, ContractError> {
        Self::require_role(&env, &finance, &tenant_id, &project_id, Role::Finance)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_MATCH,
            expected_version,
        )? {
            return Self::load_match(&env, &tenant_id, &project_id, &period_id, &result_id);
        }
        let mut period =
            Self::load_period(&env, &tenant_id, &project_id, &period_id)?;
        if period.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if period.state != PeriodState::Open {
            return Err(ContractError::InvalidState);
        }
        if tolerance < 0 || tolerance > MAX_TOLERANCE {
            return Err(ContractError::InvalidTolerance);
        }
        let mut entry =
            Self::load_entry(&env, &tenant_id, &project_id, &period_id, &entry_id)?;
        let mut counterpart = Self::load_entry(
            &env,
            &tenant_id,
            &project_id,
            &period_id,
            &counterpart_id,
        )
        .map_err(|_| ContractError::CounterpartNotFound)?;
        if entry.state != EntryState::Pending || counterpart.state != EntryState::Pending {
            return Err(ContractError::InvalidState);
        }
        let delta = entry
            .amount
            .checked_sub(counterpart.amount)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let magnitude = delta
            .checked_abs()
            .ok_or(ContractError::ArithmeticOverflow)?;
        let state = if magnitude <= tolerance {
            EntryState::Matched
        } else {
            EntryState::Mismatch
        };
        let reason_code = if state == EntryState::Mismatch { 1u32 } else { 0u32 };
        let version = period
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        let record = MatchRecord {
            entry_id: entry_id.clone(),
            counterpart_id: counterpart_id.clone(),
            delta,
            reason_code,
            resolution_hash: Self::zero_hash(&env),
            state,
            version,
        };
        Self::write_match(&env, &tenant_id, &project_id, &period_id, &record);
        entry.state = state;
        entry.counterpart_id = counterpart_id.clone();
        entry.delta = delta;
        entry.version = version;
        counterpart.state = state;
        counterpart.counterpart_id = entry_id.clone();
        counterpart.delta = -delta;
        counterpart.version = version;
        Self::write_entry(&env, &entry);
        Self::write_entry(&env, &counterpart);
        period.version = version;
        Self::write_period(&env, &period);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &entry_id,
            OP_MATCH,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("LEDMATCH"),),
            (entry_id, counterpart_id, delta, state as u32),
        );
        Ok(record)
    }

    pub fn resolve_mismatch(
        env: Env,
        finance: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        entry_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        resolution_hash: BytesN<32>,
    ) -> Result<MatchRecord, ContractError> {
        Self::require_role(&env, &finance, &tenant_id, &project_id, Role::Finance)?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_RESOLVE,
            expected_version,
        )? {
            return Self::load_match(&env, &tenant_id, &project_id, &period_id, &result_id);
        }
        let mut period =
            Self::load_period(&env, &tenant_id, &project_id, &period_id)?;
        if period.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        let mut record =
            Self::load_match(&env, &tenant_id, &project_id, &period_id, &entry_id)?;
        if record.state != EntryState::Mismatch {
            return Err(ContractError::InvalidState);
        }
        if resolution_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidEntry);
        }
        let mut entry =
            Self::load_entry(&env, &tenant_id, &project_id, &period_id, &entry_id)?;
        let mut counterpart = Self::load_entry(
            &env,
            &tenant_id,
            &project_id,
            &period_id,
            &record.counterpart_id,
        )?;
        let version = period
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        record.state = EntryState::Matched;
        record.reason_code = 0;
        record.resolution_hash = resolution_hash.clone();
        record.version = version;
        entry.state = EntryState::Matched;
        entry.version = version;
        counterpart.state = EntryState::Matched;
        counterpart.version = version;
        Self::write_match(&env, &tenant_id, &project_id, &period_id, &record);
        Self::write_entry(&env, &entry);
        Self::write_entry(&env, &counterpart);
        period.version = version;
        Self::write_period(&env, &period);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &entry_id,
            OP_RESOLVE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("LEDRES"),),
            (entry_id, resolution_hash, record.state as u32),
        );
        Ok(record)
    }

    pub fn close_period(
        env: Env,
        coordinator: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        request_id: BytesN<32>,
        expected_version: u32,
        attestation_hash: BytesN<32>,
    ) -> Result<ReconciliationPeriod, ContractError> {
        Self::require_role(
            &env,
            &coordinator,
            &tenant_id,
            &project_id,
            Role::Coordinator,
        )?;
        if let Some(result_id) = Self::request_result(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            OP_CLOSE,
            expected_version,
        )? {
            return Self::load_period(&env, &tenant_id, &project_id, &result_id);
        }
        let mut period =
            Self::load_period(&env, &tenant_id, &project_id, &period_id)?;
        if period.version != expected_version {
            return Err(ContractError::StaleVersion);
        }
        if period.state != PeriodState::Open {
            return Err(ContractError::InvalidState);
        }
        if attestation_hash == Self::zero_hash(&env) {
            return Err(ContractError::InvalidEntry);
        }
        let index = Self::load_index(
            &env,
            &DataKey::EntryIndex(
                tenant_id.clone(),
                project_id.clone(),
                period_id.clone(),
            ),
        );
        let mut mismatches = 0u32;
        for i in 0..index.len() {
            let entry = Self::load_entry(
                &env,
                &tenant_id,
                &project_id,
                &period_id,
                &index.get(i).unwrap(),
            )?;
            if entry.state == EntryState::Mismatch {
                mismatches = mismatches
                    .checked_add(1)
                    .ok_or(ContractError::ArithmeticOverflow)?;
            }
            if entry.state == EntryState::Pending {
                return Err(ContractError::InvalidState);
            }
        }
        if mismatches > 0 {
            return Err(ContractError::MismatchOpen);
        }
        period.state = PeriodState::Closed;
        period.attestation_hash = attestation_hash;
        period.version = period
            .version
            .checked_add(1)
            .ok_or(ContractError::ArithmeticOverflow)?;
        Self::write_period(&env, &period);
        Self::write_request(
            &env,
            &tenant_id,
            &project_id,
            &request_id,
            &period_id,
            OP_CLOSE,
            expected_version,
        )?;
        env.events().publish(
            (symbol_short!("LEDCLOSE"),),
            (period_id, period.entry_count, period.state as u32),
        );
        Ok(period)
    }

    pub fn get_period(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
    ) -> Result<ReconciliationPeriod, ContractError> {
        Self::load_period(&env, &tenant_id, &project_id, &period_id)
    }

    pub fn get_entry(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        entry_id: BytesN<32>,
    ) -> Result<LedgerEntry, ContractError> {
        Self::load_entry(&env, &tenant_id, &project_id, &period_id, &entry_id)
    }

    pub fn get_match(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
        entry_id: BytesN<32>,
    ) -> Result<MatchRecord, ContractError> {
        Self::load_match(&env, &tenant_id, &project_id, &period_id, &entry_id)
    }

    pub fn period_count(env: Env, tenant_id: BytesN<32>, project_id: BytesN<32>) -> u32 {
        Self::load_index(&env, &DataKey::PeriodIndex(tenant_id, project_id)).len()
    }

    pub fn entry_count(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        period_id: BytesN<32>,
    ) -> u32 {
        Self::load_index(&env, &DataKey::EntryIndex(tenant_id, project_id, period_id)).len()
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn require_platform_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if stored != admin.clone() {
            return Err(ContractError::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn role_granted(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        account: &Address,
        role: Role,
    ) -> bool {
        let key = DataKey::RoleIndex(tenant_id.clone(), project_id.clone());
        let index: Vec<RoleGrant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..index.len() {
            let grant = index.get(i).unwrap();
            if grant.account == account.clone() && grant.role == role {
                return true;
            }
        }
        false
    }

    fn require_role(
        env: &Env,
        caller: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        caller.require_auth();
        if Self::role_granted(env, tenant_id, project_id, caller, role) {
            Ok(())
        } else {
            Err(ContractError::Unauthorized)
        }
    }

    fn request_result(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        op: u32,
        expected_version: u32,
    ) -> Result<Option<BytesN<32>>, ContractError> {
        let key = DataKey::RequestReceipt(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        if let Some(receipt) = env
            .storage()
            .persistent()
            .get::<DataKey, RequestReceipt>(&key)
        {
            if receipt.op != op || receipt.version != expected_version {
                return Err(ContractError::RequestConflict);
            }
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
            return Ok(Some(receipt.result_id));
        }
        Ok(None)
    }

    fn write_request(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        request_id: &BytesN<32>,
        result_id: &BytesN<32>,
        op: u32,
        version: u32,
    ) -> Result<(), ContractError> {
        let key = DataKey::RequestReceipt(
            tenant_id.clone(),
            project_id.clone(),
            request_id.clone(),
        );
        let index_key = DataKey::RequestIndex(tenant_id.clone(), project_id.clone());
        let mut index: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&index_key)
            .unwrap_or_else(|| Vec::new(env));
        if index.len() >= MAX_REQUESTS {
            return Err(ContractError::IndexFull);
        }
        index.push_back(request_id.clone());
        env.storage().persistent().set(&index_key, &index);
        env.storage()
            .persistent()
            .extend_ttl(&index_key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        let receipt = RequestReceipt {
            result_id: result_id.clone(),
            version,
            op,
        };
        env.storage().persistent().set(&key, &receipt);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn load_period(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        period_id: &BytesN<32>,
    ) -> Result<ReconciliationPeriod, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Period(
                tenant_id.clone(),
                project_id.clone(),
                period_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_period(env: &Env, period: &ReconciliationPeriod) {
        let key = DataKey::Period(
            period.tenant_id.clone(),
            period.project_id.clone(),
            period.period_id.clone(),
        );
        env.storage().persistent().set(&key, period);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_entry(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        period_id: &BytesN<32>,
        entry_id: &BytesN<32>,
    ) -> Result<LedgerEntry, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Entry(
                tenant_id.clone(),
                project_id.clone(),
                period_id.clone(),
                entry_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_entry(env: &Env, entry: &LedgerEntry) {
        let key = DataKey::Entry(
            entry.tenant_id.clone(),
            entry.project_id.clone(),
            entry.period_id.clone(),
            entry.entry_id.clone(),
        );
        env.storage().persistent().set(&key, entry);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_match(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        period_id: &BytesN<32>,
        entry_id: &BytesN<32>,
    ) -> Result<MatchRecord, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Match(
                tenant_id.clone(),
                project_id.clone(),
                period_id.clone(),
                entry_id.clone(),
            ))
            .ok_or(ContractError::NotFound)
    }

    fn write_match(
        env: &Env,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        period_id: &BytesN<32>,
        record: &MatchRecord,
    ) {
        let key = DataKey::Match(
            tenant_id.clone(),
            project_id.clone(),
            period_id.clone(),
            record.entry_id.clone(),
        );
        env.storage().persistent().set(&key, record);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn push_index(
        env: &Env,
        key: &DataKey,
        id: &BytesN<32>,
        cap: u32,
    ) -> Result<(), ContractError> {
        let mut index = Self::load_index(env, key);
        if index.len() >= cap {
            return Err(ContractError::IndexFull);
        }
        for i in 0..index.len() {
            if index.get(i).unwrap() == id.clone() {
                return Err(ContractError::AlreadyExists);
            }
        }
        index.push_back(id.clone());
        env.storage().persistent().set(key, &index);
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    fn load_index(env: &Env, key: &DataKey) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get(key)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn zero_hash(env: &Env) -> BytesN<32> {
        BytesN::from_array(env, &[0u8; 32])
    }
}

#[cfg(test)]
mod tests;
