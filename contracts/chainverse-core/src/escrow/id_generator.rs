use soroban_sdk::Env;

use super::EscrowKey;

/// Reads the current counter, increments it in persistent storage, and returns
/// the old value as the next unique escrow ID.
///
/// IDs start at 0 and increase by 1 for every new escrow. The counter is stored
/// under `EscrowKey::NextId` so it survives ledger upgrades.
pub fn next_escrow_id(env: &Env) -> u64 {
    let current: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);

    env.storage()
        .persistent()
        .set(&EscrowKey::NextId, &(current + 1));

    current
}

/// Returns the total number of escrows ever created (i.e. the next ID that
/// would be assigned) without modifying state.
pub fn current_escrow_count(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64)
}
