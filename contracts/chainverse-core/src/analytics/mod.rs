use soroban_sdk::{contracttype, symbol_short, Env, Symbol};

// ---------------------------------------------------------------------------
// Event names — kept as short symbols (≤ 9 bytes) for efficiency
// ---------------------------------------------------------------------------

pub const EVT_ESCROW_CREATED: Symbol = symbol_short!("ESC_NEW");
pub const EVT_ESCROW_RELEASED: Symbol = symbol_short!("ESC_REL");
pub const EVT_ESCROW_CANCELLED: Symbol = symbol_short!("ESC_CAN");
pub const EVT_CONFIG_UPDATED: Symbol = symbol_short!("CFG_UPD");
pub const EVT_ADMIN_CHANGED: Symbol = symbol_short!("ADM_CHG");

/// Storage key that holds a per-event running counter.
#[contracttype]
#[derive(Clone)]
pub enum AnalyticsKey {
    EventCount(Symbol),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Increments the counter for `event` by one.
pub fn record(env: &Env, event: Symbol) {
    let key = AnalyticsKey::EventCount(event.clone());
    let count: u64 = env
        .storage()
        .instance()
        .get(&key)
        .unwrap_or(0u64);
    env.storage().instance().set(&key, &(count + 1));

    // Also emit a Soroban diagnostic event so the event can be indexed off-chain.
    env.events()
        .publish((symbol_short!("analytics"), event), count + 1);
}

/// Returns the number of times `event` has been recorded.
pub fn count(env: &Env, event: Symbol) -> u64 {
    env.storage()
        .instance()
        .get(&AnalyticsKey::EventCount(event))
        .unwrap_or(0u64)
}
