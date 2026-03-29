use soroban_sdk::{token::Client as TokenClient, Env};
use crate::errors::EscrowError;
use crate::events::escrow_released;
use crate::storage::{load_escrow, save_escrow};
use crate::types::EscrowStatus;

pub fn release_funds(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    // Load escrow, return NotFound if missing
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    // Validate: only buyer can release funds to seller
    escrow.buyer.require_auth();

    // Explicit guard: prevent double release
    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    // Validate: escrow must be in Pending state
    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    // Validate: escrow must not be expired
    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    // Transfer tokens from this contract to the seller
    TokenClient::new(env, &escrow.token).transfer(
        &env.current_contract_address(),
        &escrow.seller,
        &escrow.amount,
    );

    // Update status to Completed
    escrow.status = EscrowStatus::Completed;
    save_escrow(env, escrow_id, &escrow);

    // Emit release event
    escrow_released(env, escrow_id, &escrow.seller, escrow.amount);

    Ok(())
}
