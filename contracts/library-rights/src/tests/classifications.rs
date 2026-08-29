use crate::{ClassificationCommit, ClassificationKind, ContractError, LibraryRightsContractClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

/// Bootstraps a fresh contract and returns a client plus the
/// PolicyManager address (the role allowed to commit classifications).
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

fn hash(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// -- Positive --

#[test]
fn test_commit_and_read_classification_round_trips() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let h = hash(&env, 1);
    client.commit_classification(&policy_manager, &ClassificationKind::Taxonomy, &h, &2u32);

    let commit = client.get_classification(&ClassificationKind::Taxonomy);
    assert_eq!(commit.manifest_hash, h);
    assert_eq!(commit.schema_version, 2);
    assert_eq!(commit.issuer, policy_manager);
    assert_eq!(commit.previous_hash, None);
    assert_eq!(
        client.classification_history_len(&ClassificationKind::Taxonomy),
        1
    );
}

#[test]
fn test_commit_updates_preserve_provenance_and_append_history() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let first = hash(&env, 1);
    let second = hash(&env, 2);
    client.commit_classification(
        &policy_manager,
        &ClassificationKind::Audience,
        &first,
        &1u32,
    );
    client.commit_classification(
        &policy_manager,
        &ClassificationKind::Audience,
        &second,
        &2u32,
    );

    // The current commitment points back at the previous hash...
    let current = client.get_classification(&ClassificationKind::Audience);
    assert_eq!(current.manifest_hash, second);
    assert_eq!(current.previous_hash, Some(first.clone()));

    // ...and both entries remain queryable in the append-only history.
    assert_eq!(
        client.classification_history_len(&ClassificationKind::Audience),
        2
    );
    let first_entry = client.classification_history(&ClassificationKind::Audience, &1u64);
    assert_eq!(first_entry.manifest_hash, first);
    assert_eq!(first_entry.previous_hash, None);
    let second_entry = client.classification_history(&ClassificationKind::Audience, &2u64);
    assert_eq!(second_entry.manifest_hash, second);
    assert_eq!(second_entry.previous_hash, Some(first));
}

#[test]
fn test_kinds_are_independent() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    client.commit_classification(
        &policy_manager,
        &ClassificationKind::Taxonomy,
        &hash(&env, 1),
        &1u32,
    );

    // The audience kind has its own, empty history.
    assert_eq!(
        client.classification_history_len(&ClassificationKind::Audience),
        0
    );
    assert_eq!(
        client.try_get_classification(&ClassificationKind::Audience),
        Err(Ok(ContractError::ClassificationNotFound))
    );
}

// -- Negative --

#[test]
fn test_zero_hash_rejected() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let res = client.try_commit_classification(
        &policy_manager,
        &ClassificationKind::Taxonomy,
        &zero,
        &1u32,
    );
    assert_eq!(res, Err(Ok(ContractError::InvalidHash)));
    // Nothing was written.
    assert_eq!(
        client.classification_history_len(&ClassificationKind::Taxonomy),
        0
    );
}

// -- Authorization --

#[test]
fn test_commit_requires_policy_manager_role() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);

    // `admin` is not the PolicyManager; the call must be rejected.
    let res = client.try_commit_classification(
        &admin,
        &ClassificationKind::Taxonomy,
        &hash(&env, 1),
        &1u32,
    );
    assert_eq!(res, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_commit_before_bootstrap_fails() {
    let (env, contract_id) = super::setup();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let caller = Address::generate(&env);

    let res = client.try_commit_classification(
        &caller,
        &ClassificationKind::Taxonomy,
        &hash(&env, 1),
        &1u32,
    );
    assert_eq!(res, Err(Ok(ContractError::NotInitialized)));
}

// -- Boundary --

#[test]
fn test_history_queryable_only_within_bounds() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);

    client.commit_classification(
        &policy_manager,
        &ClassificationKind::Taxonomy,
        &hash(&env, 1),
        &1u32,
    );

    // Index 0 and any index past the length are rejected.
    assert_eq!(
        client.try_classification_history(&ClassificationKind::Taxonomy, &0u64),
        Err(Ok(ContractError::ClassificationNotFound))
    );
    assert_eq!(
        client.try_classification_history(&ClassificationKind::Taxonomy, &2u64),
        Err(Ok(ContractError::ClassificationNotFound))
    );
    assert_eq!(
        client.try_classification_history(&ClassificationKind::Audience, &1u64),
        Err(Ok(ContractError::ClassificationNotFound))
    );
}

#[test]
fn test_commit_record_carries_ledger_timestamp() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    env.ledger().set_timestamp(1_700_000_000);

    client.commit_classification(
        &policy_manager,
        &ClassificationKind::Taxonomy,
        &hash(&env, 1),
        &1u32,
    );

    let commit: ClassificationCommit = client.get_classification(&ClassificationKind::Taxonomy);
    assert_eq!(commit.committed_at, 1_700_000_000);
}
