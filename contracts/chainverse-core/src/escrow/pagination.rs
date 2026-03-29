use soroban_sdk::{Env, Vec};

use super::{EscrowKey, EscrowRecord};

/// Returns a page of escrow records starting at `offset`, returning at most `limit` entries.
///
/// Records are iterated in insertion order (by id). Ids that no longer exist
/// (e.g. due to deletion) are silently skipped.
///
/// # Arguments
/// * `offset` - Number of records to skip from the beginning.
/// * `limit`  - Maximum number of records to return.
pub fn paginate(env: &Env, offset: u64, limit: u64) -> Vec<EscrowRecord> {
    let mut results = Vec::new(env);

    let total: u64 = env
        .storage()
        .persistent()
        .get(&EscrowKey::NextId)
        .unwrap_or(0u64);

    let start = offset.min(total);
    let end = (offset + limit).min(total);

    for id in start..end {
        if let Ok(record) = super::get(env, id) {
            results.push_back(record);
        }
    }

    results
}
