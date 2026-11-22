#![cfg(test)]

use crate::{
    CapstoneDiscussionThreadsContract, CapstoneDiscussionThreadsContractClient, ContractError,
    MessageDraft, MessageModeration, ModerationAction, Resolution, ThreadAction, ThreadDraft,
    ThreadRole, ThreadState, MAX_ATTACHMENTS, MAX_MENTIONS, SCHEMA_VERSION,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol, Vec,
};

const BASE_VERSION: u32 = 2;

struct Fixture<'a> {
    env: Env,
    tenant: BytesN<32>,
    project: BytesN<32>,
    admin: Address,
    moderator: Address,
    participant: Address,
    client: CapstoneDiscussionThreadsContractClient<'a>,
}

fn setup() -> Fixture<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(CapstoneDiscussionThreadsContract, ());
    let client = CapstoneDiscussionThreadsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let moderator = Address::generate(&env);
    let participant = Address::generate(&env);
    let tenant = hash(&env, 1);
    let project = hash(&env, 2);
    client.initialize(&admin, &tenant, &project);
    client.grant_role(
        &admin, &tenant, &project, &moderator, &ThreadRole::Moderator, &hash(&env, 11), &0,
    );
    client.grant_role(
        &admin, &tenant, &project, &participant, &ThreadRole::Participant, &hash(&env, 12), &1,
    );
    Fixture {
        env,
        tenant,
        project,
        admin,
        moderator,
        participant,
        client,
    }
}

fn hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn draft(env: &Env, seed: u8, retention: u64) -> ThreadDraft {
    ThreadDraft {
        thread_id: hash(env, seed),
        topic_hash: hash(env, seed.wrapping_add(20)),
        context_hash: hash(env, seed.wrapping_add(40)),
        retention_seconds: retention,
    }
}

fn message(env: &Env, thread_seed: u8, seed: u8, parent: BytesN<32>) -> MessageDraft {
    let mut attachments = Vec::new(env);
    attachments.push_back(hash(env, seed.wrapping_add(70)));
    MessageDraft {
        thread_id: hash(env, thread_seed),
        message_id: hash(env, seed),
        parent_id: parent,
        content_hash: hash(env, seed.wrapping_add(20)),
        mention_digest: hash(env, seed.wrapping_add(40)),
        mention_count: 1,
        attachments,
    }
}

#[test]
fn roles_are_enforced_and_unassigned_actors_are_denied() {
    let f = setup();
    let outsider = Address::generate(&f.env);
    let thread = draft(&f.env, 20, 100);

    assert_eq!(
        f.client
            .try_open_thread(
                &outsider,
                &f.tenant,
                &f.project,
                &thread,
                &hash(&f.env, 60),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 61),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .try_resolve_thread(
                &f.participant,
                &f.tenant,
                &f.project,
                &thread.thread_id,
                &Resolution::Resolved,
                &hash(&f.env, 62),
                &hash(&f.env, 63),
                &(BASE_VERSION + 1)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
}

#[test]
fn thread_open_replay_is_idempotent_and_conflicts_are_rejected() {
    let f = setup();
    let thread = draft(&f.env, 30, 100);
    let request = hash(&f.env, 64);
    let events_before = f.env.events().all().len();

    let first = f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &request,
        &BASE_VERSION,
    );
    let replay = f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &request,
        &BASE_VERSION,
    );
    assert_eq!(first, replay);
    assert_eq!(f.env.events().all().len(), events_before + 1);
    assert_eq!(
        f.client
            .threads_page(&f.participant, &f.tenant, &f.project, 0, 10)
            .len(),
        1
    );
    assert_eq!(
        f.client
            .try_open_thread(
                &f.participant,
                &f.tenant,
                &f.project,
                &thread,
                &hash(&f.env, 65),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::StaleVersion
    );
    let mut changed = thread.clone();
    changed.topic_hash = hash(&f.env, 66);
    assert_eq!(
        f.client
            .try_open_thread(
                &f.participant,
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
fn messages_keep_stable_order_and_reject_bad_inputs() {
    let f = setup();
    let thread = draft(&f.env, 40, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 70),
        &BASE_VERSION,
    );
    let root = message(&f.env, 40, 41, BytesN::from_array(&f.env, &[0u8; 32]));
    let root_id = f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &root,
        &hash(&f.env, 71),
        &(BASE_VERSION + 1),
    );
    assert_eq!(root_id, root.message_id);
    let child = message(&f.env, 40, 42, root_id.clone());
    let child_request = hash(&f.env, 72);
    let child_id = f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &child,
        &child_request,
        &(BASE_VERSION + 2),
    );
    let child_replay = f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &child,
        &child_request,
        &(BASE_VERSION + 2),
    );
    assert_eq!(child_id, child_replay);
    let ordered = f.client.messages_page(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread.thread_id,
        0,
        10,
    );
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered.get(0).unwrap(), root_id);
    assert_eq!(ordered.get(1).unwrap(), child_id);
    assert_eq!(
        f.client
            .get_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &thread.thread_id,
                &child_id
            )
            .sequence,
        2
    );
    let mut bad_parent = message(&f.env, 40, 43, hash(&f.env, 73));
    assert_eq!(
        f.client
            .try_post_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &bad_parent,
                &hash(&f.env, 74),
                &(BASE_VERSION + 3)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidParent
    );
    bad_parent.mention_count = MAX_MENTIONS + 1;
    bad_parent.message_id = hash(&f.env, 75);
    assert_eq!(
        f.client
            .try_post_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &bad_parent,
                &hash(&f.env, 76),
                &(BASE_VERSION + 3)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TooManyMentions
    );
    bad_parent.mention_count = 1;
    bad_parent.attachments.push_back(hash(&f.env, 77));
    bad_parent.attachments.push_back(hash(&f.env, 78));
    bad_parent.attachments.push_back(hash(&f.env, 79));
    bad_parent.attachments.push_back(hash(&f.env, 82));
    bad_parent.message_id = hash(&f.env, 80);
    assert_eq!(
        f.client
            .try_post_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &bad_parent,
                &hash(&f.env, 81),
                &(BASE_VERSION + 3)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TooManyAttachments
    );
    assert_eq!(MAX_ATTACHMENTS, 4);
}

#[test]
fn resolution_and_reopen_follow_stable_transitions() {
    let f = setup();
    let thread = draft(&f.env, 50, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 90),
        &BASE_VERSION,
    );
    f.client.resolve_thread(
        &f.moderator,
        &f.tenant,
        &f.project,
        &thread.thread_id,
        &Resolution::ResolvedWithHelp,
        &hash(&f.env, 91),
        &hash(&f.env, 92),
        &(BASE_VERSION + 1),
    );
    assert_eq!(
        f.client
            .get_thread(&f.participant, &f.tenant, &f.project, &thread.thread_id)
            .state,
        ThreadState::Resolved
    );
    assert_eq!(
        f.client
            .try_resolve_thread(
                &f.moderator,
                &f.tenant,
                &f.project,
                &thread.thread_id,
                &Resolution::Resolved,
                &hash(&f.env, 93),
                &hash(&f.env, 94),
                &(BASE_VERSION + 2)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidThreadState
    );
    f.client.reopen_thread(
        &f.moderator,
        &f.tenant,
        &f.project,
        &thread.thread_id,
        &hash(&f.env, 95),
        &(BASE_VERSION + 2),
    );
    assert_eq!(
        f.client
            .get_thread(&f.participant, &f.tenant, &f.project, &thread.thread_id)
            .resolution,
        Resolution::Unresolved
    );
    let root = message(&f.env, 50, 51, BytesN::from_array(&f.env, &[0u8; 32]));
    f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &root,
        &hash(&f.env, 96),
        &(BASE_VERSION + 3),
    );
}

#[test]
fn moderation_hooks_are_replay_safe() {
    let f = setup();
    let thread = draft(&f.env, 60, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 100),
        &BASE_VERSION,
    );
    let root = message(&f.env, 60, 61, BytesN::from_array(&f.env, &[0u8; 32]));
    f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &root,
        &hash(&f.env, 101),
        &(BASE_VERSION + 1),
    );
    let events_before = f.env.events().all().len();
    let intent = hash(&f.env, 102);
    let first = f.client.moderate_message(
        &f.moderator,
        &f.tenant,
        &f.project,
        &thread.thread_id,
        &root.message_id,
        &ModerationAction::Hide,
        &intent,
        &(BASE_VERSION + 2),
    );
    let replay = f.client.moderate_message(
        &f.moderator,
        &f.tenant,
        &f.project,
        &thread.thread_id,
        &root.message_id,
        &ModerationAction::Hide,
        &intent,
        &(BASE_VERSION + 2),
    );
    assert_eq!(first, replay);
    assert_eq!(f.env.events().all().len(), events_before + 1);
    assert_eq!(
        f.client
            .get_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &thread.thread_id,
                &root.message_id
            )
            .moderation,
        MessageModeration::Hidden
    );
    assert_eq!(
        f.client
            .try_moderate_message(
                &f.participant,
                &f.tenant,
                &f.project,
                &thread.thread_id,
                &root.message_id,
                &ModerationAction::Restore,
                &hash(&f.env, 103),
                &(BASE_VERSION + 3)
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized
    );
}

#[test]
fn retention_expiry_and_timestamp_overflow_are_deterministic() {
    let f = setup();
    f.env.ledger().with_mut(|li| li.timestamp = 3_000);
    let thread = draft(&f.env, 70, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 110),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_thread(&f.participant, &f.tenant, &f.project, &thread.thread_id)
            .expires_at,
        3_100
    );
    f.env.ledger().with_mut(|li| li.timestamp = 3_101);
    assert_eq!(
        f.client
            .try_get_thread(&f.participant, &f.tenant, &f.project, &thread.thread_id)
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
            .try_open_thread(
                &other.participant,
                &other.tenant,
                &other.project,
                &draft(&other.env, 71, 100),
                &hash(&other.env, 111),
                &BASE_VERSION
            )
            .unwrap_err()
            .unwrap(),
        ContractError::TimestampOverflow
    );
}

#[test]
fn message_event_carries_only_ids_enums_hashes_and_sequences() {
    let f = setup();
    let thread = draft(&f.env, 80, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 120),
        &BASE_VERSION,
    );
    let root = message(&f.env, 80, 81, BytesN::from_array(&f.env, &[0u8; 32]));
    let events_before = f.env.events().all().len();
    f.client.post_message(
        &f.participant,
        &f.tenant,
        &f.project,
        &root,
        &hash(&f.env, 121),
        &(BASE_VERSION + 1),
    );
    let events = f.env.events().all();
    let (_, topics, data) = events.get(events_before).unwrap();
    let topic: Symbol = topics.get(0).unwrap().try_into_val(&f.env).unwrap();
    assert_eq!(topic, symbol_short!("THRMSG"));
    let decoded: (BytesN<32>, ThreadAction, u32, BytesN<32>) =
        data.try_into_val(&f.env).unwrap();
    assert_eq!(decoded.0, root.message_id);
    assert_eq!(decoded.1, ThreadAction::Post);
    assert_eq!(decoded.2, 1);
    assert_eq!(decoded.3, root.content_hash);
}

#[test]
fn schema_and_error_codes_are_stable() {
    let f = setup();
    let thread = draft(&f.env, 90, 100);
    f.client.open_thread(
        &f.participant,
        &f.tenant,
        &f.project,
        &thread,
        &hash(&f.env, 130),
        &BASE_VERSION,
    );
    assert_eq!(
        f.client
            .get_thread(&f.participant, &f.tenant, &f.project, &thread.thread_id)
            .schema_version,
        SCHEMA_VERSION
    );
    assert_eq!(ContractError::NotInitialized as u32, 1);
    assert_eq!(ContractError::TooManyAttachments as u32, 20);
    assert_eq!(
        f.client
            .try_threads_page(&f.participant, &f.tenant, &f.project, 0, 51)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidPage
    );
}
