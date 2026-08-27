use crate::errors::EscrowError;
use crate::events::escrow_created;
use crate::storage::{
    append_to_buyer_index, append_to_token_index, is_token_whitelisted, next_escrow_id, save_escrow,
};
use crate::types::{Escrow, EscrowStatus};
use soroban_sdk::{Address, Env};

/// Creates an unfunded escrow record. Tokens are deposited later via `fund_escrow`.
pub fn create_escrow(
    env: &Env,
    buyer: Address,
    seller: Address,
    token: Address,
    amount: i128,
    expiration: u64,
) -> Result<u64, EscrowError> {
    if amount <= 0 {
        return Err(EscrowError::InvalidAmount);
    }
    if expiration <= env.ledger().timestamp() {
        return Err(EscrowError::InvalidExpiration);
    }
    if buyer == seller {
        return Err(EscrowError::InvalidRecipient);
    }

    buyer.require_auth();

    if !is_token_whitelisted(env, &token) {
        return Err(EscrowError::TokenNotAllowed);
    }

    let escrow_id = next_escrow_id(env);
    let escrow = Escrow {
        buyer: buyer.clone(),
        seller: seller.clone(),
        token: token.clone(),
        amount,
        original_amount: amount,
        status: EscrowStatus::Created,
        expiration,
    };
    save_escrow(env, escrow_id, &escrow);
    append_to_token_index(env, &token, escrow_id);
    append_to_buyer_index(env, &buyer, escrow_id);
    escrow_created(env, escrow_id, &buyer, &seller, &token, amount);
    Ok(escrow_id)
}
