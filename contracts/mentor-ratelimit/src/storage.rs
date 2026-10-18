use mentor_types::AbuseSignals;
use soroban_sdk::{Address, Env, Symbol};

pub const MIN_TTL: u32 = 1_000_000;
pub const MAX_TTL: u32 = 17_280_000;

pub enum DataKey {
    Admin,
    RateLimitConfig(u8), // limit_type
    RequestCount(Address, u8, u64), // account, limit_type, window_start
    AbuseSignals(Address),
}

impl DataKey {
    pub fn to_symbol(&self) -> Symbol {
        match self {
            DataKey::Admin => Symbol::new(&Env::new(), "admin"),
            DataKey::RateLimitConfig(limit_type) => {
                Symbol::new(&Env::new(), &format!("rl_cfg:{}", limit_type))
            }
            DataKey::RequestCount(addr, limit_type, window_start) => {
                Symbol::new(&Env::new(), &format!("req:{}:{}:{}", addr, limit_type, window_start))
            }
            DataKey::AbuseSignals(addr) => {
                Symbol::new(&Env::new(), &format!("abuse:{}", addr))
            }
        }
    }
}

pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage()
        .instance()
        .get::<Symbol, Option<Address>>(&DataKey::Admin.to_symbol())
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set::<Symbol, Option<Address>>(&DataKey::Admin.to_symbol(), &Some(admin.clone()));
}

pub fn require_admin(env: &Env, caller: &Address) -> Result<(), soroban_sdk::contracterror::ContractError> {
    let admin = get_admin(env).ok_or_else(|| {
        soroban_sdk::contracterror::ContractError::from_type_and_code(1, 1)
    })?;

    if admin != *caller {
        return Err(soroban_sdk::contracterror::ContractError::from_type_and_code(1, 3));
    }

    Ok(())
}

pub fn get_rate_limit(env: &Env, limit_type: u8) -> (u32, u32) {
    let key = DataKey::RateLimitConfig(limit_type).to_symbol();
    env.storage()
        .instance()
        .get::<Symbol, Option<(u32, u32)>>(&key)
        .flatten()
        .unwrap_or((10, 60)) // Default fallback
}

pub fn set_rate_limit(env: &Env, limit_type: u8, max_requests: u32, window_seconds: u32) {
    let key = DataKey::RateLimitConfig(limit_type).to_symbol();
    env.storage()
        .instance()
        .set::<Symbol, Option<(u32, u32)>>(&key, &Some((max_requests, window_seconds)));
}

pub fn get_request_count(
    env: &Env,
    account: &Address,
    limit_type: u8,
    window_start: u64,
) -> u32 {
    let key = DataKey::RequestCount(account.clone(), limit_type, window_start).to_symbol();
    env.storage()
        .instance()
        .get::<Symbol, Option<u32>>(&key)
        .flatten()
        .unwrap_or(0)
}

pub fn increment_request_count(
    env: &Env,
    account: &Address,
    limit_type: u8,
    timestamp: u64,
) {
    let (_max_requests, window_seconds) = get_rate_limit(env, limit_type);
    let window_start = timestamp.saturating_sub(window_seconds as u64);

    let key = DataKey::RequestCount(account.clone(), limit_type, window_start).to_symbol();
    let current = get_request_count(env, account, limit_type, window_start);

    env.storage()
        .instance()
        .set::<Symbol, Option<u32>>(&key, &Some(current + 1));
}

pub fn get_abuse_signals(env: &Env, account: &Address) -> AbuseSignals {
    let key = DataKey::AbuseSignals(account.clone()).to_symbol();
    env.storage()
        .instance()
        .get::<Symbol, Option<AbuseSignals>>(&key)
        .flatten()
        .unwrap_or(AbuseSignals {
            rapid_queries: false,
            enumeration_pattern: false,
            cache_bypass_attempts: false,
            risk_score: 0,
        })
}

pub fn increment_abuse_signal(env: &Env, account: &Address, signal_type: u8) {
    let mut signals = get_abuse_signals(env, account);

    match signal_type {
        0 => signals.rapid_queries = true,
        1 => signals.enumeration_pattern = true,
        2 => signals.cache_bypass_attempts = true,
        _ => {}
    }

    // Increase risk score
    signals.risk_score = signals.risk_score.saturating_add(25).min(100);

    let key = DataKey::AbuseSignals(account.clone()).to_symbol();
    env.storage()
        .instance()
        .set::<Symbol, Option<AbuseSignals>>(&key, &Some(signals));
}

pub fn clear_abuse_signals(env: &Env, account: &Address) {
    let key = DataKey::AbuseSignals(account.clone()).to_symbol();
    env.storage()
        .instance()
        .set::<Symbol, Option<AbuseSignals>>(&key, &None);
}
