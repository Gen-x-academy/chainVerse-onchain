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

/// Emitted when an allowance is approved. CHV allowances do not expire, so expiry is None.
pub fn emit_approval(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("approval"), owner.clone(), spender.clone()),
        (amount, Option::<u64>::None, amount),
    );
}

/// Emitted after an allowance is consumed.
pub fn emit_allowance_decrement(
    env: &Env,
    owner: &Address,
    spender: &Address,
    amount: i128,
    remaining: i128,
) {
    env.events().publish(
        (symbol_short!("allow_dec"), owner.clone(), spender.clone()),
        (amount, Option::<u64>::None, remaining),
    );
}

/// Emitted when an allowance is revoked.
pub fn emit_allowance_revocation(
    env: &Env,
    owner: &Address,
    spender: &Address,
    amount: i128,
) {
    env.events().publish(
        (symbol_short!("allow_rev"), owner.clone(), spender.clone()),
        (amount, Option::<u64>::None, 0_i128),
    );
}
