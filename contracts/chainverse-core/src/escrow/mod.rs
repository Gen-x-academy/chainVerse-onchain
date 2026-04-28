use soroban_sdk::{contracttype, Address, Env, Vec};

use crate::errors::ContractError;

pub mod active_query;
pub mod id_generator;
pub mod pagination;
pub mod status;
pub mod status_validator;

pub use active_query::get_active_escrows;
pub use id_generator::next_escrow_id;
pub use pagination::paginate;
pub use status::EscrowStatus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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
    /// Unix timestamp after which the escrow can be refunded. 0 means no expiry.
    pub expires_at: u64,
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
/// `expires_at` is a Unix timestamp; pass 0 for no expiry.
pub fn create(
    env: &Env,
    depositor: Address,
    recipient: Address,
    token: Address,
    amount: i128,
    expires_at: u64,
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
        expires_at,
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

    soroban_sdk::token::Client::new(env, &record.token)
        .transfer(&env.current_contract_address(), &record.recipient, &record.amount);

    record.status = EscrowStatus::Completed;
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

/// Returns all escrows where `buyer` is the depositor.
pub fn get_by_buyer(env: &Env, buyer: &Address) -> Vec<EscrowRecord> {
    let mut results = Vec::new(env);
    let count: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);
    for i in 0..count {
        if let Ok(record) = load(env, i) {
            if &record.depositor == buyer {
                results.push_back(record);
            }
        }
    }
    results
}

/// Returns all escrows where `seller` is the recipient.
pub fn get_by_seller(env: &Env, seller: &Address) -> Vec<EscrowRecord> {
    let mut results = Vec::new(env);
    let count: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);
    for i in 0..count {
        if let Ok(record) = load(env, i) {
            if &record.recipient == seller {
                results.push_back(record);
            }
        }
    }
    results
}

/// Allows the buyer (depositor) to cancel a Pending escrow before the seller
/// has interacted (i.e. while status is still Pending). Marks it Cancelled.
pub fn buyer_cancel(env: &Env, buyer: Address, id: u64) -> Result<(), ContractError> {
    buyer.require_auth();
    let mut record = load(env, id)?;

    if record.depositor != buyer {
        return Err(ContractError::Unauthorized);
    }
    if record.status != EscrowStatus::Pending {
        // Seller has already interacted or escrow is no longer open.
        return Err(ContractError::InvalidEscrowState);
    }

    record.status = EscrowStatus::Cancelled;
    save(env, &record);
    Ok(())
}

/// Refunds an expired escrow back to the depositor.
/// Anyone may call this once the escrow has passed its `expires_at` timestamp.
pub fn refund_expired(env: &Env, id: u64) -> Result<(), ContractError> {
    let mut record = load(env, id)?;

    if record.expires_at == 0 {
        return Err(ContractError::InvalidEscrowState);
    }
    if env.ledger().timestamp() < record.expires_at {
        return Err(ContractError::EscrowNotExpired);
    }
    if record.status != EscrowStatus::Pending {
        return Err(ContractError::InvalidEscrowState);
    }

    record.status = EscrowStatus::Expired;
    save(env, &record);
    Ok(())
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
