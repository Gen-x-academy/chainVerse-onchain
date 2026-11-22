#![cfg(test)]

use crate::{
    CapstoneProjectNotesContract, CapstoneProjectNotesContractClient, ContractError, NoteAction,
    NoteDraft, NoteLinkInput, NoteRole, RenderPolicy, MAX_LINKS, SCHEMA_VERSION,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol,
};

const BASE_VERSION: u32 = 2;

struct Fixture<'a> {
    env: Env,
    tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    editor: Address,
    viewer: Address,
    client: CapstoneProjectNotesContractClient<'a>,
}

fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneProjectNotesContract, ());
    let client = CapstoneProjectNotesContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let editor = Address::generate(&env);
    let viewer = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin, &tenant, &project);
    client.grant_role(
        &admin,
        &tenant,
        &project,
        &editor,
        &NoteRole::Editor,
        &hash(&env, 11),
        &0,
    );
    client.grant_role(
        &admin,
        &tenant,
        &project,
        &viewer,
        &NoteRole::Viewer,
        &hash(&env, 12),
        &1,
    );
    Fixture {
        env,
        tenant,
        project,
        admin,
        editor,
        viewer,
        client,
    }
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn draft(env: &Env, note_seed: u8, content_seed: u8, retention: u64) -> NoteDraft {
    NoteDraft {
        note_id: hash(env, note_seed),
        content_hash: hash(env, content_seed),
        ref_hash: hash(env, content_seed.wrapping_add(50)),
        render_policy: RenderPolicy::MarkdownSubset,
        retention_seconds: retention,
    }
}

#[test]
fn unassigned_actors_are_denied_and_initialization_is_one_time() {
    let f = setup();
    let outsider = Address::generate(&f.env);
    let note = draft(&f.env, 20, 21, 100);

    assert_eq!(
        f.client
            .try_create_note(
                &outsider,
                &f.tenant,
                &f.project,
                &note,
                &hash(&f.env, 60),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        f.client
            .try_initialize(&f.admin, &f.tenant, &f.project)
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyInitialized
    );
}

#[test]
fn create_replay_is_idempotent_and_conflicts_are_rejected() {
    let f = setup();
    let note = draft(&f.env, 30, 31, 100);
    let request = hash(&f.env, 61);
    let events_before = f.env.events().all().len();

    let first = f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &request,
        &BASE_VERSION,
    );
    let replay = f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &request,
        &BASE_VERSION,
    );

    assert_eq!(first, replay);
    assert_eq!(replay, note.note_id);
    assert_eq!(f.env.events().all().len(), events_before + 1);
    assert_eq!(
        f.client
            .notes_page(&f.viewer, &f.tenant, &f.project, 0, 10)
            .len(),
        1
    );
    assert_eq!(
        f.client
            .try_create_note(
                &f.editor,
                &f.tenant,
                &f.project,
                &note,
                &hash(&f.env, 62),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    let mut changed = note.clone();
    changed.content_hash = hash(&f.env, 63);
    assert_eq!(
        f.client
            .try_create_note(
                &f.editor,
                &f.tenant,
                &f.project,
                &changed,
                &request,
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::RequestConflict
    );
}

#[test]
fn viewers_can_read_but_cannot_mutate() {
    let f = setup();
    let note = draft(&f.env, 40, 41, 100);
    f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 64),
        &BASE_VERSION,
    );

    let stored = f
        .client
        .get_note(&f.viewer, &f.tenant, &f.project, &note.note_id);
    assert_eq!(stored.note_id, note.note_id);
    assert_eq!(stored.content_hash, note.content_hash);
    assert_eq!(
        f.client
            .try_revise_note(
                &f.viewer,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 65),
                &(BASE_VERSION + 1),
                &hash(&f.env, 66)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
}

#[test]
fn revision_and_archive_transitions_reject_stale_versions() {
    let f = setup();
    let note = draft(&f.env, 70, 71, 100);
    f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 67),
        &BASE_VERSION,
    );
    let revised_hash = hash(&f.env, 72);
    let revision_result = f.client.revise_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note.note_id,
        &revised_hash,
        &(BASE_VERSION + 1),
        &hash(&f.env, 68),
    );
    assert_eq!(revision_result, revised_hash);
    assert_eq!(
        f.client
            .get_revision(&f.viewer, &f.tenant, &f.project, &note.note_id, 1)
            .content_hash,
        revised_hash
    );
    assert_eq!(
        f.client
            .try_revise_note(
                &f.editor,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 73),
                &BASE_VERSION,
                &hash(&f.env, 74)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );

    f.client.archive_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note.note_id,
        &hash(&f.env, 75),
        &(BASE_VERSION + 2),
    );
    assert_eq!(
        f.client
            .try_revise_note(
                &f.editor,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 76),
                &(BASE_VERSION + 3),
                &hash(&f.env, 77)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NoteArchived
    );
}

#[test]
fn links_are_bounded_by_configuration() {
    let f = setup();
    let mut ids = soroban_sdk::Vec::new(&f.env);
    let mut version = BASE_VERSION;
    for index in 0..=MAX_LINKS {
        let note = draft(&f.env, 80 + index as u8, 90 + index as u8, 100);
        let id = f.client.create_note(
            &f.editor,
            &f.tenant,
            &f.project,
            &note,
            &hash(&f.env, 100 + index as u8),
            &version,
        );
        ids.push_back(id);
        version += 1;
    }
    let source = ids.get(0).unwrap();
    for index in 1..=MAX_LINKS {
        let input = NoteLinkInput {
            note_id: source.clone(),
            other_note_id: ids.get(index).unwrap(),
        };
        let result = f.client.link_notes(
            &f.editor,
            &f.tenant,
            &f.project,
            &input,
            &hash(&f.env, 120 + index as u8),
            &version,
        );
        assert_eq!(result, input.other_note_id);
        version += 1;
    }
    let overflow = NoteLinkInput {
        note_id: source,
        other_note_id: ids.get(MAX_LINKS).unwrap(),
    };
    assert_eq!(
        f.client
            .try_link_notes(
                &f.editor,
                &f.tenant,
                &f.project,
                &overflow,
                &hash(&f.env, 140),
                &version
            )
            .unwrap_err()
            .unwrap(),
        ContractError::LinksFull
    );
}

#[test]
fn retention_expiry_and_timestamp_overflow_are_deterministic() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 1_000);
    let note = draft(&f.env, 150, 151, 100);
    f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 152),
        &BASE_VERSION,
    );
    let stored = f
        .client
        .get_note(&f.viewer, &f.tenant, &f.project, &note.note_id);
    assert_eq!(stored.expires_at, 1_100);

    f.env.ledger().with_mut(|li| li.timestamp = 1_101);
    assert_eq!(
        f.client
            .try_get_note(&f.viewer, &f.tenant, &f.project, &note.note_id)
            .unwrap_err()
            .unwrap(),
        ContractError::Expired
    );

    let other = setup();
    other
        .env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    assert_eq!(
        other
            .client
            .try_create_note(
                &other.editor,
                &other.tenant,
                &other.project,
                &draft(&other.env, 153, 154, 100),
                &hash(&other.env, 155),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TimestampOverflow
    );
}

#[test]
fn creation_event_uses_only_ids_enums_hashes_and_versions() {
    let f = setup();
    let note = draft(&f.env, 160, 161, 100);
    let events_before = f.env.events().all().len();
    f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 162),
        &BASE_VERSION,
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(events_before).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&f.env).unwrap();
    assert_eq!(topic, symbol_short!("NTCRT"));
    let decoded: (BytesN<32>, NoteAction, u32, BytesN<32>) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(decoded.0, note.note_id);
    assert_eq!(decoded.1, NoteAction::Create);
    assert_eq!(decoded.2, BASE_VERSION + 1);
    assert_eq!(decoded.3, note.content_hash);
}

#[test]
fn schema_and_error_codes_are_stable() {
    let f = setup();
    let note = draft(&f.env, 170, 171, 100);
    f.client.create_note(
        &f.editor,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 172),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_note(&f.viewer, &f.tenant, &f.project, &note.note_id)
            .schema_version,
        SCHEMA_VERSION
    );
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::UnsupportedSchema as u32, 19);
    assert_eq!(
        f.client
            .try_notes_page(&f.viewer, &f.tenant, &f.project, 0, 51)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
}
