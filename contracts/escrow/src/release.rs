use crate::errors::EscrowError;
use crate::events::{escrow_released, fee_collected, partial_released};
use crate::storage::{
    accumulate_protocol_fee, append_fee_record, get_admin, get_protocol_fee_bps, load_escrow,
    save_escrow,
};
use crate::types::{EscrowStatus, FeeRecord};
use soroban_sdk::{token::Client as TokenClient, Address, Env};

/// Computes the protocol fee and the net payout for a released amount using
/// checked arithmetic and consistent truncation (#861).
///
/// `fee = amount * fee_bps / 10_000` truncated to whole tokens; the payout is
/// `amount - fee`, which is always non-negative for `fee_bps <= 10_000`.
fn compute_fee_and_payout(amount: i128, fee_bps: u32) -> Result<(i128, i128), EscrowError> {
    if amount < 0 {
        return Err(EscrowError::InvalidAmount);
    }
    let amount_u = amount as u128;
    let fee_u = (amount_u as u128)
        .checked_mul(fee_bps as u128)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(EscrowError::InvalidAmount)?;
    let payout_u = amount_u
        .checked_sub(fee_u)
        .ok_or(EscrowError::InvalidAmount)?;
    Ok((fee_u as i128, payout_u as i128))
}

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
/// Requires the escrow to be in `Funded` or `Disputed` state (#708).
pub fn release_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if escrow.status == EscrowStatus::Completed {
        return Err(EscrowError::AlreadyReleased);
    }

    // #708: guard — only Funded or Disputed escrows may be released.
    if escrow.status != EscrowStatus::Funded && escrow.status != EscrowStatus::Disputed {
        return Err(EscrowError::InvalidEscrowState);
    }

    authorize_releaser(env, &caller, &escrow.buyer)?;

    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    let fee_bps = get_protocol_fee_bps(env);
    let (fee_amount, seller_amount) = compute_fee_and_payout(escrow.amount, fee_bps)?;

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

/// Releases a portion of the locked funds to the seller. Remaining amount stays locked.
/// Requires the escrow to be in `Funded` or `Disputed` state (#708).
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

    // #708: guard — only Funded or Disputed escrows may be partially released.
    if escrow.status != EscrowStatus::Funded && escrow.status != EscrowStatus::Disputed {
        return Err(EscrowError::InvalidEscrowState);
    }

    authorize_releaser(env, &caller, &escrow.buyer)?;

    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    if release_amount > escrow.amount {
        return Err(EscrowError::InvalidAmount);
    }

    let fee_bps = get_protocol_fee_bps(env);
    let (fee_amount, seller_amount) = compute_fee_and_payout(release_amount, fee_bps)?;

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
    partial_released(
        env,
        escrow_id,
        &caller,
        &escrow.token,
        release_amount,
        &escrow.status,
    );
    Ok(())
}

/// Backwards-compatible alias used by older call sites.
pub fn release_funds(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;
    let buyer = escrow.buyer.clone();
    release_escrow(env, buyer, escrow_id)
}
