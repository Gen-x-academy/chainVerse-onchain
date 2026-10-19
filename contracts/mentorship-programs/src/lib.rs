#![no_std]

//! Organization-owned mentorship program drafts.
//!
//! Audience and outcome details stay off-chain. The contract stores only
//! commitments to those details plus the public configuration needed to
//! identify capacity, schedule, delivery, visibility, and lifecycle.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
};

const CONTRACT_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    ProgramAlreadyExists = 1,
    ProgramNotFound = 2,
    InvalidCapacity = 3,
    InvalidDuration = 4,
    InvalidDates = 5,
    NotProgramOwner = 6,
    ProgramNotDraft = 7,
    InvalidTransition = 8,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Program(BytesN<32>),
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryMode {
    Remote,
    InPerson,
    Hybrid,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramVisibility {
    Public,
    Unlisted,
    Private,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatus {
    Draft,
    Published,
    Paused,
    Closed,
    Archived,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MentorshipProgram {
    pub id: BytesN<32>,
    /// The organization address is both the owner and the authorization principal.
    pub organization: Address,
    /// Commitments to off-chain audience and outcome content, not the content itself.
    pub audience_commitment: BytesN<32>,
    pub outcomes_commitment: BytesN<32>,
    pub capacity: u32,
    pub duration_days: u32,
    pub delivery_mode: DeliveryMode,
    pub starts_at: u64,
    pub ends_at: u64,
    pub visibility: ProgramVisibility,
    pub status: ProgramStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

#[contract]
pub struct MentorshipProgramsContract;

#[contractimpl]
impl MentorshipProgramsContract {
    /// Create an organization-owned draft. The organization address itself
    /// authorizes the operation; membership policies can be enforced by the
    /// organization account or contract that constructs this authorization.
    #[allow(clippy::too_many_arguments)]
    pub fn create_program(
        env: Env,
        organization: Address,
        program_id: BytesN<32>,
        audience_commitment: BytesN<32>,
        outcomes_commitment: BytesN<32>,
        capacity: u32,
        duration_days: u32,
        delivery_mode: DeliveryMode,
        starts_at: u64,
        ends_at: u64,
        visibility: ProgramVisibility,
    ) -> Result<(), ContractError> {
        organization.require_auth();

        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }
        Self::validate_configuration(capacity, duration_days, starts_at, ends_at)?;

        let now = env.ledger().timestamp();
        let program = MentorshipProgram {
            id: program_id.clone(),
            organization: organization.clone(),
            audience_commitment,
            outcomes_commitment,
            capacity,
            duration_days,
            delivery_mode,
            starts_at,
            ends_at,
            visibility,
            status: ProgramStatus::Draft,
            created_at: now,
            updated_at: now,
        };

        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("MENTNEW"),),
            (program_id, organization),
        );
        Ok(())
    }

    /// Replace draft configuration. Once published, terms cannot be silently
    /// changed; a future versioning workflow can add explicit amendments.
    #[allow(clippy::too_many_arguments)]
    pub fn update_draft(
        env: Env,
        organization: Address,
        program_id: BytesN<32>,
        audience_commitment: BytesN<32>,
        outcomes_commitment: BytesN<32>,
        capacity: u32,
        duration_days: u32,
        delivery_mode: DeliveryMode,
        starts_at: u64,
        ends_at: u64,
        visibility: ProgramVisibility,
    ) -> Result<(), ContractError> {
        organization.require_auth();

        let key = DataKey::Program(program_id.clone());
        let mut program: MentorshipProgram = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        Self::require_owner_and_draft(&program, &organization)?;
        Self::validate_configuration(capacity, duration_days, starts_at, ends_at)?;

        program.audience_commitment = audience_commitment;
        program.outcomes_commitment = outcomes_commitment;
        program.capacity = capacity;
        program.duration_days = duration_days;
        program.delivery_mode = delivery_mode;
        program.starts_at = starts_at;
        program.ends_at = ends_at;
        program.visibility = visibility;
        program.updated_at = env.ledger().timestamp();
        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("MENTEDIT"),),
            (program_id, organization, program.updated_at),
        );
        Ok(())
    }

    pub fn transition_program(
        env: Env,
        organization: Address,
        program_id: BytesN<32>,
        to: ProgramStatus,
    ) -> Result<(), ContractError> {
        organization.require_auth();

        let key = DataKey::Program(program_id.clone());
        let mut program: MentorshipProgram = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        if program.organization != organization {
            return Err(ContractError::NotProgramOwner);
        }
        if !Self::is_legal_transition(program.status, to) {
            return Err(ContractError::InvalidTransition);
        }

        let from = program.status;
        let now = env.ledger().timestamp();
        program.status = to;
        program.updated_at = now;
        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        env.events().publish(
            (symbol_short!("MENTSTAT"),),
            (program_id, from, to, organization, now),
        );
        Ok(())
    }

    pub fn get_program(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<MentorshipProgram, ContractError> {
        let key = DataKey::Program(program_id);
        let program = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(program)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    fn validate_configuration(
        capacity: u32,
        duration_days: u32,
        starts_at: u64,
        ends_at: u64,
    ) -> Result<(), ContractError> {
        if capacity == 0 {
            return Err(ContractError::InvalidCapacity);
        }
        if duration_days == 0 {
            return Err(ContractError::InvalidDuration);
        }
        if starts_at >= ends_at {
            return Err(ContractError::InvalidDates);
        }
        Ok(())
    }

    fn require_owner_and_draft(
        program: &MentorshipProgram,
        organization: &Address,
    ) -> Result<(), ContractError> {
        if program.organization != *organization {
            return Err(ContractError::NotProgramOwner);
        }
        if program.status != ProgramStatus::Draft {
            return Err(ContractError::ProgramNotDraft);
        }
        Ok(())
    }

    fn is_legal_transition(from: ProgramStatus, to: ProgramStatus) -> bool {
        use ProgramStatus::*;
        matches!(
            (from, to),
            (Draft, Published)
                | (Draft, Archived)
                | (Published, Paused)
                | (Published, Closed)
                | (Paused, Published)
                | (Paused, Closed)
                | (Closed, Archived)
        )
    }
}

#[cfg(test)]
mod tests;