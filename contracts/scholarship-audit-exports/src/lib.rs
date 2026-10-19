#![no_std]

//! Scoped, privacy-minimized evidence export manifests for scholarship audits.
//!
//! Export files are assembled and retained off-chain. This contract records
//! only their fixed-size integrity commitments, program scope, covered period,
//! and publisher. Sponsors publish exports; explicitly authorized regulators
//! can read exports only for the programs to which they were granted access.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const MAX_EXPORTS_PER_PROGRAM: u32 = 64;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    ProgramNotFound = 4,
    ProgramAlreadyExists = 5,
    NotProgramSponsor = 6,
    Unauthorized = 7,
    InvalidPeriod = 8,
    InvalidEvidenceHash = 9,
    ExportIdConflict = 10,
    ExportNotFound = 11,
    ExportLimitReached = 12,
    CounterOverflow = 13,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Program(BytesN<32>),
    Regulator(BytesN<32>, Address),
    Export(BytesN<32>, BytesN<32>),
    ExportCount(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub sponsor: Address,
}

/// Category commitments let an auditor review package sections independently.
/// Underlying records and any personal data stay in the off-chain evidence store.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceHashes {
    pub governance: BytesN<32>,
    pub decisions: BytesN<32>,
    pub consent: BytesN<32>,
    pub funds: BytesN<32>,
    pub payments: BytesN<32>,
}

/// Immutable commitment to one canonical off-chain evidence package.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditExport {
    pub export_id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub period_start: u64,
    pub period_end: u64,
    pub generated_at: u64,
    pub generated_by: Address,
    pub schema_version: u32,
    pub package_hash: BytesN<32>,
    pub source_checkpoint_hash: BytesN<32>,
    pub evidence_hashes: EvidenceHashes,
}

#[contract]
pub struct ScholarshipAuditExportsContract;

#[contractimpl]
impl ScholarshipAuditExportsContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        if Self::admin(env)? != *caller {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    fn extend_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        sponsor: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }
        env.storage()
            .persistent()
            .set(&key, &Program { sponsor: sponsor.clone() });
        Self::extend_ttl(&env, &key);
        env.events()
            .publish((symbol_short!("AUDPROG"),), (program_id, sponsor));
        Ok(())
    }

    /// A program sponsor or platform admin grants a regulator scoped access.
    pub fn grant_regulator(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        regulator: Address,
    ) -> Result<(), ContractError> {
        let program_key = DataKey::Program(program_id.clone());
        let program: Program = env
            .storage()
            .persistent()
            .get(&program_key)
            .ok_or(ContractError::ProgramNotFound)?;
        if caller != program.sponsor && caller != Self::admin(&env)? {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::Regulator(program_id.clone(), regulator.clone());
        env.storage().persistent().set(&key, &true);
        Self::extend_ttl(&env, &key);
        Self::extend_ttl(&env, &program_key);
        env.events().publish(
            (symbol_short!("AUDGRNT"),),
            (program_id, regulator),
        );
        Ok(())
    }

    pub fn revoke_regulator(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        regulator: Address,
    ) -> Result<(), ContractError> {
        let program_key = DataKey::Program(program_id.clone());
        let program: Program = env
            .storage()
            .persistent()
            .get(&program_key)
            .ok_or(ContractError::ProgramNotFound)?;
        if caller != program.sponsor && caller != Self::admin(&env)? {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();

        let key = DataKey::Regulator(program_id.clone(), regulator.clone());
        env.storage().persistent().set(&key, &false);
        Self::extend_ttl(&env, &key);
        env.events().publish(
            (symbol_short!("AUDRVK"),),
            (program_id, regulator),
        );
        Ok(())
    }

    /// Sponsor/admin publishes an immutable commitment. Exact retries return
    /// success without a second write/event; reusing an ID for other evidence
    /// returns `ExportIdConflict`.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_export(
        env: Env,
        caller: Address,
        export_id: BytesN<32>,
        program_id: BytesN<32>,
        period_start: u64,
        period_end: u64,
        package_hash: BytesN<32>,
        source_checkpoint_hash: BytesN<32>,
        evidence_hashes: EvidenceHashes,
    ) -> Result<(), ContractError> {
        let program_key = DataKey::Program(program_id.clone());
        let program: Program = env
            .storage()
            .persistent()
            .get(&program_key)
            .ok_or(ContractError::ProgramNotFound)?;
        if caller != program.sponsor && caller != Self::admin(&env)? {
            return Err(ContractError::NotProgramSponsor);
        }
        caller.require_auth();

        let now = env.ledger().timestamp();
        if period_start > period_end || period_end > now {
            return Err(ContractError::InvalidPeriod);
        }
        if !Self::valid_hashes(&env, &package_hash, &source_checkpoint_hash, &evidence_hashes) {
            return Err(ContractError::InvalidEvidenceHash);
        }

        let export_key = DataKey::Export(program_id.clone(), export_id.clone());
        let record = AuditExport {
            export_id: export_id.clone(),
            program_id: program_id.clone(),
            period_start,
            period_end,
            generated_at: now,
            generated_by: caller.clone(),
            schema_version: CONTRACT_VERSION,
            package_hash: package_hash.clone(),
            source_checkpoint_hash: source_checkpoint_hash.clone(),
            evidence_hashes: evidence_hashes.clone(),
        };
        if let Some(existing) = env.storage().persistent().get::<DataKey, AuditExport>(&export_key)
        {
            if Self::same_export_inputs(&existing, &record) {
                Self::extend_ttl(&env, &export_key);
                return Ok(());
            }
            return Err(ContractError::ExportIdConflict);
        }

        let count_key = DataKey::ExportCount(program_id.clone());
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        if count >= MAX_EXPORTS_PER_PROGRAM {
            return Err(ContractError::ExportLimitReached);
        }
        let next_count = count
            .checked_add(1)
            .ok_or(ContractError::CounterOverflow)?;

        env.storage().persistent().set(&export_key, &record);
        Self::extend_ttl(&env, &export_key);
        env.storage().persistent().set(&count_key, &next_count);
        Self::extend_ttl(&env, &count_key);
        Self::extend_ttl(&env, &program_key);
        env.events().publish(
            (symbol_short!("AUDEXPORT"),),
            (
                export_id,
                program_id,
                package_hash,
                period_start,
                period_end,
                now,
            ),
        );
        Ok(())
    }

    fn same_export_inputs(existing: &AuditExport, proposed: &AuditExport) -> bool {
        existing.export_id == proposed.export_id
            && existing.program_id == proposed.program_id
            && existing.period_start == proposed.period_start
            && existing.period_end == proposed.period_end
            && existing.generated_by == proposed.generated_by
            && existing.schema_version == proposed.schema_version
            && existing.package_hash == proposed.package_hash
            && existing.source_checkpoint_hash == proposed.source_checkpoint_hash
            && existing.evidence_hashes == proposed.evidence_hashes
    }

    fn valid_hashes(
        env: &Env,
        package_hash: &BytesN<32>,
        checkpoint_hash: &BytesN<32>,
        hashes: &EvidenceHashes,
    ) -> bool {
        let zero = BytesN::from_array(env, &[0; 32]);
        *package_hash != zero
            && *checkpoint_hash != zero
            && hashes.governance != zero
            && hashes.decisions != zero
            && hashes.consent != zero
            && hashes.funds != zero
            && hashes.payments != zero
    }

    pub fn get_export(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        export_id: BytesN<32>,
    ) -> Result<AuditExport, ContractError> {
        caller.require_auth();
        let program_key = DataKey::Program(program_id.clone());
        let program: Program = env
            .storage()
            .persistent()
            .get(&program_key)
            .ok_or(ContractError::ProgramNotFound)?;
        let admin = Self::admin(&env)?;
        let is_regulator: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Regulator(program_id.clone(), caller.clone()))
            .unwrap_or(false);
        if caller != admin && caller != program.sponsor && !is_regulator {
            return Err(ContractError::Unauthorized);
        }

        let key = DataKey::Export(program_id, export_id);
        let record = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ExportNotFound)?;
        Self::extend_ttl(&env, &key);
        Ok(record)
    }

    pub fn export_count(env: Env, program_id: BytesN<32>) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::ExportCount(program_id))
            .unwrap_or(0)
    }

    pub fn version() -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;