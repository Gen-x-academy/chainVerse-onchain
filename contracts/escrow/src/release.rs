use crate::errors::EscrowError;
use crate::events::{escrow_released, fee_collected};
use crate::storage::{
    accumulate_protocol_fee, append_fee_record, get_protocol_fee_bps, load_escrow, save_escrow,
};
use crate::types::{EscrowStatus, FeeRecord};
use soroban_sdk::{token::Client as TokenClient, Env};

pub fn release_funds(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    escrow.buyer.require_auth();

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status == EscrowStatus::Disputed {
        return Err(EscrowError::NotPending);
    }

    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    // Compute protocol fee
    let fee_bps = get_protocol_fee_bps(env) as i128;
    let fee_amount = escrow.amount * fee_bps / 10_000;
    let seller_amount = escrow.amount - fee_amount;

    let token_client = TokenClient::new(env, &escrow.token);

    // Transfer seller's share
    token_client.transfer(
        &env.current_contract_address(),
        &escrow.seller,
        &seller_amount,
    );

    // Accumulate the protocol fee
    accumulate_protocol_fee(env, &escrow.token, fee_amount);

    // Persist fee record
    let record = FeeRecord {
        escrow_id,
        token: escrow.token.clone(),
        amount: fee_amount,
        timestamp: env.ledger().timestamp(),
    };
    append_fee_record(env, &record);

    // Emit fee event
    fee_collected(env, escrow_id, &escrow.token, fee_amount);

    // Update escrow status
    escrow.status = EscrowStatus::Completed;
    save_escrow(env, escrow_id, &escrow);

    escrow_released(env, escrow_id, &escrow.seller, seller_amount);

    Ok(())
}
