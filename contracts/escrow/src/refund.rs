use soroban_sdk::{token::Client as TokenClient, Env};
use crate::errors::EscrowError;
use crate::events::escrow_refunded;
use crate::storage::{decrement_active_escrows, load_escrow, save_escrow};
use crate::types::EscrowStatus;

pub fn refund_buyer(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    // Load escrow, return NotFound if missing
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    // Validate: only the buyer can claim a refund
    escrow.buyer.require_auth();

    // Validate: escrow must be in Pending state
    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    // Validate: escrow must be expired before a refund is allowed
    if env.ledger().timestamp() < escrow.expiration {
        return Err(EscrowError::NotExpired);
    }

    // Transfer tokens from this contract back to the buyer
    TokenClient::new(env, &escrow.token).transfer(
        &env.current_contract_address(),
        &escrow.buyer,
        &escrow.amount,
    );

    // Update status to Cancelled
    escrow.status = EscrowStatus::Cancelled;
    save_escrow(env, escrow_id, &escrow);
    decrement_active_escrows(env);

    // Emit event
    escrow_refunded(env, escrow_id, &escrow.buyer, escrow.amount);

    Ok(())
}
