//! Tests for #932 -- anchor digital content hashes.
//!
//! Deterministic test vectors (FIPS 180-4 / NIST):
//! - `sha256("abc")` = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
//! - `sha512("abc")` = ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f

use crate::{
    CatalogEntry, ContentCommitment, ContentState, ContractError, HashAlgorithm,
    LibraryRightsContractClient, MetadataCommitment,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String};

const SHA256_ABC: [u8; 32] = [
    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
];

const SHA512_ABC: [u8; 32] = [
    0xdd, 0xaf, 0x35, 0xa1, 0x93, 0x61, 0x7a, 0xba, 0xcc, 0x41, 0x73, 0x49, 0xae, 0x20, 0x41, 0x31,
    0x12, 0xe6, 0xfa, 0x4e, 0x89, 0xa9, 0x7e, 0xa2, 0x0a, 0x9e, 0xee, 0xe6, 0x4b, 0x55, 0xd3, 0x9a,
];

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

fn meta(env: &Env, uri_str: &str, hash_byte: u8) -> MetadataCommitment {
    MetadataCommitment {
        uri: String::from_str(env, uri_str),
        manifest_hash: id(env, hash_byte),
    }
}

/// Registers work -> edition -> rendition with the given digest and
/// returns (work_id, edition_id, rendition_id).
fn reg_rendition(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    byte: u8,
    commitment: &ContentCommitment,
) -> (BytesN<32>, BytesN<32>, BytesN<32>) {
    let custodian = Address::generate(env);
    let work_id = id(env, byte);
    let edition_id = id(env, byte + 0x10);
    let rendition_id = id(env, byte + 0x20);
    client.register_work(
        policy_manager,
        &work_id,
        &meta(env, "ipfs://QmWork", byte + 0x40),
        &custodian,
    );
    client.register_edition(
        policy_manager,
        &work_id,
        &edition_id,
        &meta(env, "ipfs://QmEdition", byte + 0x50),
        &custodian,
    );
    client.register_rendition(
        policy_manager,
        &edition_id,
        &rendition_id,
        commitment,
        &meta(env, "ipfs://QmRendition", byte + 0x60),
        &custodian,
    );
    (work_id, edition_id, rendition_id)
}

fn content(algorithm: HashAlgorithm, digest: BytesN<32>) -> ContentCommitment {
    ContentCommitment { algorithm, digest }
}

// -- Positive (deterministic vectors) --

#[test]
fn test_sha256_vector_registers_and_verifies() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest.clone());

    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 1, &commitment);

    assert!(client.verify_content(&rendition_id, &HashAlgorithm::Sha256, &digest));
}

#[test]
fn test_sha512_vector_registers_and_verifies() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA512_ABC);
    let commitment = content(HashAlgorithm::Sha512, digest.clone());

    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 2, &commitment);

    assert!(client.verify_content(&rendition_id, &HashAlgorithm::Sha512, &digest));
}

#[test]
fn test_registered_digest_is_stored_on_entry() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest.clone());

    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 3, &commitment);

    let entry: CatalogEntry = client.entry(&rendition_id);
    match entry.content {
        ContentState::Committed(stored) => {
            assert_eq!(stored.algorithm, HashAlgorithm::Sha256);
            assert_eq!(stored.digest, digest);
        }
        ContentState::None => panic!("rendition must carry a content commitment"),
    }
}

// -- Negative (verification mismatch, zero digest) --

#[test]
fn test_verify_content_wrong_digest_false() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest.clone());
    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 4, &commitment);

    let wrong = id(&env, 0x7f);
    assert!(!client.verify_content(&rendition_id, &HashAlgorithm::Sha256, &wrong));
}

#[test]
fn test_verify_content_wrong_algorithm_false() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest.clone());
    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 5, &commitment);

    assert!(!client.verify_content(&rendition_id, &HashAlgorithm::Sha512, &digest));
}

#[test]
fn test_verify_content_on_non_rendition_false() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = id(&env, 6);
    let custodian = Address::generate(&env);
    client.register_work(
        &policy_manager,
        &work_id,
        &meta(&env, "ipfs://QmWork", 0x46),
        &custodian,
    );

    // Works have no content commitment; verification is `false`, not an error.
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    assert!(!client.verify_content(&work_id, &HashAlgorithm::Sha256, &digest));
}

#[test]
fn test_register_rendition_rejects_zero_digest() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work_only(&env, &client, &policy_manager, 7);
    let edition_id = reg_edition_only(&env, &client, &policy_manager, &work_id, 0x17);
    let custodian = Address::generate(&env);

    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let bad = ContentCommitment {
        algorithm: HashAlgorithm::Sha256,
        digest: zero,
    };
    let res = client.try_register_rendition(
        &policy_manager,
        &edition_id,
        &id(&env, 0x27),
        &bad,
        &meta(&env, "ipfs://QmR", 0x67),
        &custodian,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidHash)));
}

#[test]
fn test_verify_content_missing_rendition_fails() {
    let (env, contract_id) = super::setup();
    let (client, _policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);

    let res = client.try_verify_content(&id(&env, 0x77), &HashAlgorithm::Sha256, &digest);
    assert_eq!(res, Err(Ok(ContractError::EntryNotFound)));
}

// -- Immutability per version --

#[test]
fn test_update_content_hash_bumps_version_and_keeps_history() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest_a = BytesN::from_array(&env, &SHA256_ABC);
    let commitment_a = content(HashAlgorithm::Sha256, digest_a.clone());
    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 8, &commitment_a);

    // Publish a corrected artifact: new digest, new version.
    let digest_b = id(&env, 0x88);
    let commitment_b = content(HashAlgorithm::Sha512, digest_b.clone());
    let new_version = client.update_content_hash(&policy_manager, &rendition_id, &commitment_b);
    assert_eq!(new_version, 2);

    // The current entry points at the new commitment...
    let entry: CatalogEntry = client.entry(&rendition_id);
    assert_eq!(entry.version, 2);
    match &entry.content {
        ContentState::Committed(stored) => {
            assert_eq!(stored.algorithm, HashAlgorithm::Sha512);
            assert_eq!(stored.digest, digest_b);
        }
        ContentState::None => panic!("rendition must carry a content commitment"),
    }

    // ...while version 1's snapshot keeps the original hash immutable.
    let v1 = client.entry_version(&rendition_id, &1);
    match v1.content {
        ContentState::Committed(stored) => {
            assert_eq!(stored.algorithm, HashAlgorithm::Sha256);
            assert_eq!(stored.digest, digest_a);
        }
        ContentState::None => panic!("version 1 must carry a content commitment"),
    }
    let v2 = client.entry_version(&rendition_id, &2);
    match v2.content {
        ContentState::Committed(stored) => assert_eq!(stored.digest, digest_b),
        ContentState::None => panic!("version 2 must carry a content commitment"),
    }
    assert_eq!(client.entry_version_count(&rendition_id), 2);
}

#[test]
fn test_update_content_hash_on_work_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let work_id = reg_work_only(&env, &client, &policy_manager, 9);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest);

    let res = client.try_update_content_hash(&policy_manager, &work_id, &commitment);
    assert_eq!(res, Err(Ok(ContractError::InvalidKind)));
}

#[test]
fn test_update_content_hash_missing_rendition_fails() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest);

    let res = client.try_update_content_hash(&policy_manager, &id(&env, 0x79), &commitment);
    assert_eq!(res, Err(Ok(ContractError::EntryNotFound)));
}

// -- Authorization --

#[test]
fn test_update_content_hash_unauthorized() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let digest = BytesN::from_array(&env, &SHA256_ABC);
    let commitment = content(HashAlgorithm::Sha256, digest.clone());
    let (_, _, rendition_id) = reg_rendition(&env, &client, &policy_manager, 0x0a, &commitment);

    let admin: Address = client.get_role(&crate::Role::Admin);
    let res = client.try_update_content_hash(&admin, &rendition_id, &commitment);
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

// -- Helpers --

fn reg_work_only(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    byte: u8,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    let work_id = id(env, byte);
    client.register_work(
        policy_manager,
        &work_id,
        &meta(env, "ipfs://QmWork", byte + 0x40),
        &custodian,
    );
    work_id
}

fn reg_edition_only(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
    parent: &BytesN<32>,
    byte: u8,
) -> BytesN<32> {
    let custodian = Address::generate(env);
    let edition_id = id(env, byte);
    client.register_edition(
        policy_manager,
        parent,
        &edition_id,
        &meta(env, "ipfs://QmEdition", byte + 0x50),
        &custodian,
    );
    edition_id
}
