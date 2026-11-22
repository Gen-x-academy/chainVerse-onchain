extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, Symbol, TryFromVal, TryIntoVal,
};

use crate::{
    CapstoneProjectTimeline, CapstoneProjectTimelineClient, ContractError, DataKey, EntryKind,
    EntryState, RequestRecord, Role, RoleGrant, TimelineEntry, Visibility,
};

const NOW: u64 = 2_000_000;
const DAY: u64 = 86_400;

struct Fixture {
    env: Env,
    client: CapstoneProjectTimelineClient<'static>,
    admin: Address,
    manager: Address,
    publisher: Address,
    viewer: Address,
    tenant: BytesN<32>,
    project: BytesN<32>,
}

fn id(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

fn id2(env: &Env, hi: u8, lo: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = hi;
    bytes[31] = lo;
    BytesN::from_array(env, &bytes)
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract_id = env.register(CapstoneProjectTimeline, ());
    let client = CapstoneProjectTimelineClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let manager = Address::generate(&env);
    let publisher = Address::generate(&env);
    let viewer = Address::generate(&env);
    client.initialize(&admin);
    let tenant = id(&env, 1);
    let project = id(&env, 2);
    client.set_role(
        &admin,
        &manager,
        &tenant,
        &project,
        &Role::Manager,
        &true,
        &0,
    );
    client.set_role(
        &admin,
        &publisher,
        &tenant,
        &project,
        &Role::Publisher,
        &true,
        &0,
    );
    client.set_role(&admin, &viewer, &tenant, &project, &Role::Viewer, &true, &0);
    Fixture {
        env,
        client,
        admin,
        manager,
        publisher,
        viewer,
        tenant,
        project,
    }
}

fn append(f: &Fixture, request: u8, entry: u8, kind: EntryKind, visibility: Visibility) -> u32 {
    f.client.append_entry(
        &f.publisher,
        &f.tenant,
        &f.project,
        &id(&f.env, request),
        &id(&f.env, entry),
        &kind,
        &id(&f.env, entry.wrapping_add(100)),
        &(NOW + u64::from(entry) * DAY),
        &visibility,
    )
}

fn entry(f: &Fixture, who: &Address, entry: u8) -> TimelineEntry {
    f.client
        .get_entry(who, &f.tenant, &f.project, &id(&f.env, entry))
}

#[test]
fn initialization_is_one_time_and_version_is_stable() {
    let f = setup();
    assert_eq!(f.client.version(), 1);
    assert_eq!(
        f.client.try_initialize(&f.admin).unwrap_err().unwrap(),
        ContractError::AlreadyInitialized
    );
}

#[test]
fn entries_cover_every_timeline_kind_and_keep_stable_sequence() {
    let f = setup();
    let kinds = [
        EntryKind::Objective,
        EntryKind::Task,
        EntryKind::Milestone,
        EntryKind::Decision,
        EntryKind::Review,
        EntryKind::Funding,
        EntryKind::CharterChange,
    ];
    for (n, kind) in kinds.iter().enumerate() {
        let seq = append(&f, 10 + n as u8, 1 + n as u8, *kind, Visibility::Team);
        assert_eq!(seq, n as u32 + 1);
    }
    let page = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.len(), 7);
    assert!(!page.has_more);
    assert_eq!(page.next_seq, 8);
    for (n, kind) in kinds.iter().enumerate() {
        let item = page.entries.get(n as u32).unwrap();
        assert_eq!(item.seq, n as u32 + 1);
        assert_eq!(item.kind, *kind);
        assert_eq!(item.state, EntryState::Active);
        assert_eq!(item.recorded_at, NOW);
    }
}

#[test]
fn order_is_insertion_order_not_timestamp_order() {
    let f = setup();
    // The later-dated entry is appended first; its position must not move.
    append(&f, 10, 9, EntryKind::Milestone, Visibility::Team);
    append(&f, 11, 2, EntryKind::Task, Visibility::Team);
    let page = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.get(0).unwrap().entry_id, id(&f.env, 9));
    assert_eq!(page.entries.get(1).unwrap().entry_id, id(&f.env, 2));
    // Re-reading after more appends returns the same prefix.
    append(&f, 12, 5, EntryKind::Review, Visibility::Team);
    let again = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &2);
    assert_eq!(again.entries.get(0).unwrap(), page.entries.get(0).unwrap());
    assert_eq!(again.entries.get(1).unwrap(), page.entries.get(1).unwrap());
    assert!(again.has_more);
    assert_eq!(again.next_seq, 3);
}

#[test]
fn authorization_tree_is_deny_by_default() {
    let f = setup();
    let stranger = Address::generate(&f.env);
    let try_append = |actor: &Address, request: u8, entry: u8| {
        f.client.try_append_entry(
            actor,
            &f.tenant,
            &f.project,
            &id(&f.env, request),
            &id(&f.env, entry),
            &EntryKind::Task,
            &id(&f.env, 150),
            &(NOW + DAY),
            &Visibility::Team,
        )
    };
    assert_eq!(
        try_append(&f.viewer, 10, 1).unwrap_err().unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        try_append(&stranger, 11, 2).unwrap_err().unwrap(),
        ContractError::Unauthorized
    );
    assert!(try_append(&f.publisher, 12, 3).is_ok());
    assert!(try_append(&f.manager, 13, 4).is_ok());

    let try_redact = |actor: &Address, request: u8| {
        f.client.try_redact_entry(
            actor,
            &f.tenant,
            &f.project,
            &id(&f.env, request),
            &id(&f.env, 3),
            &id(&f.env, 160),
        )
    };
    for actor in [&f.viewer, &f.publisher, &stranger] {
        assert_eq!(
            try_redact(actor, 20).unwrap_err().unwrap(),
            ContractError::Unauthorized
        );
    }
    assert!(try_redact(&f.manager, 21).is_ok());

    assert_eq!(
        f.client
            .try_get_entry(&stranger, &f.tenant, &f.project, &id(&f.env, 4))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_list_entries(&stranger, &f.tenant, &f.project, &1, &8)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_set_role(
                &f.manager,
                &stranger,
                &f.tenant,
                &f.project,
                &Role::Viewer,
                &true,
                &0
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn tenant_and_project_boundaries_are_enforced() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let other_tenant = id(&f.env, 60);
    let other_project = id(&f.env, 61);
    assert_eq!(
        f.client
            .try_list_entries(&f.manager, &other_tenant, &f.project, &1, &8)
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    assert_eq!(
        f.client
            .try_get_entry(&f.manager, &f.tenant, &other_project, &id(&f.env, 1))
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
    let other_publisher = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &other_publisher,
        &other_tenant,
        &f.project,
        &Role::Publisher,
        &true,
        &0,
    );
    // Same entry id and request id live independently per tenant.
    let seq = f.client.append_entry(
        &other_publisher,
        &other_tenant,
        &f.project,
        &id(&f.env, 10),
        &id(&f.env, 1),
        &EntryKind::Task,
        &id(&f.env, 101),
        &(NOW + DAY),
        &Visibility::Team,
    );
    assert_eq!(seq, 1);
    let theirs = f
        .client
        .list_entries(&other_publisher, &other_tenant, &f.project, &1, &8);
    assert_eq!(theirs.entries.len(), 1);
    assert_eq!(theirs.entries.get(0).unwrap().tenant_id, other_tenant);
}

#[test]
fn replay_returns_the_recorded_sequence_without_side_effects() {
    let f = setup();
    let first = append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let replay = append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    assert_eq!(first, replay);
    assert_eq!(f.env.events().all().len(), 0);
    append(&f, 11, 2, EntryKind::Task, Visibility::Team);
    // Replaying the first request later still yields sequence 1 and appends nothing.
    assert_eq!(append(&f, 10, 1, EntryKind::Task, Visibility::Team), 1);
    let page = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.len(), 2);
}

#[test]
fn request_reuse_with_other_payload_or_actor_conflicts() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let err = f
        .client
        .try_append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &id(&f.env, 2),
            &EntryKind::Task,
            &id(&f.env, 102),
            &(NOW + DAY),
            &Visibility::Team,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
    let err = f
        .client
        .try_append_entry(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 10),
            &id(&f.env, 1),
            &EntryKind::Task,
            &id(&f.env, 101),
            &(NOW + DAY),
            &Visibility::Team,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::RequestConflict);
}

#[test]
fn duplicate_entry_ids_cannot_be_appended_twice() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let err = f
        .client
        .try_append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id(&f.env, 11),
            &id(&f.env, 1),
            &EntryKind::Task,
            &id(&f.env, 101),
            &(NOW + DAY),
            &Visibility::Team,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::AlreadyExists);
    let page = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.len(), 1);
}

#[test]
fn restricted_entries_are_visible_to_managers_only_and_never_shift_cursors() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Funding, Visibility::Team);
    append(&f, 11, 2, EntryKind::CharterChange, Visibility::Restricted);
    append(&f, 12, 3, EntryKind::Task, Visibility::Team);

    for who in [&f.viewer, &f.publisher] {
        assert_eq!(
            f.client
                .try_get_entry(who, &f.tenant, &f.project, &id(&f.env, 2))
                .unwrap_err()
                .unwrap(),
            ContractError::NotFound
        );
        let page = f.client.list_entries(who, &f.tenant, &f.project, &1, &32);
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries.get(0).unwrap().seq, 1);
        assert_eq!(page.entries.get(1).unwrap().seq, 3);
    }
    let manager_page = f
        .client
        .list_entries(&f.manager, &f.tenant, &f.project, &1, &32);
    assert_eq!(manager_page.entries.len(), 3);
    assert_eq!(entry(&f, &f.manager, 2).visibility, Visibility::Restricted);

    // Paging two at a time: the restricted slot is consumed, not skipped over.
    let first = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &2);
    assert_eq!(first.entries.len(), 1);
    assert_eq!(first.next_seq, 3);
    assert!(first.has_more);
    let second = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &first.next_seq, &2);
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.entries.get(0).unwrap().seq, 3);
    assert!(!second.has_more);
}

#[test]
fn redaction_clears_the_reference_once_and_keeps_the_slot() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Decision, Visibility::Team);
    append(&f, 11, 2, EntryKind::Task, Visibility::Team);
    let seq = f.client.redact_entry(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 20),
        &id(&f.env, 1),
        &id(&f.env, 170),
    );
    assert_eq!(seq, 1);
    let redacted = entry(&f, &f.viewer, 1);
    assert_eq!(redacted.state, EntryState::Redacted);
    assert_eq!(redacted.ref_hash, BytesN::from_array(&f.env, &[0u8; 32]));
    assert_eq!(redacted.redaction_hash, id(&f.env, 170));
    assert_eq!(redacted.seq, 1);
    assert_eq!(entry(&f, &f.viewer, 2).seq, 2);

    // A second redaction under a new request is an invalid transition.
    let err = f
        .client
        .try_redact_entry(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 21),
            &id(&f.env, 1),
            &id(&f.env, 171),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidTransition);
    // The same request replays cleanly with no new event.
    assert_eq!(
        f.client.redact_entry(
            &f.manager,
            &f.tenant,
            &f.project,
            &id(&f.env, 20),
            &id(&f.env, 1),
            &id(&f.env, 170),
        ),
        1
    );
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(entry(&f, &f.viewer, 1).redaction_hash, id(&f.env, 170));

    assert_eq!(
        f.client
            .try_redact_entry(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 22),
                &id(&f.env, 99),
                &id(&f.env, 172),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
}

#[test]
fn input_validation_rejects_empty_hashes_and_distant_dates() {
    let f = setup();
    let try_append = |request: u8, entry: BytesN<32>, reference: BytesN<32>, at: u64| {
        f.client
            .try_append_entry(
                &f.publisher,
                &f.tenant,
                &f.project,
                &id(&f.env, request),
                &entry,
                &EntryKind::Task,
                &reference,
                &at,
                &Visibility::Team,
            )
            .unwrap_err()
            .unwrap()
    };
    let zero = BytesN::from_array(&f.env, &[0u8; 32]);
    assert_eq!(
        try_append(10, zero.clone(), id(&f.env, 1), NOW),
        ContractError::InvalidInput
    );
    assert_eq!(
        try_append(11, id(&f.env, 1), zero.clone(), NOW),
        ContractError::InvalidInput
    );
    assert_eq!(
        try_append(12, id(&f.env, 1), id(&f.env, 2), 0),
        ContractError::InvalidInput
    );
    assert_eq!(
        try_append(13, id(&f.env, 1), id(&f.env, 2), NOW + 6 * 365 * DAY),
        ContractError::InvalidInput
    );
    // The boundary itself is accepted.
    assert!(f
        .client
        .try_append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id(&f.env, 14),
            &id(&f.env, 1),
            &EntryKind::Milestone,
            &id(&f.env, 2),
            &(NOW + 157_680_000),
            &Visibility::Team,
        )
        .is_ok());
    assert_eq!(
        f.client
            .try_redact_entry(
                &f.manager,
                &f.tenant,
                &f.project,
                &id(&f.env, 15),
                &id(&f.env, 1),
                &zero,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
}

#[test]
fn role_expiry_denies_after_the_deadline() {
    let f = setup();
    let temp = Address::generate(&f.env);
    f.client.set_role(
        &f.admin,
        &temp,
        &f.tenant,
        &f.project,
        &Role::Publisher,
        &true,
        &(NOW + 60),
    );
    let try_append = |request: u8, entry: u8| {
        f.client.try_append_entry(
            &temp,
            &f.tenant,
            &f.project,
            &id(&f.env, request),
            &id(&f.env, entry),
            &EntryKind::Task,
            &id(&f.env, 140),
            &(NOW + DAY),
            &Visibility::Team,
        )
    };
    assert!(try_append(10, 1).is_ok());
    f.env.ledger().set_timestamp(NOW + 60);
    assert_eq!(
        try_append(11, 2).unwrap_err().unwrap(),
        ContractError::RoleExpired
    );
    assert_eq!(
        f.client
            .try_set_role(
                &f.admin,
                &temp,
                &f.tenant,
                &f.project,
                &Role::Publisher,
                &true,
                &(NOW + 10)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidInput
    );
    f.client.set_role(
        &f.admin,
        &f.publisher,
        &f.tenant,
        &f.project,
        &Role::Publisher,
        &false,
        &0,
    );
    f.env.ledger().set_timestamp(NOW + 61);
    assert_eq!(
        f.client
            .try_append_entry(
                &f.publisher,
                &f.tenant,
                &f.project,
                &id(&f.env, 12),
                &id(&f.env, 3),
                &EntryKind::Task,
                &id(&f.env, 140),
                &(NOW + DAY),
                &Visibility::Team,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Unauthorized
    );
}

#[test]
fn storage_and_page_bounds_are_enforced() {
    let f = setup();
    for n in 0u32..256 {
        f.client.append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id2(&f.env, 3, n as u8),
            &id2(&f.env, 4, n as u8),
            &EntryKind::Task,
            &id(&f.env, 9),
            &(NOW + DAY),
            &Visibility::Team,
        );
    }
    let err = f
        .client
        .try_append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id2(&f.env, 3, 255),
            &id2(&f.env, 5, 0),
            &EntryKind::Task,
            &id(&f.env, 9),
            &(NOW + DAY),
            &Visibility::Team,
        )
        .unwrap_err()
        .unwrap();
    // The reused request id is rejected before the bound is even consulted.
    assert_eq!(err, ContractError::RequestConflict);
    let err = f
        .client
        .try_append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id2(&f.env, 6, 0),
            &id2(&f.env, 5, 0),
            &EntryKind::Task,
            &id(&f.env, 9),
            &(NOW + DAY),
            &Visibility::Team,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::LimitReached);

    let last = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &225, &32);
    assert_eq!(last.entries.len(), 32);
    assert_eq!(last.next_seq, 257);
    assert!(!last.has_more);
    let beyond = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &257, &32);
    assert_eq!(beyond.entries.len(), 0);
    for (from, limit) in [(0u32, 8u32), (1, 0), (1, 33), (258, 1)] {
        assert_eq!(
            f.client
                .try_list_entries(&f.viewer, &f.tenant, &f.project, &from, &limit)
                .unwrap_err()
                .unwrap(),
            ContractError::InvalidPage
        );
    }
}

#[test]
fn events_follow_the_documented_schema_without_private_payloads() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Funding, Visibility::Restricted);
    let events = f.env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    assert_eq!(topics.len(), 1);
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("TLN_ADD"));
    let (tenant, project, entry_id, seq, kind, at, visibility): (
        BytesN<32>,
        BytesN<32>,
        BytesN<32>,
        u32,
        u32,
        u64,
        u32,
    ) = data.try_into_val(&f.env).unwrap();
    assert_eq!(tenant, f.tenant);
    assert_eq!(project, f.project);
    assert_eq!(entry_id, id(&f.env, 1));
    assert_eq!((seq, kind, at, visibility), (1, 6, NOW + DAY, 2));

    f.client.redact_entry(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &id(&f.env, 180),
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("TLN_RED"));
    let (_, _, redacted, seq): (BytesN<32>, BytesN<32>, BytesN<32>, u32) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!((redacted, seq), (id(&f.env, 1), 1));

    f.client.set_role(
        &f.admin,
        &f.viewer,
        &f.tenant,
        &f.project,
        &Role::Viewer,
        &true,
        &(NOW + 9),
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(0).unwrap();
    let topic = Symbol::try_from_val(&f.env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("TLN_ROLE"));
    let (_, _, account, role, active, expires): (BytesN<32>, BytesN<32>, Address, u32, bool, u64) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(
        (account, role, active, expires),
        (f.viewer.clone(), 3, true, NOW + 9)
    );
}

#[test]
fn failed_calls_emit_no_events_and_change_nothing() {
    let f = setup();
    append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let _ = f.client.try_redact_entry(
        &f.viewer,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &id(&f.env, 180),
    );
    assert_eq!(f.env.events().all().len(), 0);
    assert_eq!(entry(&f, &f.viewer, 1).state, EntryState::Active);
}

#[test]
fn error_codes_and_discriminants_are_stable() {
    assert_eq!(ContractError::AlreadyInitialized as u32, 1);
    assert_eq!(ContractError::NotInitialized as u32, 2);
    assert_eq!(ContractError::Unauthorized as u32, 3);
    assert_eq!(ContractError::RequestConflict as u32, 4);
    assert_eq!(ContractError::StaleVersion as u32, 5);
    assert_eq!(ContractError::NotFound as u32, 6);
    assert_eq!(ContractError::AlreadyExists as u32, 7);
    assert_eq!(ContractError::InvalidInput as u32, 8);
    assert_eq!(ContractError::InvalidTransition as u32, 9);
    assert_eq!(ContractError::LimitReached as u32, 10);
    assert_eq!(ContractError::InvalidPage as u32, 11);
    assert_eq!(ContractError::ArithmeticError as u32, 12);
    assert_eq!(ContractError::RoleExpired as u32, 13);
    assert_eq!(
        [
            Role::Manager as u32,
            Role::Publisher as u32,
            Role::Viewer as u32
        ],
        [1, 2, 3]
    );
    assert_eq!(
        [
            EntryKind::Objective as u32,
            EntryKind::Task as u32,
            EntryKind::Milestone as u32,
            EntryKind::Decision as u32,
            EntryKind::Review as u32,
            EntryKind::Funding as u32,
            EntryKind::CharterChange as u32
        ],
        [1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(
        [Visibility::Team as u32, Visibility::Restricted as u32],
        [1, 2]
    );
    assert_eq!(
        [EntryState::Active as u32, EntryState::Redacted as u32],
        [1, 2]
    );
}

#[test]
fn records_written_by_an_earlier_deployment_remain_readable() {
    let f = setup();
    let legacy = TimelineEntry {
        entry_id: id(&f.env, 77),
        tenant_id: f.tenant.clone(),
        project_id: f.project.clone(),
        seq: 1,
        kind: EntryKind::Milestone,
        ref_hash: id(&f.env, 78),
        redaction_hash: BytesN::from_array(&f.env, &[0u8; 32]),
        occurred_at: 10,
        visibility: Visibility::Team,
        state: EntryState::Active,
        recorded_at: 11,
    };
    f.env.as_contract(&f.client.address, || {
        let storage = f.env.storage().persistent();
        storage.set(
            &DataKey::Entry(f.tenant.clone(), f.project.clone(), 1),
            &legacy,
        );
        storage.set(
            &DataKey::EntrySeq(f.tenant.clone(), f.project.clone(), id(&f.env, 77)),
            &1u32,
        );
        storage.set(&DataKey::Head(f.tenant.clone(), f.project.clone()), &1u32);
        storage.set(
            &DataKey::Role(
                f.viewer.clone(),
                f.tenant.clone(),
                f.project.clone(),
                Role::Viewer,
            ),
            &RoleGrant {
                active: true,
                expires_at: 0,
            },
        );
        storage.set(
            &DataKey::Request(f.tenant.clone(), f.project.clone(), id(&f.env, 90)),
            &RequestRecord {
                input_hash: id(&f.env, 91),
                result: 1,
            },
        );
    });
    assert_eq!(entry(&f, &f.viewer, 77), legacy);
    // New appends continue after the legacy head.
    assert_eq!(append(&f, 92, 5, EntryKind::Task, Visibility::Team), 2);
    let page = f
        .client
        .list_entries(&f.viewer, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries.get(0).unwrap(), legacy);
}

#[test]
fn resource_budgets_stay_within_transaction_limits() {
    let f = setup();
    f.env.cost_estimate().budget().reset_default();
    append(&f, 10, 1, EntryKind::Task, Visibility::Team);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    f.env.cost_estimate().budget().reset_default();
    f.client.redact_entry(
        &f.manager,
        &f.tenant,
        &f.project,
        &id(&f.env, 11),
        &id(&f.env, 1),
        &id(&f.env, 190),
    );
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 10_000_000);
    assert!(budget.memory_bytes_cost() < 4_194_304);

    for n in 0u32..40 {
        f.client.append_entry(
            &f.publisher,
            &f.tenant,
            &f.project,
            &id2(&f.env, 3, n as u8),
            &id2(&f.env, 4, n as u8),
            &EntryKind::Task,
            &id(&f.env, 9),
            &(NOW + DAY),
            &Visibility::Team,
        );
    }
    f.env.cost_estimate().budget().reset_default();
    let page = f
        .client
        .list_entries(&f.manager, &f.tenant, &f.project, &1, &32);
    assert_eq!(page.entries.len(), 32);
    let budget = f.env.cost_estimate().budget();
    assert!(budget.cpu_instruction_cost() < 25_000_000);
    assert!(budget.memory_bytes_cost() < 10_485_760);
}
