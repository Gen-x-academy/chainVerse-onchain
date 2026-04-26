use soroban_sdk::{Address, Env, symbol_short};

pub fn escrow_created(env: &Env, escrow_id: u64, buyer: &Address, seller: &Address, token: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_CRTD"),),
        (escrow_id, buyer.clone(), seller.clone(), token.clone(), amount),
    );
}

pub fn escrow_released(env: &Env, escrow_id: u64, seller: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_RLSD"),),
        (escrow_id, seller.clone(), amount),
    );
}

pub fn escrow_refunded(env: &Env, escrow_id: u64, buyer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_RFND"),),
        (escrow_id, buyer.clone(), amount),
    );
}
