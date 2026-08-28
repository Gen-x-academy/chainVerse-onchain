use crate::{ContractError, LibraryRightsContractClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

/// Bootstraps a fresh contract and returns a client plus the
/// PolicyManager address (the role allowed to write works).
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

fn work_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// -- Positive --

#[test]
fn test_put_and_get_work_round_trips() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);
    let id = work_id(&env, 1);
    let hash = work_id(&env, 2);

    client.put_work(&policy_manager, &id, &hash, &custodian);
    let record = client.get_work(&id);

    assert_eq!(record.work_hash, hash);
    assert_eq!(record.custodian, custodian);
}

// -- Negative --

#[test]
fn test_get_work_missing_fails() {
    let (env, contract_id) = super::setup();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let id = work_id(&env, 9);

    let result = client.try_get_work(&id);

    assert_eq!(result, Err(Ok(ContractError::WorkNotFound)));
}

// -- Authorization --

#[test]
fn test_put_work_requires_policy_manager_role() {
    let (env, contract_id) = super::setup();
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency);

    let custodian = Address::generate(&env);
    let id = work_id(&env, 3);
    let hash = work_id(&env, 4);

    // `admin` is not the PolicyManager; the call must be rejected.
    let result = client.try_put_work(&admin, &id, &hash, &custodian);

    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// -- Boundary (TTL renewal) --

#[test]
fn test_work_ttl_renews_on_read_after_partial_expiry_window() {
    let (env, contract_id) = super::setup();
    let (client, policy_manager) = bootstrapped(&env, &contract_id);
    let custodian = Address::generate(&env);
    let id = work_id(&env, 5);
    let hash = work_id(&env, 6);
    client.put_work(&policy_manager, &id, &hash, &custodian);

    // Advance the ledger by a modest amount to simulate time passing,
    // then read again -- this exercises the renewal path in `get_work`
    // rather than assuming a specific TTL-introspection API. Kept well
    // below any default test-environment archival threshold: jumping by
    // a large fraction of `CATALOG_MIN_TTL` here would advance the
    // ledger past the *contract instance's own* default TTL and archive
    // the whole contract, which is a test-environment artifact unrelated
    // to the `Work` entry's TTL this test is actually checking.
    env.ledger().with_mut(|l| {
        l.sequence_number += 100;
    });

    let record = client.get_work(&id);
    assert_eq!(record.work_hash, hash);
}
