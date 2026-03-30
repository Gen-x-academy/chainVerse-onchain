use soroban_sdk::{Env, Vec};

use super::{EscrowKey, EscrowRecord, EscrowStatus};

/// Returns all escrows that are currently in the `Pending` state (i.e. active).
///
/// Iterates over every escrow ID from 0 to the current counter and collects
/// those whose status is `EscrowStatus::Pending`. Missing records are skipped
/// silently.
pub fn get_active_escrows(env: &Env) -> Vec<EscrowRecord> {
    let mut active = Vec::new(env);

    let total: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);

    for id in 0..total {
        if let Some(record) = env
            .storage()
            .persistent()
            .get::<EscrowKey, EscrowRecord>(&EscrowKey::Record(id))
        {
            if record.status == EscrowStatus::Pending {
                active.push_back(record);
            }
        }
    }

    active
}
