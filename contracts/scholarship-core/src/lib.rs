#![no_std]
// `create_program` takes the full program record as explicit parameters —
// eight including `env`. Grouping them into a struct would change the
// contract's public ABI to work around a lint, which is not a trade this
// contract should make.
#![allow(clippy::too_many_arguments)]

//! Scholarship/bursary program data model and lifecycle contract.
//!
//! Scope of this pass (issues #1058, #1061): the canonical `Program`
//! record — stable identifier, sponsor ownership reference, bounded
//! title/description, currency, and funding model — plus an explicit
//! Draft → Published → Paused/Closed → Archived state machine that only
//! allows legal transitions and records who performed each one and when.
//!
//! Sponsor organizations and team membership (#1059, #1060) are tracked
//! separately; this contract treats `sponsor_id` as an opaque reference
//! and `owner` as the program's actual authorization principal until a
//! sponsor-org contract exists to delegate that properly. See
//! `contracts/docs/scholarship-core.md` for ownership, privacy,
//! migration, and operational notes.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, BytesN, Env, String, Symbol,
};

const CONTRACT_VERSION: u32 = 1;

const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

/// #1058 — bounded storage: title/description length caps.
const MAX_TITLE_LEN: u32 = 120;
const MAX_DESCRIPTION_LEN: u32 = 2_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAdmin = 3,
    /// #1058 — a program with this ID already exists; identifiers never change.
    ProgramAlreadyExists = 4,
    ProgramNotFound = 5,
    /// #1058 — title is empty or exceeds MAX_TITLE_LEN.
    InvalidTitle = 6,
    /// #1058 — description exceeds MAX_DESCRIPTION_LEN.
    InvalidDescription = 7,
    /// Caller is not this program's owner.
    NotProgramOwner = 8,
    /// #1061 — the requested status change is not a legal transition from
    /// the program's current status.
    InvalidTransition = 9,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Program(BytesN<32>),
}

/// #1061 — explicit program lifecycle. Only the transitions enumerated in
/// `is_legal_transition` succeed; every other (from, to) pair is rejected.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatus {
    Draft,
    Published,
    Paused,
    Closed,
    Archived,
}

/// #1058 — how a program's awards are funded. Deliberately a small,
/// bounded enum rather than free text.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FundingModel {
    FixedAward,
    MatchingFund,
    MilestoneBased,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    /// #1058 — stable identifier, set once at creation, never changed
    /// after (it is also this record's storage key).
    pub id: BytesN<32>,
    /// Opaque reference to a sponsor organization (#1059) — not validated
    /// here, since no sponsor-org contract exists yet in this pass.
    pub sponsor_id: BytesN<32>,
    pub owner: Address,
    pub title: String,
    pub description: String,
    pub currency: Symbol,
    pub funding_model: FundingModel,
    pub status: ProgramStatus,
    pub created_at: u64,
    /// #1061 — audit trail summary kept on the record itself (bounded,
    /// O(1) fields); the full history is the append-only event log — see
    /// `contracts/docs/scholarship-core.md`.
    pub last_transitioned_by: Address,
    pub last_transitioned_at: u64,
}

#[contract]
pub struct ScholarshipCoreContract;

#[contractimpl]
impl ScholarshipCoreContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    // ── #1058 — program data model ───────────────────────────────────────

    /// Create a new program record. `program_id` becomes its permanent,
    /// immutable identifier — there is no rename/re-key operation.
    /// `owner` (the caller) is the only party who can transition this
    /// program's lifecycle or, later, update its editable fields.
    pub fn create_program(
        env: Env,
        owner: Address,
        program_id: BytesN<32>,
        sponsor_id: BytesN<32>,
        title: String,
        description: String,
        currency: Symbol,
        funding_model: FundingModel,
    ) -> Result<(), ContractError> {
        owner.require_auth();

        let key = DataKey::Program(program_id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ProgramAlreadyExists);
        }

        if title.is_empty() || title.len() > MAX_TITLE_LEN {
            return Err(ContractError::InvalidTitle);
        }
        if description.len() > MAX_DESCRIPTION_LEN {
            return Err(ContractError::InvalidDescription);
        }

        let now = env.ledger().timestamp();
        let program = Program {
            id: program_id.clone(),
            sponsor_id,
            owner: owner.clone(),
            title,
            description,
            currency,
            funding_model,
            status: ProgramStatus::Draft,
            created_at: now,
            last_transitioned_by: owner.clone(),
            last_transitioned_at: now,
        };

        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("PROGNEW"),),
            (program_id, owner),
        );
        Ok(())
    }

    pub fn get_program(env: Env, program_id: BytesN<32>) -> Result<Program, ContractError> {
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

    // ── #1061 — explicit lifecycle ────────────────────────────────────────

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

    /// Move `program_id` to `to`, iff `(current status, to)` is a legal
    /// transition. Only the program's owner may transition it. Every
    /// successful transition updates the bounded `last_transitioned_by`/
    /// `last_transitioned_at` fields on the record *and* emits a
    /// `PROGTRAN` event carrying `(program_id, from, to, actor, at)` —
    /// the event log is the unbounded, queryable audit trail; an archived
    /// program's full transition history remains reconstructable from it
    /// even though the record itself only keeps the latest transition.
    pub fn transition_program(
        env: Env,
        caller: Address,
        program_id: BytesN<32>,
        to: ProgramStatus,
    ) -> Result<(), ContractError> {
        caller.require_auth();

        let key = DataKey::Program(program_id.clone());
        let mut program: Program = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ProgramNotFound)?;

        if caller != program.owner {
            return Err(ContractError::NotProgramOwner);
        }
        if !Self::is_legal_transition(program.status, to) {
            return Err(ContractError::InvalidTransition);
        }

        let from = program.status;
        let now = env.ledger().timestamp();
        program.status = to;
        program.last_transitioned_by = caller.clone();
        program.last_transitioned_at = now;

        env.storage().persistent().set(&key, &program);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);

        env.events().publish(
            (soroban_sdk::symbol_short!("PROGTRAN"),),
            (program_id, from, to, caller, now),
        );
        Ok(())
    }

    pub fn get_program_status(
        env: Env,
        program_id: BytesN<32>,
    ) -> Result<ProgramStatus, ContractError> {
        Ok(Self::get_program(env, program_id)?.status)
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
