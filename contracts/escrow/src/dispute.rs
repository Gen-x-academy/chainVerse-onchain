use crate::errors::EscrowError;
use crate::events::dispute_opened;
use crate::storage::{load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{Address, Env};

/// Opens a dispute on a funded escrow. Only the buyer may dispute, and
/// only while the escrow is `Pending` — a released or cancelled escrow can
/// no longer be disputed, preventing post-settlement state corruption.
pub fn dispute(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status == EscrowStatus::Disputed {
        return Err(EscrowError::AlreadyDisputed);
    }

    // Reject disputes on any escrow that is no longer funded.
    if escrow.status != EscrowStatus::Funded {
        return Err(EscrowError::InvalidEscrowState);
    }

    // #714 — validate the status transition before writing it.
    crate::escrow_state::assert_transition_allowed(&escrow.status, &EscrowStatus::Disputed)?;

    let affected_amount = escrow.amount;
    escrow.status = EscrowStatus::Disputed;
    save_escrow(env, escrow_id, &escrow);
    dispute_opened(
        env,
        escrow_id,
        &caller,
        &escrow.token,
        affected_amount,
        &EscrowStatus::Disputed,
    );
    Ok(())
}
