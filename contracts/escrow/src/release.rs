use crate::errors::EscrowError;
use crate::events::{escrow_released, fee_collected};
use crate::storage::{
    accumulate_protocol_fee, append_fee_record, get_protocol_fee_bps, load_escrow, save_escrow,
};
use crate::types::{EscrowStatus, FeeRecord};
use soroban_sdk::{token::Client as TokenClient, Address, Env};

/// Releases funds to the seller. Buyer or admin may authorize.
pub fn release_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    caller.require_auth();
    let is_buyer = caller == escrow.buyer;
    let is_admin = crate::storage::get_admin(env).as_ref() == Some(&caller);
    if !is_buyer && !is_admin {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status != EscrowStatus::Funded {
        return Err(EscrowError::InvalidEscrowState);
    }

    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    let fee_bps = get_protocol_fee_bps(env) as i128;
    let fee_amount = escrow.amount * fee_bps / 10_000;
    let seller_amount = escrow.amount - fee_amount;

    let token_client = TokenClient::new(env, &escrow.token);
    token_client.transfer(
        &env.current_contract_address(),
        &escrow.seller,
        &seller_amount,
    );

    accumulate_protocol_fee(env, &escrow.token, fee_amount);

    let record = FeeRecord {
        escrow_id,
        token: escrow.token.clone(),
        amount: fee_amount,
        timestamp: env.ledger().timestamp(),
    };
    append_fee_record(env, &record);
    fee_collected(env, escrow_id, &escrow.token, fee_amount);

    escrow.status = EscrowStatus::Completed;
    save_escrow(env, escrow_id, &escrow);
    escrow_released(env, escrow_id, &escrow.seller, seller_amount);
    Ok(())
}

/// Backwards-compatible entry used by older call sites.
pub fn release_funds(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;
    let buyer = escrow.buyer.clone();
    release_escrow(env, buyer, escrow_id)
}
