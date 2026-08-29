//! Tests for #928 (canonical work registration) and #929 (works,
//! editions, and renditions).

use crate::{
    CatalogEntry, ContentCommitment, ContentState, ContractError, EntryKind, HashAlgorithm,
    LibraryRightsContractClient, MetadataCommitment,
};
use soroban_sdk::testutils::{Address as _, Events, Ledger as _};
use soroban_sdk::{symbol_short, Address, BytesN, Env, String, Symbol, TryFromVal, TryIntoVal};

fn bootstrapped<'a>(
    env: &'a Env,
    contract_id: &Address,
) -> (LibraryRightsContractClient<'a>, Address) {
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(env, contract_id);
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let policy_manager = Address::generate(env);
    let emergency = Address::generate(env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);
    (client, policy_manager)
}

fn id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

fn uri(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

fn meta(env: &Env, uri_str: &str, hash_byte: u8) -> MetadataCommitment {
    MetadataCommitment {
        uri: uri(env, uri_str),
        manifest_hash: id(env, hash_byte),
    }
}

fn content(env: &Env, algorithm: HashAlgorithm, byte: u8) -> ContentCommitment {
    ContentCommitment {
        algorithm,
        digest: id(env, byte),
    }
}

/// Registers a canonical work and returns its id.
fn reg_work(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    byte: u8,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    client.register_work(
        policy_manager,
        &id(env, byte),
        &meta(env, "ipfs://QmWork", byte + 0x40),
        &custodian,
    );
    id(env, byte)
}

/// Registers an edition of `parent` and returns its id.
fn reg_edition(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    parent: &BytesN<32>,
    byte: u8,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    client.register_edition(
        policy_manager,
        parent,
        &id(env, byte),
        &meta(env, "ipfs://QmEdition", byte + 0x40),
        &custodian,
    );
    id(env, byte)
}

/// Registers a rendition of `parent` with a fixed sha256 digest and
/// returns its id.
fn reg_rendition(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    parent: &BytesN<32>,
    byte: u8,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    client.register_rendition(
        policy_manager,
        parent,
        &id(env, byte),
        &content(env, HashAlgorithm::Sha256, byte + 0x40),
        &meta(env, "ipfs://QmRendition", byte + 0x60),
        &custodian,
    );
    id(env, byte)
}

// ===================== #928 -- canonical work registration =====================

// -- Positive --

#[test]
fn test_register_work_round_trips() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = id(&env, 1);
    let commitment = meta(&env, "ipfs://QmCanonical", 0x41);
    let custodian = Address::generate(&env);

    client.register_work(&policy_manager, &work_id, &commitment, &custodian);

    let entry: CatalogEntry = client.entry(&work_id);
    assert_eq!(entry.kind, EntryKind::Work);
    assert_eq!(entry.parent, None);
    assert_eq!(entry.version, 1);
    assert_eq!(entry.metadata, commitment);
    assert_eq!(entry.content, ContentState::None);
    assert_eq!(entry.custodian, custodian);
}

#[test]
fn test_register_work_creates_version_snapshot() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 2);

    let snapshot = client.entry_version(&work_id, &1);
    assert_eq!(snapshot.version, 1);
    assert_eq!(snapshot.registered_by, policy_manager);
    assert_eq!(client.entry_version_count(&work_id), 1);

    // Version 0 and version 2 do not exist.
    assert_eq!(
        client.try_entry_version(&work_id, &0),
        Err(Ok(ContractError::VersionNotFound))
    );
    assert_eq!(
        client.try_entry_version(&work_id, &2),
        Err(Ok(ContractError::VersionNotFound))
    );
}

// -- Negative (identifier/hash validation, overwrite prevention) --

#[test]
fn test_register_work_rejects_all_zero_id() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let res = client.try_register_work(
        &policy_manager,
        &zero,
        &meta(&env, "ipfs://QmX", 0x41),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidIdentifier)));
}

#[test]
fn test_register_work_rejects_all_zero_manifest_hash() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let bad = MetadataCommitment {
        uri: uri(&env, "ipfs://QmX"),
        manifest_hash: zero,
    };
    let res = client.try_register_work(&policy_manager, &id(&env, 3), &bad, &custodian);
    assert_eq!(res, Err(Ok(ContractError::InvalidHash)));
}

#[test]
fn test_register_work_rejects_duplicate_id() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 4);
    let custodian = Address::generate(&env);

    let res = client.try_register_work(
        &policy_manager,
        &work_id,
        &meta(&env, "ipfs://QmOther", 0x50),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::AlreadyRegistered)));
}

// -- Authorization --

#[test]
fn test_register_work_requires_policy_manager_role() {
    let (env, contract_id) = super::setup();
    let (client, _policy_manager) = bootstrapped(&env, &contract_id);
    // The bootstrap admin is not the PolicyManager.
    let admin: Address = client.get_role(&crate::Role::Admin);
    let custodian = Address::generate(&env);

    let res = client.try_register_work(
        &admin,
        &id(&env, 5),
        &meta(&env, "ipfs://QmX", 0x45),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_register_work_before_bootstrap_fails() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let caller = Address::generate(&env);
    let custodian = Address::generate(&env);

    let res = client.try_register_work(
        &caller,
        &id(&env, 6),
        &meta(&env, "ipfs://QmX", 0x46),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::NotInitialized)));
}

// -- Events --

#[test]
fn test_register_work_emits_versioned_event() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = id(&env, 7);
    let commitment = meta(&env, "ipfs://QmCanonical", 0x47);
    let custodian = Address::generate(&env);

    client.register_work(&policy_manager, &work_id, &commitment, &custodian);

    // The testutils event buffer exposes the most recent invocation's
    // events, so exactly the single `WRK_NEW` event is visible here.
    let events = env.events().all();
    assert_eq!(events.len(), 1);
    let (_, topics, data) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("WRK_NEW"));
    // Versioned event: payload is (work_id, version, metadata_hash).
    let (event_id, version, event_hash): (BytesN<32>, u32, BytesN<32>) =
        data.try_into_val(&env).unwrap();
    assert_eq!(event_id, work_id);
    assert_eq!(version, 1);
    assert_eq!(event_hash, commitment.manifest_hash);
}

// -- Boundary (TTL renewal) --

#[test]
fn test_register_work_ttl_renews_on_read() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 8);

    env.ledger().with_mut(|l| {
        l.sequence_number += 100;
    });

    let entry = client.entry(&work_id);
    assert_eq!(entry.kind, EntryKind::Work);
}

// ===================== #929 -- works, editions, renditions =====================

// -- Positive --

#[test]
fn test_edition_and_renditions_round_trip() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x10);
    let edition_id = reg_edition(&env, &client, &policy_manager, &work_id, 0x20);
    let rendition_epub = reg_rendition(&env, &client, &policy_manager, &edition_id, 0x30);
    let rendition_pdf = reg_rendition(&env, &client, &policy_manager, &edition_id, 0x31);

    let edition: CatalogEntry = client.entry(&edition_id);
    assert_eq!(edition.kind, EntryKind::Edition);
    assert_eq!(edition.parent, Some(work_id.clone()));

    let epub: CatalogEntry = client.entry(&rendition_epub);
    assert_eq!(epub.kind, EntryKind::Rendition);
    assert_eq!(epub.parent, Some(edition_id.clone()));
    assert!(matches!(epub.content, ContentState::Committed(_)));

    let pdf: CatalogEntry = client.entry(&rendition_pdf);
    assert_eq!(pdf.kind, EntryKind::Rendition);
    assert_eq!(pdf.parent, Some(edition_id.clone()));

    // Bounded children queries: work has 1 edition, edition has 2 renditions.
    let work_children = client.children(&work_id, &0, &10);
    assert_eq!(work_children.ids.len(), 1);
    assert_eq!(work_children.ids.get(0).unwrap(), edition_id);
    assert!(work_children.done);

    let edition_children = client.children(&edition_id, &0, &10);
    assert_eq!(edition_children.ids.len(), 2);
    assert_eq!(edition_children.ids.get(0).unwrap(), rendition_epub);
    assert_eq!(edition_children.ids.get(1).unwrap(), rendition_pdf);
    assert!(edition_children.done);
}

#[test]
fn test_multiple_formats_and_editions_paginated() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x11);

    // Two editions of the same work, each with its own formats.
    let edition_a = reg_edition(&env, &client, &policy_manager, &work_id, 0x21);
    let edition_b = reg_edition(&env, &client, &policy_manager, &work_id, 0x22);
    let _a_epub = reg_rendition(&env, &client, &policy_manager, &edition_a, 0x31);
    let _a_pdf = reg_rendition(&env, &client, &policy_manager, &edition_a, 0x32);
    let _b_audio = reg_rendition(&env, &client, &policy_manager, &edition_b, 0x33);

    // Page through the work's editions, 1 at a time.
    let page1 = client.children(&work_id, &0, &1);
    assert_eq!(page1.ids.len(), 1);
    assert_eq!(page1.ids.get(0).unwrap(), edition_a);
    assert_eq!(page1.next_cursor, 1);
    assert!(!page1.done);

    let page2 = client.children(&work_id, &1, &1);
    assert_eq!(page2.ids.len(), 1);
    assert_eq!(page2.ids.get(0).unwrap(), edition_b);
    assert_eq!(page2.next_cursor, 2);
    assert!(page2.done);

    // Empty trailing page is valid and done.
    let page3 = client.children(&work_id, &2, &1);
    assert_eq!(page3.ids.len(), 0);
    assert!(page3.done);
}

#[test]
fn test_children_pagination_respects_max_page() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x12);
    for i in 0..60u8 {
        reg_edition(&env, &client, &policy_manager, &work_id, 0x40 + i);
    }

    let page1 = client.children(&work_id, &0, &50);
    assert_eq!(page1.ids.len(), 50);
    assert_eq!(page1.next_cursor, 50);
    assert!(!page1.done);

    let page2 = client.children(&work_id, &50, &50);
    assert_eq!(page2.ids.len(), 10);
    assert_eq!(page2.next_cursor, 60);
    assert!(page2.done);
}

// -- Negative (invalid parents, duplicates, bounded queries) --

#[test]
fn test_edition_under_edition_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x13);
    let edition_a = reg_edition(&env, &client, &policy_manager, &work_id, 0x23);
    let custodian = Address::generate(&env);

    // Editions can only hang off works, never off other editions.
    let res = client.try_register_edition(
        &policy_manager,
        &edition_a,
        &id(&env, 0x24),
        &meta(&env, "ipfs://QmX", 0x64),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidParent)));
}

#[test]
fn test_edition_under_missing_work_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let res = client.try_register_edition(
        &policy_manager,
        &id(&env, 0x99),
        &id(&env, 0x25),
        &meta(&env, "ipfs://QmX", 0x65),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::EntryNotFound)));
}

#[test]
fn test_rendition_under_work_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x14);
    let custodian = Address::generate(&env);

    // Renditions can only hang off editions, never directly off works.
    let res = client.try_register_rendition(
        &policy_manager,
        &work_id,
        &id(&env, 0x34),
        &content(&env, HashAlgorithm::Sha256, 0x74),
        &meta(&env, "ipfs://QmX", 0x54),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidParent)));
}

#[test]
fn test_rendition_under_missing_edition_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let res = client.try_register_rendition(
        &policy_manager,
        &id(&env, 0x98),
        &id(&env, 0x35),
        &content(&env, HashAlgorithm::Sha256, 0x75),
        &meta(&env, "ipfs://QmX", 0x55),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::EntryNotFound)));
}

#[test]
fn test_duplicate_child_id_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x15);
    let edition_id = reg_edition(&env, &client, &policy_manager, &work_id, 0x26);
    let custodian = Address::generate(&env);

    // Reusing `edition_id` as a rendition id must be rejected.
    let res = client.try_register_rendition(
        &policy_manager,
        &work_id,
        &edition_id,
        &content(&env, HashAlgorithm::Sha256, 0x76),
        &meta(&env, "ipfs://QmX", 0x56),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::AlreadyRegistered)));
}

#[test]
fn test_children_zero_limit_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x16);

    assert_eq!(
        client.try_children(&work_id, &0, &0),
        Err(Ok(ContractError::InvalidLimit))
    );
}

#[test]
fn test_children_over_max_limit_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x17);

    assert_eq!(
        client.try_children(&work_id, &0, &51),
        Err(Ok(ContractError::InvalidLimit))
    );
}

#[test]
fn test_children_cursor_beyond_list_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x18);
    reg_edition(&env, &client, &policy_manager, &work_id, 0x28);

    // Only 1 child exists; a cursor past the end is out of bounds.
    assert_eq!(
        client.try_children(&work_id, &5, &10),
        Err(Ok(ContractError::InvalidLimit))
    );
}

// -- Authorization for hierarchy writes --

#[test]
fn test_register_edition_requires_policy_manager_role() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x19);
    let admin: Address = client.get_role(&crate::Role::Admin);
    let custodian = Address::generate(&env);

    let res = client.try_register_edition(
        &admin,
        &work_id,
        &id(&env, 0x29),
        &meta(&env, "ipfs://QmX", 0x69),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

// -- Metadata commitment type sanity (used by #933 too) --

#[test]
fn test_entry_metadata_commitment_shape() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x1a);

    let entry: CatalogEntry = client.entry(&work_id);
    let MetadataCommitment { uri, manifest_hash } = entry.metadata;
    assert!(!uri.is_empty());
    assert_ne!(manifest_hash, BytesN::from_array(&env, &[0u8; 32]));
}
