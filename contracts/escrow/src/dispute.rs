use crate::errors::EscrowError;
use crate::storage::{load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{Address, Env};

/// Opens a dispute on a funded escrow. Buyer or seller may call.
pub fn dispute_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer && caller != escrow.seller {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status == EscrowStatus::Disputed {
        return Err(EscrowError::AlreadyDisputed);
    }

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status != EscrowStatus::Funded {
        return Err(EscrowError::InvalidEscrowState);
    }

    escrow.status = EscrowStatus::Disputed;
    save_escrow(env, escrow_id, &escrow);
    Ok(())
}
