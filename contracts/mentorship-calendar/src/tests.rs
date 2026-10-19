use crate::{
    BusyWindow, ConnectionInput, ConnectionStatus, ContractError, MentorshipCalendarContract,
    MentorshipCalendarContractClient, Provider, SCOPE_FREE_BUSY, SCOPE_TIMEZONE,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol, Vec,
};

const HOUR: u64 = 3_600;
const DAY: u64 = 86_400;
/// A fixed reference instant to keep expectations readable.
const NOW: u64 = 1_767_571_200;

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

struct Setup {
    env: Env,
    contract_id: Address,
    owner: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(MentorshipCalendarContract, ());
    let owner = Address::generate(&env);
    Setup {
        env,
        contract_id,
        owner,
    }
}

fn client<'a>(env: &'a Env, contract_id: &Address) -> MentorshipCalendarContractClient<'a> {
    MentorshipCalendarContractClient::new(env, contract_id)
}

fn windows(env: &Env, specs: &[(u64, u64)]) -> Vec<BusyWindow> {
    let mut v = Vec::new(env);
    for (start, end) in specs.iter() {
        v.push_back(BusyWindow {
            start_ts: *start,
            end_ts: *end,
        });
    }
    v
}

fn input(
    env: &Env,
    id_byte: u8,
    scopes: u32,
    commitment_byte: u8,
    expires_at: u64,
) -> ConnectionInput {
    ConnectionInput {
        id: hash(env, id_byte),
        provider: Provider::Google,
        scopes,
        token_commitment: hash(env, commitment_byte),
        token_ref: hash(env, 51),
        expires_at,
    }
}

fn connect(s: &Setup, id_byte: u8) -> BytesN<32> {
    let spec = input(
        &s.env,
        id_byte,
        SCOPE_FREE_BUSY | SCOPE_TIMEZONE,
        50,
        NOW + 30 * DAY,
    );
    client(&s.env, &s.contract_id).connect(&s.owner, &spec);
    spec.id
}

// ── connection lifecycle ───────────────────────────────────────────────────

#[test]
fn test_connect_stores_only_commitments() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    let connection = c.get_connection(&s.owner, &id);
    assert_eq!(connection.status, ConnectionStatus::Active);
    assert_eq!(connection.provider, Provider::Google);
    assert_eq!(connection.scopes, SCOPE_FREE_BUSY | SCOPE_TIMEZONE);
    assert_eq!(connection.token_commitment, hash(&s.env, 50));
    assert_eq!(connection.token_ref, hash(&s.env, 51));
    assert_eq!(connection.sync_epoch, 0);
    assert!(!connection.degraded);
    assert!(connection.last_error_code.is_none());
    assert_eq!(c.connection_ids(&s.owner).len(), 1);
}

#[test]
fn test_connect_rejects_content_scopes_and_bad_input() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);

    // A content scope (titles/attendees/descriptions) is never allowed.
    assert_eq!(
        c.try_connect(
            &s.owner,
            &input(&s.env, 2, SCOPE_FREE_BUSY | 8, 50, NOW + DAY),
        ),
        Err(Ok(ContractError::ScopeNotAllowed))
    );
    // An unknown bit is rejected too.
    assert_eq!(
        c.try_connect(
            &s.owner,
            &input(&s.env, 2, SCOPE_FREE_BUSY | 64, 50, NOW + DAY),
        ),
        Err(Ok(ContractError::ScopeNotAllowed))
    );
    // Free/busy is mandatory.
    assert_eq!(
        c.try_connect(&s.owner, &input(&s.env, 2, SCOPE_TIMEZONE, 50, NOW + DAY)),
        Err(Ok(ContractError::InvalidScope))
    );
    assert_eq!(
        c.try_connect(&s.owner, &input(&s.env, 2, SCOPE_FREE_BUSY, 0, NOW + DAY),),
        Err(Ok(ContractError::InvalidTokenCommitment))
    );
    assert_eq!(
        c.try_connect(&s.owner, &input(&s.env, 2, SCOPE_FREE_BUSY, 50, NOW)),
        Err(Ok(ContractError::InvalidExpiry))
    );

    // Free/busy alone is a valid scope set and is stored as given.
    c.connect(&s.owner, &input(&s.env, 2, SCOPE_FREE_BUSY, 50, NOW + DAY));
    assert_eq!(
        c.get_connection(&s.owner, &hash(&s.env, 2)).scopes,
        SCOPE_FREE_BUSY
    );
}

#[test]
fn test_connection_count_is_bounded_and_ids_unique() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    connect(&s, 1);
    assert_eq!(
        c.try_connect(&s.owner, &input(&s.env, 1, SCOPE_FREE_BUSY, 50, NOW + DAY),),
        Err(Ok(ContractError::ConnectionAlreadyExists))
    );

    for index in 2..9u8 {
        connect(&s, index);
    }
    assert_eq!(c.connection_ids(&s.owner).len(), 8);
    assert_eq!(
        c.try_connect(&s.owner, &input(&s.env, 9, SCOPE_FREE_BUSY, 50, NOW + DAY),),
        Err(Ok(ContractError::TooManyConnections))
    );
}

#[test]
fn test_connections_are_scoped_to_their_owner() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    let stranger = Address::generate(&s.env);
    assert_eq!(
        c.try_get_connection(&stranger, &id),
        Err(Ok(ContractError::ConnectionNotFound))
    );
    assert_eq!(c.connection_ids(&stranger).len(), 0);
}

// ── sync is replay-safe and normalized ─────────────────────────────────────

#[test]
fn test_record_busy_normalizes_and_replaces_atomically() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    // Overlapping and unsorted windows are merged into a canonical list.
    c.record_busy(
        &s.owner,
        &id,
        &1,
        &hash(&s.env, 60),
        &windows(
            &s.env,
            &[
                (NOW + 5 * HOUR, NOW + 7 * HOUR),
                (NOW + 6 * HOUR, NOW + 8 * HOUR),
                (NOW + 8 * HOUR, NOW + 9 * HOUR),
                (NOW + 2 * HOUR, NOW + 3 * HOUR),
            ],
        ),
    );

    let busy = c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10);
    assert_eq!(busy.len(), 2);
    assert_eq!(busy.get(0).unwrap().start_ts, NOW + 2 * HOUR);
    assert_eq!(busy.get(0).unwrap().end_ts, NOW + 3 * HOUR);
    assert_eq!(busy.get(1).unwrap().start_ts, NOW + 5 * HOUR);
    assert_eq!(busy.get(1).unwrap().end_ts, NOW + 9 * HOUR);

    // A newer epoch replaces the previous snapshot entirely.
    c.record_busy(
        &s.owner,
        &id,
        &2,
        &hash(&s.env, 61),
        &windows(&s.env, &[(NOW + 20 * HOUR, NOW + 21 * HOUR)]),
    );
    let busy = c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10);
    assert_eq!(busy.len(), 1);
    assert_eq!(busy.get(0).unwrap().start_ts, NOW + 20 * HOUR);
    let connection = c.get_connection(&s.owner, &id);
    assert_eq!(connection.sync_epoch, 2);
    assert_eq!(connection.payload_hash, hash(&s.env, 61));
    assert_eq!(connection.last_synced_at, NOW);
}

#[test]
fn test_identical_retry_is_a_noop_and_older_epoch_is_rejected() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    let payload = hash(&s.env, 60);
    let first = windows(&s.env, &[(NOW + 5 * HOUR, NOW + 6 * HOUR)]);
    c.record_busy(&s.owner, &id, &1, &payload, &first);

    // Exact retry of the same epoch and payload: no duplicate effect, no
    // error, and the stored snapshot is untouched.
    s.env.ledger().set_timestamp(NOW + 60);
    c.record_busy(&s.owner, &id, &1, &payload, &first);
    let connection = c.get_connection(&s.owner, &id);
    assert_eq!(connection.sync_epoch, 1);
    assert_eq!(connection.last_synced_at, NOW);
    assert_eq!(
        c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10)
            .len(),
        1
    );

    // Same epoch with a different payload is a stale/conflicting sync.
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 99),
            &windows(&s.env, &[(NOW + 9 * HOUR, NOW + 10 * HOUR)]),
        ),
        Err(Ok(ContractError::StaleSyncEpoch))
    );
    // An older epoch is rejected outright.
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &0,
            &hash(&s.env, 98),
            &windows(&s.env, &[(NOW + 9 * HOUR, NOW + 10 * HOUR)]),
        ),
        Err(Ok(ContractError::StaleSyncEpoch))
    );

    // A newer epoch with a new payload is accepted.
    c.record_busy(
        &s.owner,
        &id,
        &2,
        &hash(&s.env, 97),
        &windows(&s.env, &[(NOW + 9 * HOUR, NOW + 10 * HOUR)]),
    );
    assert_eq!(c.get_connection(&s.owner, &id).sync_epoch, 2);
    assert_eq!(
        c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10)
            .get(0)
            .unwrap()
            .start_ts,
        NOW + 9 * HOUR
    );
}

#[test]
fn test_record_busy_validation() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(0, HOUR)]),
        ),
        Err(Ok(ContractError::InvalidBusyWindow))
    );
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(NOW + HOUR, NOW + HOUR)]),
        ),
        Err(Ok(ContractError::InvalidBusyWindow))
    );
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(NOW + HOUR, NOW + 8 * DAY)]),
        ),
        Err(Ok(ContractError::BusyWindowSpanTooLarge))
    );
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(NOW + 61 * DAY, NOW + 61 * DAY + HOUR)]),
        ),
        Err(Ok(ContractError::InvalidSyncRange))
    );

    // More windows than the bound allows.
    let mut too_many: Vec<BusyWindow> = Vec::new(&s.env);
    for index in 0..129u64 {
        too_many.push_back(BusyWindow {
            start_ts: NOW + index * 10 * 60,
            end_ts: NOW + index * 10 * 60 + 60,
        });
    }
    assert_eq!(
        c.try_record_busy(&s.owner, &id, &1, &hash(&s.env, 60), &too_many),
        Err(Ok(ContractError::TooManyBusyWindows))
    );
    assert_eq!(
        c.try_record_busy(
            &Address::generate(&s.env),
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(NOW + HOUR, NOW + 2 * HOUR)]),
        ),
        Err(Ok(ContractError::ConnectionNotFound))
    );
}

// ── graceful degradation ───────────────────────────────────────────────────

#[test]
fn test_failed_sync_keeps_busy_windows_and_marks_degraded() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    c.record_busy(
        &s.owner,
        &id,
        &1,
        &hash(&s.env, 60),
        &windows(&s.env, &[(NOW + 5 * HOUR, NOW + 6 * HOUR)]),
    );
    assert!(c.connection_healthy(&s.owner, &id));

    c.mark_sync_failed(&s.owner, &id, &Symbol::new(&s.env, "PROVIDER_5XX"));
    let connection = c.get_connection(&s.owner, &id);
    assert!(connection.degraded);
    assert_eq!(
        connection.last_error_code,
        Some(Symbol::new(&s.env, "PROVIDER_5XX"))
    );
    assert!(!c.connection_healthy(&s.owner, &id));
    // The last known busy windows survive the outage.
    assert_eq!(
        c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10)
            .len(),
        1
    );

    // The next successful sync clears the degraded flag.
    c.record_busy(
        &s.owner,
        &id,
        &2,
        &hash(&s.env, 61),
        &windows(&s.env, &[(NOW + 7 * HOUR, NOW + 8 * HOUR)]),
    );
    let connection = c.get_connection(&s.owner, &id);
    assert!(!connection.degraded);
    assert!(connection.last_error_code.is_none());
    assert!(c.connection_healthy(&s.owner, &id));
}

#[test]
fn test_expired_token_stops_sync_and_flags_the_connection() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    // Import one busy window while the token is still valid.
    c.record_busy(
        &s.owner,
        &id,
        &1,
        &hash(&s.env, 60),
        &windows(&s.env, &[(NOW + 5 * HOUR, NOW + 6 * HOUR)]),
    );
    s.env.ledger().set_timestamp(NOW + 31 * DAY);

    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &1,
            &hash(&s.env, 60),
            &windows(&s.env, &[(NOW + 32 * DAY, NOW + 32 * DAY + HOUR)]),
        ),
        Err(Ok(ContractError::ConnectionExpired))
    );
    // A failed sync rolls the whole invocation back, so the connection keeps
    // its stored status while `connection_healthy` reports it unhealthy.
    assert_eq!(
        c.get_connection(&s.owner, &id).status,
        ConnectionStatus::Active
    );
    assert!(!c.connection_healthy(&s.owner, &id));

    // The owner (or the off-chain worker) then retires the connection, which
    // deletes the only busy window it ever had.
    c.expire_connection(&s.owner, &id);
    let connection = c.get_connection(&s.owner, &id);
    assert_eq!(connection.status, ConnectionStatus::Expired);
    assert_eq!(
        c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10)
            .len(),
        0
    );
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &2,
            &hash(&s.env, 61),
            &windows(&s.env, &[(NOW + 32 * DAY, NOW + 32 * DAY + HOUR)]),
        ),
        Err(Ok(ContractError::ConnectionNotActive))
    );
    assert_eq!(
        c.try_expire_connection(&s.owner, &id),
        Err(Ok(ContractError::ConnectionNotActive))
    );
}

#[test]
fn test_expire_connection_requires_a_past_expiry() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    assert_eq!(
        c.try_expire_connection(&s.owner, &id),
        Err(Ok(ContractError::ConnectionNotExpired))
    );

    s.env.ledger().set_timestamp(NOW + 31 * DAY);
    c.expire_connection(&s.owner, &id);
    assert_eq!(
        c.get_connection(&s.owner, &id).status,
        ConnectionStatus::Expired
    );
}

// ── disconnect removes future access ───────────────────────────────────────

#[test]
fn test_disconnect_revokes_and_deletes_busy_windows() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    c.record_busy(
        &s.owner,
        &id,
        &1,
        &hash(&s.env, 60),
        &windows(&s.env, &[(NOW + 5 * HOUR, NOW + 6 * HOUR)]),
    );

    c.disconnect(&s.owner, &id);
    let connection = c.get_connection(&s.owner, &id);
    assert_eq!(connection.status, ConnectionStatus::Revoked);
    assert_eq!(connection.revoked_at, NOW);
    assert_eq!(
        c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &10)
            .len(),
        0
    );
    assert_eq!(
        c.try_record_busy(
            &s.owner,
            &id,
            &2,
            &hash(&s.env, 61),
            &windows(&s.env, &[(NOW + 9 * HOUR, NOW + 10 * HOUR)]),
        ),
        Err(Ok(ContractError::ConnectionNotActive))
    );
    assert_eq!(
        c.try_disconnect(&s.owner, &id),
        Err(Ok(ContractError::ConnectionNotActive))
    );
    assert!(!c.connection_healthy(&s.owner, &id));
}

#[test]
fn test_reconnect_after_disconnect_starts_clean() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    c.record_busy(
        &s.owner,
        &id,
        &5,
        &hash(&s.env, 60),
        &windows(&s.env, &[(NOW + 5 * HOUR, NOW + 6 * HOUR)]),
    );
    c.disconnect(&s.owner, &id);

    // Connectors are immutable today, so a fresh connection id is used.
    let fresh = connect(&s, 2);
    assert_eq!(
        c.busy_windows(&s.owner, &fresh, &NOW, &(NOW + DAY), &0, &10)
            .len(),
        0
    );
    let connection = c.get_connection(&s.owner, &fresh);
    assert_eq!(connection.sync_epoch, 0);
    assert_eq!(connection.status, ConnectionStatus::Active);
}

// ── queries ────────────────────────────────────────────────────────────────

#[test]
fn test_busy_windows_clip_and_paginate() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    c.record_busy(
        &s.owner,
        &id,
        &1,
        &hash(&s.env, 60),
        &windows(
            &s.env,
            &[
                (NOW + HOUR, NOW + 3 * HOUR),
                (NOW + 5 * HOUR, NOW + 6 * HOUR),
                (NOW + 7 * HOUR, NOW + 8 * HOUR),
            ],
        ),
    );

    // A query window that starts inside a busy block clips its start.
    let clipped = c.busy_windows(&s.owner, &id, &(NOW + 2 * HOUR), &(NOW + 7 * HOUR), &0, &10);
    assert_eq!(clipped.len(), 2);
    assert_eq!(clipped.get(0).unwrap().start_ts, NOW + 2 * HOUR);
    assert_eq!(clipped.get(0).unwrap().end_ts, NOW + 3 * HOUR);
    assert_eq!(clipped.get(1).unwrap().start_ts, NOW + 5 * HOUR);
    assert_eq!(clipped.get(1).unwrap().end_ts, NOW + 6 * HOUR);

    let page_zero = c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &2);
    assert_eq!(page_zero.len(), 2);
    let page_one = c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &1, &2);
    assert_eq!(page_one.len(), 1);
    assert_eq!(page_one.get(0).unwrap().start_ts, NOW + 7 * HOUR);
    let page_two = c.busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &2, &2);
    assert_eq!(page_two.len(), 0);
}

#[test]
fn test_query_validation() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);

    assert_eq!(
        c.try_busy_windows(&s.owner, &id, &NOW, &NOW, &0, &10),
        Err(Ok(ContractError::InvalidQueryRange))
    );
    assert_eq!(
        c.try_busy_windows(&s.owner, &id, &NOW, &(NOW + 91 * DAY), &0, &10),
        Err(Ok(ContractError::QuerySpanTooLarge))
    );
    assert_eq!(
        c.try_busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &0),
        Err(Ok(ContractError::InvalidPage))
    );
    assert_eq!(
        c.try_busy_windows(&s.owner, &id, &NOW, &(NOW + DAY), &0, &65),
        Err(Ok(ContractError::InvalidPage))
    );
    assert_eq!(
        c.try_busy_windows(&s.owner, &hash(&s.env, 77), &NOW, &(NOW + DAY), &0, &10),
        Err(Ok(ContractError::ConnectionNotFound))
    );
}

#[test]
fn test_connection_healthy_requires_fresh_state() {
    let s = setup();
    let c = client(&s.env, &s.contract_id);
    let id = connect(&s, 1);
    assert!(c.connection_healthy(&s.owner, &id));
    assert!(!c.connection_healthy(&Address::generate(&s.env), &id));
    s.env.ledger().set_timestamp(NOW + 31 * DAY);
    assert!(!c.connection_healthy(&s.owner, &id));
}

#[test]
fn test_version() {
    let s = setup();
    assert_eq!(client(&s.env, &s.contract_id).version(), 1);
}
