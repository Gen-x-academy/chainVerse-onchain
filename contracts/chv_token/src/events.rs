use soroban_sdk::{symbol_short, Address, Env};

/// Emitted when tokens are transferred between accounts.
pub fn emit_transfer(env: &Env, from: &Address, to: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("transfer"), from.clone(), to.clone()),
        amount,
    );
}

/// Emitted when new tokens are minted to an account.
pub fn emit_mint(env: &Env, to: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("mint"), to.clone()),
        amount,
    );
}

/// Emitted when tokens are burned from an account.
pub fn emit_burn(env: &Env, from: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("burn"), from.clone()),
        amount,
    );
}

/// Emitted when an account is frozen.
pub fn emit_freeze(env: &Env, account: &Address) {
    env.events().publish(
        (symbol_short!("freeze"), account.clone()),
        (),
    );
}

/// Emitted when an account is unfrozen.
pub fn emit_unfreeze(env: &Env, account: &Address) {
    env.events().publish(
        (symbol_short!("unfreeze"), account.clone()),
        (),
    );
}
