use soroban_sdk::{contracttype, Address, Env};

use crate::errors::ContractError;

pub mod status_validator;
pub use status_validator::validate_transition;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lifecycle of an escrow deposit.
#[contracttype]
#[derive(Clone, PartialEq)]
pub enum EscrowStatus {
    /// Funds are held, awaiting release or cancellation.
    Pending,
    /// Funds have been released to the recipient.
    Released,
    /// Funds have been returned to the depositor.
    Cancelled,
}

/// A single escrow record stored on-chain.
#[contracttype]
#[derive(Clone)]
pub struct EscrowRecord {
    pub id: u64,
    pub depositor: Address,
    pub recipient: Address,
    pub token: Address,
    pub amount: i128,
    pub status: EscrowStatus,
    pub created_at: u64,
}

/// Storage key for escrow entries.
#[contracttype]
#[derive(Clone)]
pub enum EscrowKey {
    /// Maps escrow id → EscrowRecord.
    Record(u64),
    /// Monotonically increasing counter used to generate new ids.
    NextId,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn next_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);
    env.storage()
        .persistent()
        .set(&EscrowKey::NextId, &(id + 1));
    id
}

fn load(env: &Env, id: u64) -> Result<EscrowRecord, ContractError> {
    env.storage()
        .persistent()
        .get(&EscrowKey::Record(id))
        .ok_or(ContractError::EscrowNotFound)
}

fn save(env: &Env, record: &EscrowRecord) {
    env.storage()
        .persistent()
        .set(&EscrowKey::Record(record.id), record);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Creates a new escrow and returns its id.
/// The depositor must authorise this call.
pub fn create(
    env: &Env,
    depositor: Address,
    recipient: Address,
    token: Address,
    amount: i128,
) -> Result<u64, ContractError> {
    depositor.require_auth();
    crate::utils::validate_amount(amount)?;
    crate::utils::require_supported_token(env, &token)?;

    let id = next_id(env);
    let record = EscrowRecord {
        id,
        depositor,
        recipient,
        token,
        amount,
        status: EscrowStatus::Pending,
        created_at: env.ledger().timestamp(),
    };
    save(env, &record);
    Ok(id)
}

/// Releases a pending escrow to the recipient.
/// Only the depositor may release funds.
pub fn release(env: &Env, caller: Address, id: u64) -> Result<(), ContractError> {
    caller.require_auth();
    let mut record = load(env, id)?;

    if record.status != EscrowStatus::Pending {
        return Err(ContractError::InvalidEscrowState);
    }
    if record.depositor != caller {
        return Err(ContractError::Unauthorized);
    }

    record.status = EscrowStatus::Released;
    save(env, &record);
    Ok(())
}

/// Cancels a pending escrow, returning funds to the depositor.
/// Only the depositor may cancel.
pub fn cancel(env: &Env, caller: Address, id: u64) -> Result<(), ContractError> {
    caller.require_auth();
    let mut record = load(env, id)?;

    if record.status != EscrowStatus::Pending {
        return Err(ContractError::InvalidEscrowState);
    }
    if record.depositor != caller {
        return Err(ContractError::Unauthorized);
    }

    record.status = EscrowStatus::Cancelled;
    save(env, &record);
    Ok(())
}

/// Returns the escrow record for the given id.
pub fn get(env: &Env, id: u64) -> Result<EscrowRecord, ContractError> {
    load(env, id)
}

/// Returns all escrows optionally filtered by token and/or status.
/// Using iterative search — for high-volume use cases, off-chain indexing is recommended.
pub fn search(
    env: &Env,
    token: Option<Address>,
    status: Option<EscrowStatus>,
) -> soroban_sdk::Vec<EscrowRecord> {
    let mut results = soroban_sdk::Vec::new(env);
    let count: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);

    for i in 0..count {
        if let Ok(record) = load(env, i) {
            let mut matches = true;
            if let Some(t) = &token {
                if &record.token != t {
                    matches = false;
                }
            }
            if let Some(s) = &status {
                if &record.status != s {
                    matches = false;
                }
            }
            if matches {
                results.push_back(record);
            }
        }
    }
    results
}
