use crate::errors::EscrowError;
use crate::storage::{load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{Address, Env};

/// Opens a dispute on an escrow.
///
/// State machine (dispute transition only):
/// ```text
///   Pending    --dispute-->  Disputed        (allowed)
///   Completed  --dispute-->  Err(NotPending)  (already released — rejected)
///   Cancelled  --dispute-->  Err(NotPending)  (already settled — rejected)
///   Disputed   --dispute-->  Err(AlreadyDisputed)
/// ```
///
/// Only the buyer may dispute, and only while the escrow is still `Pending`
/// (funded). This prevents post-settlement disputes: a buyer who has already
/// had funds released (status `Completed`) or refunded (`Cancelled`) can no
/// longer flip a zero-balance escrow back to `Disputed` and corrupt its state.
pub fn dispute(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
/// Opens a dispute on a funded escrow. Buyer or seller may call.
pub fn dispute_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer {
    if caller != escrow.buyer && caller != escrow.seller {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status == EscrowStatus::Disputed {
        return Err(EscrowError::AlreadyDisputed);
    }

    // Reject disputes on any escrow that is no longer funded.
    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status != EscrowStatus::Funded {
        return Err(EscrowError::InvalidEscrowState);
    }

    // #714 — validate the status transition before writing it.
    crate::escrow_state::assert_transition_allowed(&escrow.status, &EscrowStatus::Disputed)?;

    escrow.status = EscrowStatus::Disputed;
    save_escrow(env, escrow_id, &escrow);
    Ok(())
}
