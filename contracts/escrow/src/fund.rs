use crate::errors::EscrowError;
use crate::events::escrow_funded;
use crate::storage::{add_to_total_volume, load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{token::Client as TokenClient, Address, Env};

/// Funds a `Created` escrow. Only the escrow buyer may deposit tokens.
pub fn fund_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status != EscrowStatus::Created {
        return Err(EscrowError::InvalidEscrowState);
    }

    // #709: reject funding if the escrow has already expired.
    if env.ledger().timestamp() >= escrow.expiration {
        return Err(EscrowError::Expired);
    }

    TokenClient::new(env, &escrow.token).transfer(
        &escrow.buyer,
        &env.current_contract_address(),
        &escrow.amount,
    );

    let funded_amount = escrow.amount;
    escrow.status = EscrowStatus::Funded;
    save_escrow(env, escrow_id, &escrow);
    add_to_total_volume(env, escrow.amount);
    escrow_funded(
        env,
        escrow_id,
        &caller,
        &escrow.token,
        funded_amount,
        &EscrowStatus::Funded,
    );
    Ok(())
}
