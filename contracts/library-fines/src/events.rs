use soroban_sdk::{symbol_short, Address, BytesN, Env};

pub fn assessment_recorded(
    env: &Env,
    patron_ref: BytesN<32>,
    ref_id: BytesN<32>,
    amount: i128,
    new_balance: i128,
) {
    env.events().publish(
        (symbol_short!("ASSESSED"),),
        (patron_ref, ref_id, amount, new_balance),
    );
}

/// Emits actor, amount, reason_hash, and resulting balance per #981.
pub fn waiver_granted(
    env: &Env,
    actor: Address,
    patron_ref: BytesN<32>,
    ref_id: BytesN<32>,
    amount: i128,
    reason_hash: BytesN<32>,
    new_balance: i128,
) {
    env.events().publish(
        (symbol_short!("WAIVED"),),
        (actor, patron_ref, ref_id, amount, reason_hash, new_balance),
    );
}

pub fn payment_initiated(
    env: &Env,
    patron_ref: BytesN<32>,
    settlement_id: BytesN<32>,
    asset: Address,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("PAY_INIT"),),
        (patron_ref, settlement_id, asset, amount),
    );
}

/// Receipt identifies the resulting ledger entry via `ref_id` (#982).
pub fn payment_confirmed(
    env: &Env,
    patron_ref: BytesN<32>,
    settlement_id: BytesN<32>,
    ref_id: BytesN<32>,
    new_balance: i128,
) {
    env.events().publish(
        (symbol_short!("PAY_CONF"),),
        (patron_ref, settlement_id, ref_id, new_balance),
    );
}

pub fn payment_failed(env: &Env, patron_ref: BytesN<32>, settlement_id: BytesN<32>) {
    env.events().publish(
        (symbol_short!("PAY_FAIL"),),
        (patron_ref, settlement_id),
    );
}

pub fn payment_refunded(
    env: &Env,
    patron_ref: BytesN<32>,
    settlement_id: BytesN<32>,
    ref_id: BytesN<32>,
    new_balance: i128,
) {
    env.events().publish(
        (symbol_short!("PAY_RFND"),),
        (patron_ref, settlement_id, ref_id, new_balance),
    );
}

pub fn payment_reversed(
    env: &Env,
    patron_ref: BytesN<32>,
    settlement_id: BytesN<32>,
    ref_id: BytesN<32>,
    new_balance: i128,
) {
    env.events().publish(
        (symbol_short!("PAY_REV"),),
        (patron_ref, settlement_id, ref_id, new_balance),
    );
}
