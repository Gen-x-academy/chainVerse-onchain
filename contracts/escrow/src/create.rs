use crate::errors::EscrowError;
use crate::events::escrow_created;
use crate::storage::{add_to_total_volume, append_to_token_index, is_token_whitelisted, next_escrow_id, save_escrow};
use crate::types::{Escrow, EscrowStatus};
use soroban_sdk::{token::Client as TokenClient, Address, Env};

pub fn create_escrow(
    env: &Env,
    buyer: Address,
    seller: Address,
    token: Address,
    amount: i128,
    expiration: u64,
) -> Result<u64, EscrowError> {
    // Validate order per spec: amount → expiration → self-transfer → whitelist
    if amount <= 0 {
        return Err(EscrowError::InvalidAmount);
    }

    if expiration <= env.ledger().timestamp() {
        return Err(EscrowError::InvalidExpiration);
    }

    if buyer == seller {
        return Err(EscrowError::InvalidRecipient);
    }

    // Validate: buyer must authorize this call
    buyer.require_auth();

    // Validate: token must be whitelisted
    if !is_token_whitelisted(env, &token) {
        return Err(EscrowError::TokenNotAllowed);
    }

    // Transfer funds from buyer into this contract
    TokenClient::new(env, &token).transfer(&buyer, &env.current_contract_address(), &amount);

    // Assign a unique ID and store the escrow
    let escrow_id = next_escrow_id(env);
    let escrow = Escrow {
        buyer: buyer.clone(),
        seller: seller.clone(),
        token,
        amount,
        status: EscrowStatus::Pending,
        expiration,
    };
    save_escrow(env, escrow_id, &escrow);

    // Update the per-token index
    append_to_token_index(env, &escrow.token, escrow_id);

    // Track total volume processed
    add_to_total_volume(env, amount);

    // Emit event
    escrow_created(env, escrow_id, &buyer, &seller, &escrow.token, amount);

    Ok(escrow_id)
}
