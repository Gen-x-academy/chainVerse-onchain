#![cfg(test)]

use crate::{
    CapstonePrivateNotesContract, CapstonePrivateNotesContractClient, ContractError, CryptoPolicy,
    PrivateAction, PrivateNoteDraft, PrivateRole, MAX_CONTENT_LENGTH, MAX_NOTES_PER_OWNER,
    SCHEMA_VERSION,
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
    owner: Address,
    other: Address,
    client: CapstonePrivateNotesContractClient<'a>,
}

fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstonePrivateNotesContract, ());
    let client = CapstonePrivateNotesContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let other = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin, &tenant, &project);
    client.grant_researcher(
        &admin,
        &tenant,
        &project,
        &owner,
        &hash(&env, 11),
        &0,
    );
    client.grant_researcher(
        &admin,
        &tenant,
        &project,
        &other,
        &hash(&env, 12),
        &1,
    );
    Fixture {
        env,
        tenant,
        project,
        admin,
        owner,
        other,
        client,
    }
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn draft(env: &Env, note_seed: u8, cipher_seed: u8, retention: u64) -> PrivateNoteDraft {
    PrivateNoteDraft {
        note_id: hash(env, note_seed),
        ciphertext_hash: hash(env, cipher_seed),
        key_wrap_hash: hash(env, cipher_seed.wrapping_add(40)),
        content_length: 512,
        algorithm: CryptoPolicy::XChaCha20Poly1305,
        retention_seconds: retention,
    }
}

#[test]
fn unassigned_researchers_and_admin_are_denied_reads_of_private_notes() {
    let f = setup();
    let note = draft(&f.env, 20, 21, 100);
    f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 60),
        &BASE_VERSION,
    );
    let outsider = Address::generate(&f.env);

    assert_eq!(
        f.client
            .try_get_note(&outsider, &f.tenant, &f.project, &note.note_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        f.client
            .try_get_note(&f.admin, &f.tenant, &f.project, &note.note_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        f.client
            .try_get_note(&f.other, &f.tenant, &f.project, &note.note_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    assert_eq!(
        f.client
            .owner_notes(&f.other, &f.tenant, &f.project, 0, 10)
            .len(),
        0
    );
}

#[test]
fn create_replay_is_idempotent_and_conflicts_are_rejected() {
    let f = setup();
    let note = draft(&f.env, 30, 31, 100);
    let request = hash(&f.env, 61);
    let events_before = f.env.events().all().len();

    let first = f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &request,
        &BASE_VERSION,
    );
    let replay = f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &request,
        &BASE_VERSION,
    );
    assert_eq!(first, replay);
    assert_eq!(f.env.events().all().len(), events_before + 1);
    assert_eq!(
        f.client
            .owner_notes(&f.owner, &f.tenant, &f.project, 0, 10)
            .len(),
        1
    );
    assert_eq!(
        f.client
            .try_create_note(
                &f.owner,
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
    changed.ciphertext_hash = hash(&f.env, 63);
    assert_eq!(
        f.client
            .try_create_note(
                &f.owner,
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
fn revisions_and_archive_are_owner_only_and_reject_stale_versions() {
    let f = setup();
    let note = draft(&f.env, 70, 71, 100);
    f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 64),
        &BASE_VERSION,
    );
    let new_ciphertext = hash(&f.env, 72);
    let new_key = hash(&f.env, 73);
    assert_eq!(
        f.client
            .try_revise_note(
                &f.other,
                &f.tenant,
                &f.project,
                &note.note_id,
                &new_ciphertext,
                &new_key,
                &(BASE_VERSION + 1),
                &hash(&f.env, 65)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotFound
    );
    let result = f.client.revise_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note.note_id,
        &new_ciphertext,
        &new_key,
        &(BASE_VERSION + 1),
        &hash(&f.env, 66),
    );
    assert_eq!(result, new_ciphertext);
    assert_eq!(
        f.client
            .get_revision(&f.owner, &f.tenant, &f.project, &note.note_id, 1)
            .ciphertext_hash,
        new_ciphertext
    );
    assert_eq!(
        f.client
            .try_revise_note(
                &f.owner,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 74),
                &hash(&f.env, 75),
                &BASE_VERSION,
                &hash(&f.env, 76)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    f.client.archive_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note.note_id,
        &hash(&f.env, 77),
        &(BASE_VERSION + 2),
    );
    assert_eq!(
        f.client
            .try_revise_note(
                &f.owner,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 78),
                &hash(&f.env, 79),
                &(BASE_VERSION + 3),
                &hash(&f.env, 80)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NoteArchived
    );
}

#[test]
fn expiry_and_retention_inputs_are_checked() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 1_000);
    let note = draft(&f.env, 90, 91, 100);
    f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 96),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_note(&f.owner, &f.tenant, &f.project, &note.note_id)
            .expires_at,
        1_100
    );
    assert_eq!(
        f.client
            .try_expire_note(
                &f.owner,
                &f.tenant,
                &f.project,
                &note.note_id,
                &hash(&f.env, 97),
                &(BASE_VERSION + 1)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotExpired
    );
    f.env.ledger().with_mut(|li| li.timestamp = 1_101);
    assert_eq!(
        f.client
            .try_get_note(&f.owner, &f.tenant, &f.project, &note.note_id)
            .unwrap_err()
            .unwrap(),
        ContractError::Expired
    );
    f.client.expire_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note.note_id,
        &hash(&f.env, 98),
        &(BASE_VERSION + 1),
    );

    let overflow = setup();
    overflow
        .env
        .ledger()
        .with_mut(|li| li.timestamp = u64::MAX - 10);
    assert_eq!(
        overflow
            .client
            .try_create_note(
                &overflow.owner,
                &overflow.tenant,
                &overflow.project,
                &draft(&overflow.env, 99, 100, 100),
                &hash(&overflow.env, 101),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TimestampOverflow
    );
    let mut invalid = draft(&f.env, 102, 103, 100);
    invalid.content_length = MAX_CONTENT_LENGTH + 1;
    assert_eq!(
        f.client
            .try_create_note(
                &f.owner,
                &f.tenant,
                &f.project,
                &invalid,
                &hash(&f.env, 104),
                &(BASE_VERSION + 2)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidContentLength
    );
}

#[test]
fn owner_index_is_bounded_per_researcher() {
    let f = setup();
    let mut version = BASE_VERSION;
    for index in 0..MAX_NOTES_PER_OWNER {
        let note = draft(&f.env, 110 + index as u8, 130 + index as u8, 100);
        f.client.create_note(
            &f.owner,
            &f.tenant,
            &f.project,
            &note,
            &hash(&f.env, 150 + index as u8),
            &version,
        );
        version += 1;
    }
    assert_eq!(
        f.client
            .try_create_note(
                &f.owner,
                &f.tenant,
                &f.project,
                &draft(&f.env, 200, 201, 100),
                &hash(&f.env, 202),
                &version
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotesFull
    );
}

#[test]
fn creation_event_excludes_private_content_and_addresses() {
    let f = setup();
    let note = draft(&f.env, 210, 211, 100);
    let events_before = f.env.events().all().len();
    f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 212),
        &BASE_VERSION,
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(events_before).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&f.env).unwrap();
    assert_eq!(topic, symbol_short!("PRVNEW"));
    let decoded: (BytesN<32>, PrivateAction, u32, BytesN<32>) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(decoded.0, note.note_id);
    assert_eq!(decoded.1, PrivateAction::Create);
    assert_eq!(decoded.2, 0);
    assert_eq!(decoded.3, note.ciphertext_hash);
}

#[test]
fn schema_and_error_codes_are_stable() {
    let f = setup();
    let note = draft(&f.env, 220, 221, 100);
    f.client.create_note(
        &f.owner,
        &f.tenant,
        &f.project,
        &note,
        &hash(&f.env, 222),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_note(&f.owner, &f.tenant, &f.project, &note.note_id)
            .schema_version,
        SCHEMA_VERSION
    );
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::NotExpired as u32, 19);
    assert_eq!(
        f.client
            .try_owner_notes(&f.owner, &f.tenant, &f.project, 0, 51)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
}
