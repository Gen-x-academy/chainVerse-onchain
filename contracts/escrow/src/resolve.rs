use crate::errors::EscrowError;
use crate::events::escrow_resolved;
use crate::storage::{accumulate_protocol_fee, get_arbiter, load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{token::Client as TokenClient, Address, BytesN, Env};

/// Resolves a disputed escrow by allocating the remaining locked funds between
/// the buyer and seller, plus an optional resolution fee (#864).
///
/// Only the configured arbiter may call this. The escrow must be in the
/// `Disputed` state, and the allocations must be non-negative with their total
/// (including the resolution fee) not exceeding the remaining escrow amount.
pub fn resolve_dispute(
    env: &Env,
    arbiter: Address,
    escrow_id: u64,
    buyer_amount: i128,
    seller_amount: i128,
    fee_amount: i128,
    reason_hash: BytesN<32>,
) -> Result<(), EscrowError> {
    let configured = get_arbiter(env).ok_or(EscrowError::NoArbiterConfigured)?;
    if configured != arbiter {
        return Err(EscrowError::Unauthorized);
    }
    arbiter.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if escrow.status != EscrowStatus::Disputed {
        return Err(EscrowError::NotDisputed);
    }

    // #714 — validate the status transition before writing it.
    crate::escrow_state::assert_transition_allowed(&escrow.status, &EscrowStatus::Completed)?;

    if buyer_amount < 0 || seller_amount < 0 || fee_amount < 0 {
        return Err(EscrowError::InvalidAllocation);
    }

    // Bounded allocation: the total paid out (buyer + seller + fee) must not
    // exceed the remaining locked balance (#864).
    let total = buyer_amount
        .checked_add(seller_amount)
        .and_then(|v| v.checked_add(fee_amount))
        .ok_or(EscrowError::InvalidAllocation)?;

    if total > escrow.amount {
        return Err(EscrowError::InvalidAllocation);
    }

    let token_client = TokenClient::new(env, &escrow.token);
    if buyer_amount > 0 {
        token_client.transfer(
            &env.current_contract_address(),
            &escrow.buyer,
            &buyer_amount,
        );
    }
    if seller_amount > 0 {
        token_client.transfer(
            &env.current_contract_address(),
            &escrow.seller,
            &seller_amount,
        );
    }

    if fee_amount > 0 {
        accumulate_protocol_fee(env, &escrow.token, fee_amount);
    }

    escrow.status = EscrowStatus::Completed;
    escrow.amount = 0;
    save_escrow(env, escrow_id, &escrow);
    escrow_resolved(
        env,
        escrow_id,
        &arbiter,
        buyer_amount,
        seller_amount,
        fee_amount,
        &reason_hash,
    );
    Ok(())
}
