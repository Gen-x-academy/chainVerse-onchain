use crate::errors::EscrowError;
use crate::storage::{add_to_total_volume, load_escrow, save_escrow};
use crate::types::EscrowStatus;
use soroban_sdk::{token::Client as TokenClient, Env};

/// Funds a `Created` escrow by transferring tokens from the buyer.
///
/// Authorization is bound to `escrow.buyer` via `require_auth` so a third party
/// cannot fund (or confuse funding of) an escrow on the buyer's behalf.
pub fn fund_escrow(env: &Env, escrow_id: u64) -> Result<(), EscrowError> {
    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    // Critical: only the buyer may authorize funding; tokens are pulled from buyer.
    escrow.buyer.require_auth();

    if escrow.status != EscrowStatus::Created {
        return Err(EscrowError::InvalidEscrowState);
use soroban_sdk::{token::Client as TokenClient, Address, Env};

/// Funds a `Created` escrow. Only the buyer may deposit tokens.
pub fn fund_escrow(env: &Env, caller: Address, escrow_id: u64) -> Result<(), EscrowError> {
    caller.require_auth();

    let mut escrow = load_escrow(env, escrow_id).ok_or(EscrowError::NotFound)?;

    if caller != escrow.buyer {
        return Err(EscrowError::Unauthorized);
    }

    if escrow.status != EscrowStatus::Created {
        return Err(EscrowError::NotPending);
    }

    TokenClient::new(env, &escrow.token).transfer(
        &escrow.buyer,
        &env.current_contract_address(),
        &escrow.amount,
    );

    escrow.status = EscrowStatus::Funded;
    escrow.status = EscrowStatus::Pending;
    save_escrow(env, escrow_id, &escrow);
    add_to_total_volume(env, escrow.amount);
    Ok(())
}
