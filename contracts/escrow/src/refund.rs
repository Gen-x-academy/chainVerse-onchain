use crate::errors::EscrowError;
use crate::events::escrow_refunded;
use crate::storage::{load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{token::Client as TokenClient, Address, Env};

/// Refunds remaining funds to the buyer after the escrow deadline.
pub fn refund_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    if env.ledger().timestamp() < escrow.expiration {
        return Err(EscrowError::NotExpired);
    }

    TokenClient::new(env, &escrow.token).transfer(
        &env.current_contract_address(),
        &escrow.buyer,
        &escrow.amount,
    );

    let refunded = escrow.amount;
    escrow.status = EscrowStatus::Cancelled;
    escrow.amount = 0;
    save_escrow(env, escrow_id, &escrow);
    escrow_refunded(env, escrow_id, &escrow.buyer, refunded);
    Ok(())
}

/// Backwards-compatible alias.
pub fn refund_buyer(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;
    let buyer = escrow.buyer.clone();
    refund_escrow(env, buyer, escrow_id)
}
