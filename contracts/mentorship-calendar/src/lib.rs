#![no_std]

//! Mentorship external-calendar availability contract (work item MENTOR-054,
//! issue #1250).
//!
//! The contract lets a participant connect an external calendar provider and
//! import free/busy time, while keeping the calendar itself private:
//!
//! - **No event content, ever.** A connection stores only a provider, a
//!   scope bitmask, a `token_commitment` (hash over the encrypted token), a
//!   `token_ref` handle to the ciphertext in the off-chain secret vault, and
//!   sync bookkeeping. Event titles, attendees, descriptions, and the token
//!   itself are never written on-chain — `BusyWindow` is two timestamps.
//! - **Scoped and encrypted.** The scope bitmask may only contain free/busy
//!   and time-zone scopes; any content scope is rejected with
//!   `ScopeNotAllowed`. The token is stored as a commitment plus a vault
//!   reference so a leaked ledger cannot decrypt anything.
//! - **Replay-safe sync.** Each sync carries a monotonically increasing
//!   `epoch` and the hash of its payload. Retrying the exact same sync is a
//!   no-op, an older or equal epoch with a different payload is rejected
//!   (`StaleSyncEpoch`), and an accepted sync replaces the connection's busy
//!   windows atomically, so a retry can never duplicate busy time.
//! - **Failures degrade gracefully.** `mark_sync_failed` records a degraded
//!   flag and an error code while keeping the last known busy windows, so a
//!   provider outage removes nothing; a successful sync clears the flag.
//! - **Disconnect removes future access.** `disconnect` revokes the
//!   connection and deletes its busy windows immediately.
//!
//! See `contracts/docs/mentorship-calendar.md` for the off-chain boundary,
//! privacy notes, and rollout guidance.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

const CONTRACT_VERSION: u32 = 1;

// TTL constants: ~1 year at 6-second ledgers, matching the sibling contracts.
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;

const SECONDS_PER_DAY: u64 = 86_400;

/// Scope: the provider returns free/busy intervals only. Required.
pub const SCOPE_FREE_BUSY: u32 = 1;
/// Scope: the provider returns the calendar's time zone. Optional.
pub const SCOPE_TIMEZONE: u32 = 2;
/// Scopes this contract will ever accept.
pub const ALLOWED_SCOPES: u32 = SCOPE_FREE_BUSY | SCOPE_TIMEZONE;
/// Content scopes (event titles, attendees, descriptions) are forbidden: the
/// contract must never be granted access to them.
const FORBIDDEN_CONTENT_SCOPE: u32 = 8;

/// Bounded storage: at most this many connections per owner.
const MAX_CONNECTIONS_PER_OWNER: u32 = 8;
/// Bounded storage: at most this many busy windows per connection.
const MAX_BUSY_WINDOWS: u32 = 128;
/// A single busy window may not span more than 7 days.
const MAX_WINDOW_SPAN_SECONDS: u64 = 7 * SECONDS_PER_DAY;
/// A sync may not publish busy time further than this ahead of the ledger.
const MAX_SYNC_HORIZON_DAYS: u64 = 60;
/// One query may cover at most 90 days.
const MAX_QUERY_SPAN_SECONDS: u64 = 90 * SECONDS_PER_DAY;
const MAX_PAGE_SIZE: u32 = 64;
const MAX_PAGE: u32 = 4_096;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    ConnectionNotFound = 1,
    ConnectionAlreadyExists = 2,
    TooManyConnections = 3,
    /// The free/busy scope is required and unknown bits are rejected.
    InvalidScope = 4,
    /// A content scope (titles, attendees, descriptions) was requested.
    ScopeNotAllowed = 5,
    /// The token commitment must not be all zeroes.
    InvalidTokenCommitment = 6,
    /// `expires_at` must be in the future.
    InvalidExpiry = 7,
    /// The connection is revoked or expired.
    ConnectionNotActive = 8,
    /// The stored token expired; reconnect with a fresh one.
    ConnectionExpired = 9,
    /// The sync epoch is older than the last accepted one.
    StaleSyncEpoch = 10,
    TooManyBusyWindows = 11,
    /// A busy window must satisfy `0 < start_ts < until_ts`.
    InvalidBusyWindow = 12,
    BusyWindowSpanTooLarge = 13,
    /// A busy window falls outside the permitted sync horizon.
    InvalidSyncRange = 14,
    InvalidPage = 15,
    InvalidQueryRange = 16,
    QuerySpanTooLarge = 17,
    /// `expire_connection` can only be called once the token has expired.
    ConnectionNotExpired = 18,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Google,
    Outlook,
    Calendly,
    Apple,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Active,
    Revoked,
    Expired,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Connection ids of an owner, in connection order.
    Connections(Address),
    /// One connection, keyed by (owner, id).
    Connection(Address, BytesN<32>),
    /// Normalized busy windows of a connection, keyed by (owner, id).
    Busy(Address, BytesN<32>),
}

/// Caller-supplied connection details.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionInput {
    pub id: BytesN<32>,
    pub provider: Provider,
    /// Bitmask drawn from `ALLOWED_SCOPES` only.
    pub scopes: u32,
    /// Commitment over the encrypted provider token.
    pub token_commitment: BytesN<32>,
    /// Vault handle for the ciphertext.
    pub token_ref: BytesN<32>,
    pub expires_at: u64,
}

/// A connection to an external calendar provider.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarConnection {
    pub id: BytesN<32>,
    pub owner: Address,
    pub provider: Provider,
    /// Bitmask drawn from `ALLOWED_SCOPES` only.
    pub scopes: u32,
    /// Commitment over the encrypted provider token; the ciphertext lives in
    /// the off-chain secret vault, never on-chain.
    pub token_commitment: BytesN<32>,
    /// Opaque handle the off-chain worker uses to fetch the ciphertext.
    pub token_ref: BytesN<32>,
    pub status: ConnectionStatus,
    pub connected_at: u64,
    pub last_synced_at: u64,
    /// Highest accepted sync epoch.
    pub sync_epoch: u64,
    /// Hash of the payload of the highest accepted sync epoch.
    pub payload_hash: BytesN<32>,
    /// Token expiry; syncs after this instant are rejected.
    pub expires_at: u64,
    /// True while the last sync attempt failed. Busy windows are preserved.
    pub degraded: bool,
    pub last_error_code: Option<Symbol>,
    pub revoked_at: u64,
}

/// A free/busy interval. Contains no calendar content by construction.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BusyWindow {
    pub start_ts: u64,
    pub end_ts: u64,
}

#[contract]
pub struct MentorshipCalendarContract;

#[contractimpl]
impl MentorshipCalendarContract {
    // ── helpers ──────────────────────────────────────────────────────────

    fn load_ids(env: &Env, owner: &Address) -> Vec<BytesN<32>> {
        let key = DataKey::Connections(owner.clone());
        let ids: Vec<BytesN<32>> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if !ids.is_empty() {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        ids
    }

    fn put_ids(env: &Env, owner: &Address, ids: &Vec<BytesN<32>>) {
        let key = DataKey::Connections(owner.clone());
        env.storage().persistent().set(&key, ids);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_connection(
        env: &Env,
        owner: &Address,
        id: &BytesN<32>,
    ) -> Result<CalendarConnection, ContractError> {
        let key = DataKey::Connection(owner.clone(), id.clone());
        let connection: CalendarConnection = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::ConnectionNotFound)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        Ok(connection)
    }

    fn put_connection(env: &Env, connection: &CalendarConnection) {
        let key = DataKey::Connection(connection.owner.clone(), connection.id.clone());
        env.storage().persistent().set(&key, connection);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn load_busy(env: &Env, owner: &Address, id: &BytesN<32>) -> Vec<BusyWindow> {
        let key = DataKey::Busy(owner.clone(), id.clone());
        let windows: Vec<BusyWindow> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        if !windows.is_empty() {
            env.storage()
                .persistent()
                .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
        }
        windows
    }

    fn put_busy(env: &Env, owner: &Address, id: &BytesN<32>, windows: &Vec<BusyWindow>) {
        let key = DataKey::Busy(owner.clone(), id.clone());
        if windows.is_empty() {
            env.storage().persistent().remove(&key);
            return;
        }
        env.storage().persistent().set(&key, windows);
        env.storage()
            .persistent()
            .extend_ttl(&key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }

    fn require_scope(scopes: u32) -> Result<(), ContractError> {
        if scopes & FORBIDDEN_CONTENT_SCOPE != 0 || scopes & !ALLOWED_SCOPES != 0 {
            return Err(ContractError::ScopeNotAllowed);
        }
        if scopes & SCOPE_FREE_BUSY == 0 {
            return Err(ContractError::InvalidScope);
        }
        Ok(())
    }

    /// Deterministically sort and merge busy windows into a canonical list.
    fn normalize(env: &Env, windows: &Vec<BusyWindow>) -> Vec<BusyWindow> {
        let mut sorted: Vec<BusyWindow> = Vec::new(env);
        for window in windows.iter() {
            sorted.push_back(window);
        }
        // Selection sort: the window count is bounded by MAX_BUSY_WINDOWS.
        let count = sorted.len();
        for i in 0..count {
            let mut min_index = i;
            let mut min_value = match sorted.get(i) {
                Some(value) => value,
                None => continue,
            };
            for j in (i + 1)..count {
                if let Some(value) = sorted.get(j) {
                    if value.start_ts < min_value.start_ts {
                        min_index = j;
                        min_value = value;
                    }
                }
            }
            if min_index != i {
                if let Some(left) = sorted.get(i) {
                    if let Some(right) = sorted.get(min_index) {
                        sorted.set(i, right);
                        sorted.set(min_index, left);
                    }
                }
            }
        }

        let mut merged: Vec<BusyWindow> = Vec::new(env);
        for window in sorted.iter() {
            match merged.last() {
                Some(last) if window.start_ts <= last.end_ts => {
                    if window.end_ts > last.end_ts {
                        merged.set(
                            merged.len() - 1,
                            BusyWindow {
                                start_ts: last.start_ts,
                                end_ts: window.end_ts,
                            },
                        );
                    }
                }
                _ => merged.push_back(window),
            }
        }
        merged
    }

    /// Clip a canonical window list to an interval, splitting at the bounds.
    fn clip_windows(
        env: &Env,
        windows: &Vec<BusyWindow>,
        from_ts: u64,
        until_ts: u64,
    ) -> Vec<BusyWindow> {
        let mut out: Vec<BusyWindow> = Vec::new(env);
        for window in windows.iter() {
            let start = if window.start_ts > from_ts {
                window.start_ts
            } else {
                from_ts
            };
            let end = if window.end_ts < until_ts {
                window.end_ts
            } else {
                until_ts
            };
            if end > start {
                out.push_back(BusyWindow {
                    start_ts: start,
                    end_ts: end,
                });
            }
        }
        out
    }

    // ── MENTOR-054 — provider connections ────────────────────────────────

    /// Connect an external calendar. The owner authorizes; the token is
    /// supplied only as a commitment plus a vault reference.
    pub fn connect(env: Env, owner: Address, input: ConnectionInput) -> Result<(), ContractError> {
        owner.require_auth();

        if input.token_commitment == BytesN::from_array(&env, &[0u8; 32]) {
            return Err(ContractError::InvalidTokenCommitment);
        }
        Self::require_scope(input.scopes)?;
        if input.expires_at <= env.ledger().timestamp() {
            return Err(ContractError::InvalidExpiry);
        }

        let key = DataKey::Connection(owner.clone(), input.id.clone());
        if env.storage().persistent().has(&key) {
            return Err(ContractError::ConnectionAlreadyExists);
        }

        let mut ids = Self::load_ids(&env, &owner);
        if ids.len() >= MAX_CONNECTIONS_PER_OWNER {
            return Err(ContractError::TooManyConnections);
        }

        let now = env.ledger().timestamp();
        let connection = CalendarConnection {
            id: input.id.clone(),
            owner: owner.clone(),
            provider: input.provider,
            scopes: input.scopes,
            token_commitment: input.token_commitment,
            token_ref: input.token_ref,
            status: ConnectionStatus::Active,
            connected_at: now,
            last_synced_at: 0,
            sync_epoch: 0,
            payload_hash: BytesN::from_array(&env, &[0u8; 32]),
            expires_at: input.expires_at,
            degraded: false,
            last_error_code: None,
            revoked_at: 0,
        };
        Self::put_connection(&env, &connection);

        ids.push_back(input.id.clone());
        Self::put_ids(&env, &owner, &ids);

        env.events().publish(
            (symbol_short!("CALCONN"),),
            (owner, input.id, input.provider),
        );
        Ok(())
    }

    /// Import a full free/busy snapshot for a connection.
    ///
    /// Accepted only when `epoch` is strictly newer than the last accepted
    /// epoch, or when it repeats the last epoch with the same payload hash
    /// (an idempotent replay that changes nothing). Accepted snapshots
    /// replace the previous windows atomically.
    pub fn record_busy(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
        epoch: u64,
        payload_hash: BytesN<32>,
        windows: Vec<BusyWindow>,
    ) -> Result<(), ContractError> {
        owner.require_auth();

        let mut connection = Self::load_connection(&env, &owner, &connection_id)?;
        if connection.status != ConnectionStatus::Active {
            return Err(ContractError::ConnectionNotActive);
        }
        let now = env.ledger().timestamp();
        if connection.expires_at <= now {
            // Fail closed. The expiry is authoritative on-chain (the
            // timestamp is already stored), so no state write is needed here;
            // the owner or worker can call `expire_connection` to mark the
            // status and delete the stale busy windows.
            return Err(ContractError::ConnectionExpired);
        }

        // Replay protection: identical retry is a no-op, older epoch fails.
        if epoch == connection.sync_epoch && payload_hash == connection.payload_hash {
            return Ok(());
        }
        if epoch <= connection.sync_epoch {
            return Err(ContractError::StaleSyncEpoch);
        }

        if windows.len() > MAX_BUSY_WINDOWS {
            return Err(ContractError::TooManyBusyWindows);
        }
        let horizon_end = now
            .checked_add(MAX_SYNC_HORIZON_DAYS * SECONDS_PER_DAY)
            .unwrap_or(u64::MAX);
        for window in windows.iter() {
            if window.start_ts == 0 || window.start_ts >= window.end_ts {
                return Err(ContractError::InvalidBusyWindow);
            }
            if window.end_ts - window.start_ts > MAX_WINDOW_SPAN_SECONDS {
                return Err(ContractError::BusyWindowSpanTooLarge);
            }
            if window.end_ts > horizon_end {
                return Err(ContractError::InvalidSyncRange);
            }
        }

        let normalized = Self::normalize(&env, &windows);
        if normalized.len() > MAX_BUSY_WINDOWS {
            return Err(ContractError::TooManyBusyWindows);
        }
        Self::put_busy(&env, &owner, &connection_id, &normalized);

        connection.sync_epoch = epoch;
        connection.payload_hash = payload_hash;
        connection.last_synced_at = now;
        connection.degraded = false;
        connection.last_error_code = None;
        Self::put_connection(&env, &connection);

        env.events()
            .publish((symbol_short!("CALSYNC"),), (owner, connection_id, epoch));
        Ok(())
    }

    /// Record a failed sync attempt. The last known busy windows are kept so
    /// a provider outage degrades to stale-but-present data rather than
    /// silently opening the calendar.
    pub fn mark_sync_failed(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
        error_code: Symbol,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        let mut connection = Self::load_connection(&env, &owner, &connection_id)?;
        if connection.status != ConnectionStatus::Active {
            return Err(ContractError::ConnectionNotActive);
        }
        connection.degraded = true;
        connection.last_error_code = Some(error_code.clone());
        Self::put_connection(&env, &connection);

        env.events().publish(
            (symbol_short!("CALFAIL"),),
            (owner, connection_id, error_code),
        );
        Ok(())
    }

    /// Mark an expired connection as expired and delete its busy windows, so
    /// no further access remains after the token lifetime ends. Idempotent
    /// callers should treat `ConnectionNotActive` as "already done".
    pub fn expire_connection(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        let mut connection = Self::load_connection(&env, &owner, &connection_id)?;
        if connection.status != ConnectionStatus::Active {
            return Err(ContractError::ConnectionNotActive);
        }
        if connection.expires_at > env.ledger().timestamp() {
            return Err(ContractError::ConnectionNotExpired);
        }
        connection.status = ConnectionStatus::Expired;
        connection.degraded = false;
        connection.last_error_code = None;
        Self::put_connection(&env, &connection);
        Self::put_busy(&env, &owner, &connection_id, &Vec::new(&env));

        env.events()
            .publish((symbol_short!("CALEXP"),), (owner, connection_id));
        Ok(())
    }

    /// Revoke a connection. Busy windows are deleted immediately, so no
    /// future access remains once the owner disconnects.
    pub fn disconnect(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
    ) -> Result<(), ContractError> {
        owner.require_auth();
        let mut connection = Self::load_connection(&env, &owner, &connection_id)?;
        if connection.status == ConnectionStatus::Revoked {
            return Err(ContractError::ConnectionNotActive);
        }
        connection.status = ConnectionStatus::Revoked;
        connection.revoked_at = env.ledger().timestamp();
        connection.degraded = false;
        connection.last_error_code = None;
        Self::put_connection(&env, &connection);
        Self::put_busy(&env, &owner, &connection_id, &Vec::new(&env));

        env.events()
            .publish((symbol_short!("CALDISC"),), (owner, connection_id));
        Ok(())
    }

    pub fn get_connection(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
    ) -> Result<CalendarConnection, ContractError> {
        Self::load_connection(&env, &owner, &connection_id)
    }

    pub fn connection_ids(env: Env, owner: Address) -> Vec<BytesN<32>> {
        Self::load_ids(&env, &owner)
    }

    /// Normalized busy windows of one connection, clipped to
    /// `[from_ts, until_ts)`. Revoked and expired connections expose none.
    pub fn busy_windows(
        env: Env,
        owner: Address,
        connection_id: BytesN<32>,
        from_ts: u64,
        until_ts: u64,
        page: u32,
        limit: u32,
    ) -> Result<Vec<BusyWindow>, ContractError> {
        if from_ts >= until_ts {
            return Err(ContractError::InvalidQueryRange);
        }
        if until_ts - from_ts > MAX_QUERY_SPAN_SECONDS {
            return Err(ContractError::QuerySpanTooLarge);
        }
        if limit == 0 || limit > MAX_PAGE_SIZE || page > MAX_PAGE {
            return Err(ContractError::InvalidPage);
        }

        let connection = Self::load_connection(&env, &owner, &connection_id)?;
        if connection.status != ConnectionStatus::Active {
            return Ok(Vec::new(&env));
        }

        let windows = Self::load_busy(&env, &owner, &connection_id);
        let clipped = Self::clip_windows(&env, &windows, from_ts, until_ts);

        let skip = (page as u64) * (limit as u64);
        let mut out: Vec<BusyWindow> = Vec::new(&env);
        for (index, window) in clipped.iter().enumerate() {
            if (index as u64) < skip {
                continue;
            }
            if out.len() >= limit {
                break;
            }
            out.push_back(window);
        }
        Ok(out)
    }

    /// True when a connection is active, unexpired, and not degraded.
    pub fn connection_healthy(env: Env, owner: Address, connection_id: BytesN<32>) -> bool {
        match Self::load_connection(&env, &owner, &connection_id) {
            Ok(connection) => {
                connection.status == ConnectionStatus::Active
                    && !connection.degraded
                    && connection.expires_at > env.ledger().timestamp()
            }
            Err(_) => false,
        }
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }
}

#[cfg(test)]
mod tests;
