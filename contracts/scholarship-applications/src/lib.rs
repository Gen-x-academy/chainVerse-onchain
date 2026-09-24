#![no_std]

//! Scholarship/bursary application intake contract.
//!
//! Scope (issues #1074, #1075): register a program, then let an applicant
//! submit exactly one application per (applicant, program) pair in a single
//! atomic transition. On-chain storage is privacy-minimized — it never holds
//! form answers, only a caller-supplied integrity commitment (`data_hash`)
//! over the off-chain application content, plus the metadata needed to
//! enforce uniqueness, deadlines, and consent. See
//! `contracts/docs/scholarship-applications.md` for ownership, privacy,
//! migration, and operational notes.
//!
//! Withdrawal (#1076) and tamper-evident receipts (#1077) are tracked
//! separately and are not implemented here.

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, BytesN, Env};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching course_registry's convention.
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
    ProgramInactive = 6,
    DeadlinePassed = 7,
    ConsentRequired = 8,
    /// #1074 — an application already exists for this (applicant, program) pair.
    DuplicateApplication = 9,
    ApplicationNotFound = 10,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Program(BytesN<32>),
    Application(Address, BytesN<32>),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub active: bool,
    /// Ledger timestamp (seconds) after which submissions are rejected.
    pub deadline: u64,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationStatus {
    Submitted,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application {
    pub applicant: Address,
    pub program_id: BytesN<32>,
    pub status: ApplicationStatus,
    pub submitted_at: u64,
    /// Integrity commitment over the off-chain application content (e.g. a
    /// hash of the form answers/documents). Never the answers themselves —
    /// this contract is privacy-minimized by design.
    pub data_hash: BytesN<32>,
}

#[contract]
pub struct ScholarshipApplicationsContract;

#[contractimpl]
impl ScholarshipApplicationsContract {
    /// Initialize the contract admin. Run once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if *caller != admin {
            return Err(ContractError::NotAdmin);
        }
        caller.require_auth();
        Ok(())
    }

    /// Admin-only: register (or re-register) a program that applications can
    /// be submitted against.
    pub fn register_program(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        deadline: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        let program = Program {
            active: true,
            deadline,
        };
        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    /// Admin-only: activate/deactivate a program (e.g. to stop intake early).
    pub fn set_program_active(
        env: Env,
        admin: Address,
        program_id: BytesN<32>,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;

        let key = DataKey::Program(program_id.clone());
        let mut program: Program = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        program.active = active;

        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(())
    }

    pub fn get_program(env: Env, program_id: BytesN<32>) -> Result<Program, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Program(program_id))
            .ok_or(ContractError::ProgramNotFound)
    }

    /// #1075 — submit an application atomically: eligibility (program
    /// active), deadline, consent, and uniqueness (#1074) are all validated
    /// before any state is written. If any check fails, the whole
    /// invocation reverts (standard Soroban semantics) and no partial
    /// record — and no submission receipt — is ever created, so a failed
    /// check is safe to retry and a retry after a transient failure cannot
    /// duplicate a successful submission.
    pub fn submit_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
        data_hash: BytesN<32>,
        consent: bool,
    ) -> Result<(), ContractError> {
        applicant.require_auth();

        let program: Program = env
            .storage()
            .persistent()
            .get(&DataKey::Program(program_id.clone()))
            .ok_or(ContractError::ProgramNotFound)?;

        if !program.active {
            return Err(ContractError::ProgramInactive);
        }
        if env.ledger().timestamp() > program.deadline {
            return Err(ContractError::DeadlinePassed);
        }
        if !consent {
            return Err(ContractError::ConsentRequired);
        }

        // #1074 — the (applicant, program_id) key itself is the uniqueness
        // constraint: a prior successful submission always leaves this key
        // set, so a retried or duplicate submit is rejected here before any
        // write happens, rather than after.
        let application_key = DataKey::Application(applicant.clone(), program_id.clone());
        if env.storage().persistent().has(&application_key) {
            return Err(ContractError::DuplicateApplication);
        }

        let application = Application {
            applicant: applicant.clone(),
            program_id: program_id.clone(),
            status: ApplicationStatus::Submitted,
            submitted_at: env.ledger().timestamp(),
            data_hash,
        };

        env.storage()
            .persistent()
            .set(&application_key, &application);
        env.storage().persistent().extend_ttl(
            &application_key,
            RECORD_MIN_TTL,
            RECORD_MAX_TTL,
        );

        env.events().publish(
            (soroban_sdk::symbol_short!("SUBMIT"),),
            (applicant, program_id),
        );

        Ok(())
    }

    pub fn get_application(
        env: Env,
        applicant: Address,
        program_id: BytesN<32>,
    ) -> Result<Application, ContractError> {
        let key = DataKey::Application(applicant, program_id);
        let application = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ApplicationNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(application)
    }

    /// #1074 — cheap existence check for callers that only need to know
    /// whether a duplicate would be rejected, without fetching the record.
    pub fn has_applied(env: Env, applicant: Address, program_id: BytesN<32>) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Application(applicant, program_id))
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
