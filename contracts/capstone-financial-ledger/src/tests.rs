extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    symbol_short, Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneFinancialLedger, CapstoneFinancialLedgerClient, ContractError, EntryKind, EntryState,
    PeriodState, Role,
};

const NOW: u64 = 1_000;

struct Fixture {
    env: Env,
    client: CapstoneFinancialLedgerClient<'static>,
    admin: Address,
    coordinator: Address,
    finance: Address,
    auditor: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn id(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn seed(env: &Env, n: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0..4].copy_from_slice(&n.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneFinancialLedger, ());
    let client = CapstoneFinancialLedgerClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    let coordinator = Address::generate(&env);
    let finance = Address::generate(&env);
    let auditor = Address::generate(&env);
    client.grant_role(&admin, &tenant, &project, &coordinator, &Role::Coordinator);
    client.grant_role(&admin, &tenant, &project, &finance, &Role::Finance);
    client.grant_role(&admin, &tenant, &project, &auditor, &Role::Auditor);
    Fixture {
        env,
        client,
        admin,
        coordinator,
        finance,
        auditor,
        tenant,
        project,
    }
}

fn open_period(f: &Fixture, period_id: &BytesN<32>, request_id: &BytesN<32>, limit: u32) {
    f.client.open_period(
        &f.coordinator,
        &f.tenant,
        &f.project,
        period_id,
        request_id,
        &0,
        &NOW,
        &(NOW + 10_000),
        &limit,
    );
}

#[test]
fn all_entry_kinds_reconcile_and_close_with_typed_event() {
    let f = setup();
    let period_id = id(&f.env, 10);
    open_period(&f, &period_id, &id(&f.env, 200), 32);
    let kinds = [
        EntryKind::Intent,
        EntryKind::Reservation,
        EntryKind::Hold,
        EntryKind::Release,
        EntryKind::Refund,
        EntryKind::Reimbursement,
        EntryKind::Fee,
        EntryKind::External,
    ];
    let mut version = 1u32;
    for n in 0u32..8u32 {
        f.client.record_entry(
            &f.finance,
            &f.tenant,
            &f.project,
            &period_id,
            &seed(&f.env, 100 + n),
            &seed(&f.env, 200 + n),
            &version,
            &kinds[n as usize],
            &100,
            &seed(&f.env, 300 + n),
        );
        version += 1;
    }
    assert_eq!(f.client.entry_count(&f.tenant, &f.project, &period_id), 8);

    for pair in 0u32..4u32 {
        let left = 100 + pair * 2;
        let right = left + 1;
        f.client.match_entry(
            &f.finance,
            &f.tenant,
            &f.project,
            &period_id,
            &seed(&f.env, left),
            &seed(&f.env, right),
            &seed(&f.env, 400 + pair),
            &version,
            &0,
        );
        version += 1;
    }
    assert_eq!(version, 13);

    let attestation = id(&f.env, 77);
    let closed = f.client.close_period(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &period_id,
        &seed(&f.env, 500),
        &version,
        &attestation,
    );
    assert_eq!(closed.state, PeriodState::Closed);
    assert_eq!(closed.entry_count, 8);
    assert_eq!(closed.attestation_hash, attestation);

    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("LEDCLOSE"));
    let (event_period, count, state): (BytesN<32>, u32, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(event_period, period_id);
    assert_eq!(count, 8);
    assert_eq!(state, 2);
}

#[test]
fn mismatch_blocks_close_until_resolved() {
    let f = setup();
    let period_id = id(&f.env, 11);
    open_period(&f, &period_id, &id(&f.env, 201), 32);
    f.client.record_entry(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 30),
        &id(&f.env, 202),
        &1,
        &EntryKind::Intent,
        &100,
        &id(&f.env, 31),
    );
    f.client.record_entry(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 32),
        &id(&f.env, 203),
        &2,
        &EntryKind::Release,
        &90,
        &id(&f.env, 33),
    );

    assert_eq!(
        f.client
            .try_match_entry(
                &f.finance,
                &f.tenant,
                &f.project,
                &period_id,
                &id(&f.env, 30),
                &id(&f.env, 32),
                &id(&f.env, 204),
                &3,
                &1_000_001,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidTolerance
    );

    let record = f.client.match_entry(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 30),
        &id(&f.env, 32),
        &id(&f.env, 205),
        &3,
        &5,
    );
    assert_eq!(record.state, EntryState::Mismatch);
    assert_eq!(record.delta, 10);
    assert_eq!(record.reason_code, 1);

    assert_eq!(
        f.client
            .try_close_period(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &period_id,
                &id(&f.env, 206),
                &4,
                &id(&f.env, 34),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::MismatchOpen
    );

    let resolved = f.client.resolve_mismatch(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 30),
        &id(&f.env, 207),
        &4,
        &id(&f.env, 35),
    );
    assert_eq!(resolved.state, EntryState::Matched);
    assert_eq!(resolved.reason_code, 0);
    let closed = f.client.close_period(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 208),
        &5,
        &id(&f.env, 36),
    );
    assert_eq!(closed.state, PeriodState::Closed);
}

#[test]
fn replay_is_cached_and_stale_writes_fail() {
    let f = setup();
    let period_id = id(&f.env, 12);
    let request_id = id(&f.env, 210);
    open_period(&f, &period_id, &request_id, 8);
    let replay = f.client.open_period(
        &f.coordinator,
        &f.tenant,
        &f.project,
        &period_id,
        &request_id,
        &0,
        &NOW,
        &(NOW + 10_000),
        &8,
    );
    assert_eq!(replay.period_id, period_id);
    assert_eq!(f.client.period_count(&f.tenant, &f.project), 1);
    assert_eq!(f.env.events().all().len(), 0);

    assert_eq!(
        f.client
            .try_open_period(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &period_id,
                &request_id,
                &7,
                &NOW,
                &(NOW + 10_000),
                &8,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
    assert_eq!(
        f.client
            .try_record_entry(
                &f.finance,
                &f.tenant,
                &f.project,
                &period_id,
                &id(&f.env, 37),
                &id(&f.env, 211),
                &0,
                &EntryKind::Fee,
                &10,
                &id(&f.env, 38),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
}

#[test]
fn entry_limit_and_period_index_are_bounded() {
    let f = setup();
    let period_id = id(&f.env, 13);
    open_period(&f, &period_id, &id(&f.env, 220), 32);
    for n in 0u32..32u32 {
        f.client.record_entry(
            &f.finance,
            &f.tenant,
            &f.project,
            &period_id,
            &seed(&f.env, 1_000 + n),
            &seed(&f.env, 2_000 + n),
            &(1 + n),
            &EntryKind::External,
            &10,
            &seed(&f.env, 3_000 + n),
        );
    }
    assert_eq!(f.client.entry_count(&f.tenant, &f.project, &period_id), 32);
    assert_eq!(
        f.client
            .try_record_entry(
                &f.finance,
                &f.tenant,
                &f.project,
                &period_id,
                &seed(&f.env, 1_100),
                &seed(&f.env, 2_100),
                &33,
                &EntryKind::External,
                &10,
                &seed(&f.env, 3_100),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::IndexFull
    );

    let other = setup();
    for n in 0u32..16u32 {
        open_period(&other, &seed(&other.env, 4_000 + n), &seed(&other.env, 5_000 + n), 1);
    }
    assert_eq!(other.client.period_count(&other.tenant, &other.project), 16);
    assert_eq!(
        other
            .client
            .try_open_period(
                &other.coordinator,
                &other.tenant,
                &other.project,
                &seed(&other.env, 4_100),
                &seed(&other.env, 5_100),
                &0,
                &NOW,
                &(NOW + 10_000),
                &1,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::IndexFull
    );
}

#[test]
fn roles_and_period_windows_are_enforced() {
    let f = setup();
    let other_tenant = id(&f.env, 92);
    assert_eq!(
        f.client
            .try_open_period(
                &f.auditor,
                &f.tenant,
                &f.project,
                &id(&f.env, 14),
                &id(&f.env, 230),
                &0,
                &NOW,
                &(NOW + 100),
                &4,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_record_entry(
                &f.finance,
                &other_tenant,
                &f.project,
                &id(&f.env, 14),
                &id(&f.env, 40),
                &id(&f.env, 231),
                &0,
                &EntryKind::Intent,
                &10,
                &id(&f.env, 41),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_open_period(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 14),
                &id(&f.env, 232),
                &0,
                &NOW + 100,
                &NOW,
                &4,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPeriod
    );
    assert_eq!(
        f.client
            .try_open_period(
                &f.coordinator,
                &f.tenant,
                &f.project,
                &id(&f.env, 14),
                &id(&f.env, 233),
                &0,
                &(NOW - 100),
                &(NOW - 10),
                &4,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPeriod
    );
}

#[test]
fn missing_counterpart_returns_actionable_error() {
    let f = setup();
    let period_id = id(&f.env, 15);
    open_period(&f, &period_id, &id(&f.env, 240), 4);
    f.client.record_entry(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 42),
        &id(&f.env, 241),
        &1,
        &EntryKind::Hold,
        &10,
        &id(&f.env, 43),
    );
    assert_eq!(
        f.client
            .try_match_entry(
                &f.finance,
                &f.tenant,
                &f.project,
                &period_id,
                &id(&f.env, 42),
                &id(&f.env, 44),
                &id(&f.env, 242),
                &2,
                &0,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::CounterpartNotFound
    );
}

#[test]
fn resource_budget_stays_within_transaction_limits() {
    let f = setup();
    let period_id = id(&f.env, 16);
    f.env.cost_estimate().budget().reset_default();
    open_period(&f, &period_id, &id(&f.env, 250), 8);
    f.client.record_entry(
        &f.finance,
        &f.tenant,
        &f.project,
        &period_id,
        &id(&f.env, 45),
        &id(&f.env, 251),
        &1,
        &EntryKind::Fee,
        &10,
        &id(&f.env, 46),
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 100_000_000);
    assert!(budget.memory_bytes_cost() < 41_943_040);
}

#[test]
fn version_and_initialization_are_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}
