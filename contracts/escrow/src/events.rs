use soroban_sdk::{symbol_short, Address, Env};

pub fn escrow_created(env: &Env, escrow_id: u64, buyer: &Address, seller: &Address, token: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_CRE"),),
        (escrow_id, buyer.clone(), seller.clone(), token.clone(), amount),
    );
}

pub fn escrow_released(env: &Env, escrow_id: u64, seller: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_RLSD"),),
        (escrow_id, seller.clone(), amount),
    );
}

/// Emitted when an escrow is funded by the buyer (#863).
pub fn escrow_funded(
    env: &Env,
    escrow_id: u64,
    actor: &Address,
    token: &Address,
    amount: i128,
    resulting_status: &crate::types::EscrowStatus,
) {
    env.events().publish(
        (symbol_short!("ESC_FND"),),
        (escrow_id, actor.clone(), token.clone(), amount, resulting_status.clone()),
    );
}

/// Emitted when a dispute is opened on a funded escrow (#863).
pub fn dispute_opened(
    env: &Env,
    escrow_id: u64,
    actor: &Address,
    token: &Address,
    amount: i128,
    resulting_status: &crate::types::EscrowStatus,
) {
    env.events().publish(
        (symbol_short!("DSP_OPN"),),
        (escrow_id, actor.clone(), token.clone(), amount, resulting_status.clone()),
    );
}

/// Emitted when a portion of locked funds is released to the seller (#863).
pub fn partial_released(
    env: &Env,
    escrow_id: u64,
    actor: &Address,
    token: &Address,
    amount: i128,
    resulting_status: &crate::types::EscrowStatus,
) {
    env.events().publish(
        (symbol_short!("PRT_RLS"),),
        (escrow_id, actor.clone(), token.clone(), amount, resulting_status.clone()),
    );
}

/// Emitted when an arbiter resolves a disputed escrow by allocating remaining
/// funds between buyer and seller (#864).
pub fn escrow_resolved(
    env: &Env,
    escrow_id: u64,
    arbiter: &Address,
    buyer_amount: i128,
    seller_amount: i128,
    fee_amount: i128,
    reason_hash: &soroban_sdk::BytesN<32>,
) {
    env.events().publish(
        (symbol_short!("ESC_RSLV"),),
        (
            escrow_id,
            arbiter.clone(),
            buyer_amount,
            seller_amount,
            fee_amount,
            reason_hash.clone(),
        ),
    );
}

pub fn escrow_refunded(env: &Env, escrow_id: u64, buyer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_RFND"),),
        (escrow_id, buyer.clone(), amount),
    );
}

pub fn fee_collected(env: &Env, escrow_id: u64, token: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("FEE_COL"),),
        (escrow_id, token.clone(), amount),
    );
}

pub fn fee_withdrawn(env: &Env, recipient: &Address, token: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("FEE_WDR"),),
        (recipient.clone(), token.clone(), amount),
    );
}
