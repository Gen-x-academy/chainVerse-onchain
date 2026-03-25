use soroban_sdk::{Address, Env, symbol_short};

pub fn escrow_created(env: &Env, escrow_id: u64, buyer: &Address, seller: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("ESC_CRTD"),),
        (escrow_id, buyer.clone(), seller.clone(), amount),
    );
}
