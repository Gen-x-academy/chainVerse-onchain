use crate::errors::EscrowError;
use crate::events::{escrow_released, fee_collected};
use crate::storage::{
    accumulate_protocol_fee, append_fee_record, get_admin, get_protocol_fee_bps, load_escrow,
    save_escrow,
};
use crate::types::{EscrowStatus, FeeRecord};
use soroban_sdk::{token::Client as TokenClient, Address, Env};

fn authorize_releaser(env: &Env, caller: &Address, buyer: &Address) -> Result<(), EscrowError> {
    caller.require_auth();
    if caller == buyer {
        return Ok(());
    }
    if get_admin(env).as_ref() == Some(caller) {
        return Ok(());
    }
    Err(EscrowError::Unauthorized)
}

/// Releases the full remaining balance of a funded escrow to the seller.
pub fn release_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    authorize_releaser(env, &caller, &escrow.buyer)?;

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
    escrow.amount = 0;
    save_escrow(env, escrow_id, &escrow);
    escrow_released(env, escrow_id, &escrow.seller, seller_amount);
    Ok(())
}

/// Releases part of a funded escrow to the seller. Remaining amount stays locked.
pub fn partial_release(
    env: &Env,
    caller: Address,
    escrow_id: u64,
    release_amount: i128,
) -> Result<(), EscrowError> {
    if release_amount <= 0 {
        return Err(EscrowError::InvalidAmount);
    }

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    if escrow.status != EscrowStatus::Pending {
        return Err(EscrowError::NotPending);
    }

    authorize_releaser(env, &caller, &escrow.buyer)?;

    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    if release_amount > escrow.amount {
        return Err(EscrowError::InvalidAmount);
    }

    let fee_bps = get_protocol_fee_bps(env) as i128;
    let fee_amount = release_amount * fee_bps / 10_000;
    let seller_amount = release_amount - fee_amount;

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

    escrow.amount -= release_amount;
    if escrow.amount == 0 {
        escrow.status = EscrowStatus::Completed;
    }
    save_escrow(env, escrow_id, &escrow);
    escrow_released(env, escrow_id, &escrow.seller, seller_amount);
    Ok(())
}

/// Backwards-compatible alias used by older call sites / snapshots.
pub fn release_funds(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;
    let buyer = escrow.buyer.clone();
    release_escrow(env, buyer, escrow_id)
}
