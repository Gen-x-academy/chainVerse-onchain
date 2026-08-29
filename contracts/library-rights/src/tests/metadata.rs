//! Tests for #933 -- bounded metadata URI commitments.
//!
//! URIs are scheme-allowlisted (`ipfs://`, `ipns://`, `https://`,
//! `ar://`), bounded to [`crate::metadata::METADATA_URI_MAX_LEN`]
//! characters, and every update creates a new version while the previous
//! version's commitment stays immutable.

use crate::{
    CatalogEntry, ContractError, EntryKind, LibraryRightsContractClient, MetadataCommitment,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String};

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

/// Registers a work with the given metadata URI and returns its id.
fn reg_work(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    byte: u8,
    metadata_uri: &str,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    let work_id = id(env, byte);
    client.register_work(
        policy_manager,
        &work_id,
        &meta(env, metadata_uri, byte + 0x40),
        &custodian,
    );
    work_id
}

// -- Positive --

#[test]
fn test_ipfs_uri_registers_and_round_trips() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(
        &env,
        &client,
        &policy_manager,
        1,
        "ipfs://QmCanonicalMetadata",
    );

    let entry: CatalogEntry = client.entry(&work_id);
    assert_eq!(
        entry.metadata,
        meta(&env, "ipfs://QmCanonicalMetadata", 0x41)
    );
    assert_eq!(entry.version, 1);
}

#[test]
fn test_https_and_ar_schemes_allowed() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let https_work = reg_work(
        &env,
        &client,
        &policy_manager,
        2,
        "https://catalog.example/meta/1",
    );
    assert_eq!(
        client.entry(&https_work).metadata.uri,
        uri(&env, "https://catalog.example/meta/1")
    );

    let ar_work = reg_work(&env, &client, &policy_manager, 3, "ar://arweave-tx-id");
    assert_eq!(
        client.entry(&ar_work).metadata.uri,
        uri(&env, "ar://arweave-tx-id")
    );
}

#[test]
fn test_update_metadata_creates_version_and_keeps_history() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 4, "ipfs://QmOld");

    let new_version =
        client.update_metadata(&policy_manager, &work_id, &meta(&env, "ipfs://QmNew", 0x55));
    assert_eq!(new_version, 2);

    let entry: CatalogEntry = client.entry(&work_id);
    assert_eq!(entry.version, 2);
    assert_eq!(entry.metadata.uri, uri(&env, "ipfs://QmNew"));

    // Version 1's commitment is immutable.
    let v1 = client.entry_version(&work_id, &1);
    assert_eq!(v1.metadata.uri, uri(&env, "ipfs://QmOld"));
    assert_eq!(client.entry_version_count(&work_id), 2);
}

#[test]
fn test_update_metadata_on_edition_bumps_version() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 5, "ipfs://QmWork");
    let custodian = Address::generate(&env);
    let edition_id = id(&env, 0x15);
    client.register_edition(
        &policy_manager,
        &work_id,
        &edition_id,
        &meta(&env, "ipfs://QmEditionV1", 0x45),
        &custodian,
    );

    let new_version = client.update_metadata(
        &policy_manager,
        &edition_id,
        &meta(&env, "ipfs://QmEditionV2", 0x55),
    );
    assert_eq!(new_version, 2);

    let entry: CatalogEntry = client.entry(&edition_id);
    assert_eq!(entry.kind, EntryKind::Edition);
    assert_eq!(entry.version, 2);
}

// -- Negative (scheme and length validation) --

#[test]
fn test_non_allowlisted_schemes_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    for bad in [
        "http://catalog.example/meta/1", // plain http is not allowlisted
        "file:///etc/passwd",            // local filesystem
        "javascript:alert(1)",           // script scheme
        "data:text/plain,hi",            // data URI
        "http://evil/ipfs://QmX",        // allowlisted scheme embedded mid-string
        "no-scheme-at-all",
    ] {
        let res = client.try_register_work(
            &policy_manager,
            &id(&env, 0x60),
            &meta(&env, bad, 0x70),
            &custodian,
        );
        assert_eq!(
            res,
            Err(Ok(ContractError::InvalidMetadataUri)),
            "scheme: {bad}"
        );
    }
}

#[test]
fn test_empty_uri_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let res = client.try_register_work(
        &policy_manager,
        &id(&env, 6),
        &meta(&env, "", 0x46),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidMetadataUri)));
}

#[test]
fn test_uri_length_boundary() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    // Exactly the max length is accepted.
    let mut at_limit_bytes = [b'a'; 200];
    at_limit_bytes[..7].copy_from_slice(b"ipfs://");
    let ok = client.try_register_work(
        &policy_manager,
        &id(&env, 7),
        &MetadataCommitment {
            uri: String::from_bytes(&env, &at_limit_bytes),
            manifest_hash: id(&env, 0x47),
        },
        &custodian,
    );
    assert_eq!(ok, Ok(Ok(1)));

    // One byte over the max length is rejected.
    let mut over_limit_bytes = [b'a'; 201];
    over_limit_bytes[..7].copy_from_slice(b"ipfs://");
    let res = client.try_register_work(
        &policy_manager,
        &id(&env, 8),
        &MetadataCommitment {
            uri: String::from_bytes(&env, &over_limit_bytes),
            manifest_hash: id(&env, 0x48),
        },
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidMetadataUri)));
}

#[test]
fn test_zero_manifest_hash_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);

    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let res = client.try_register_work(
        &policy_manager,
        &id(&env, 9),
        &MetadataCommitment {
            uri: uri(&env, "ipfs://QmX"),
            manifest_hash: zero,
        },
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidHash)));
}

#[test]
fn test_update_metadata_rejects_invalid_uri() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x0a, "ipfs://QmOld");

    let res =
        client.try_update_metadata(&policy_manager, &work_id, &meta(&env, "ftp://legacy", 0x4a));
    assert_eq!(res, Err(Ok(ContractError::InvalidMetadataUri)));
}

// -- Authorization & NotFound --

#[test]
fn test_update_metadata_unauthorized() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work(&env, &client, &policy_manager, 0x0b, "ipfs://QmOld");
    let admin: Address = client.get_role(&crate::Role::Admin);

    let res = client.try_update_metadata(&admin, &work_id, &meta(&env, "ipfs://QmNew", 0x4b));
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_update_metadata_missing_entry_fails() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let res = client.try_update_metadata(
        &policy_manager,
        &id(&env, 0x7c),
        &meta(&env, "ipfs://QmNew", 0x4c),
    );
    assert_eq!(res, Err(Ok(ContractError::EntryNotFound)));
}
