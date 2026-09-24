use crate::wasm_info::DeployInfo;
use crate::{ContractError, LibraryRightsContractClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, Address, BytesN, Env, Symbol};

const HASH_A: [u8; 32] = [7; 32];
const HASH_B: [u8; 32] = [9; 32];

fn hash(env: &Env, arr: &[u8; 32]) -> BytesN<32> {
    BytesN::from_array(env, arr)
}

fn bootstrapped(env: &Env, contract_id: &Address) -> (LibraryRightsContractClient, Address, Address) {
    env.mock_all_auths();
    let client = LibraryRightsContractClient::new(env, contract_id);
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let policy_manager = Address::generate(env);
    let emergency = Address::generate(env);
    let librarian = Address::generate(env);
    client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);
    (client, admin, policy_manager)
}

// -- Positive --

#[test]
fn test_schema_and_abi_queries_after_bootstrap() {
    let (env, contract_id) = super::setup();
    let (client, _, _) = bootstrapped(&env, &contract_id);

    // Bootstrap stamps SCHEMA_VERSION; the semantic ABI revision is a
    // build constant.
    assert_eq!(client.schema_version(), 2);
    assert_eq!(client.abi_version(), 1);

    let info = client.deploy_info();
    assert_eq!(
        info,
        DeployInfo {
            schema_version: 2,
            abi_version: 1,
            approved_wasm_hash: hash(&env, &[0; 32]),
        }
    );
}

#[test]
fn test_queries_are_zero_before_bootstrap() {
    let (env, contract_id) = super::setup();
    let client = LibraryRightsContractClient::new(&env, &contract_id);

    assert_eq!(client.schema_version(), 0);
    assert_eq!(client.abi_version(), 1);
    // Unset approval is the all-zero digest, never a usable hash.
    assert_eq!(client.approved_wasm_hash(), hash(&env, &[0; 32]));
}

#[test]
fn test_approve_wasm_hash_updates_and_emits_upgrade_event() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.approve_wasm_hash(&admin, &hash(&env, &HASH_A));
    assert_eq!(client.approved_wasm_hash(), hash(&env, &HASH_A));
    assert_eq!(client.deploy_info().approved_wasm_hash, hash(&env, &HASH_A));

    // Re-approval is the governed upgrade path; the event carries both
    // digests so deployment tooling can verify the transition.
    client.approve_wasm_hash(&admin, &hash(&env, &HASH_B));
    assert_eq!(client.approved_wasm_hash(), hash(&env, &HASH_B));

    let events = env.events().all();
    assert_eq!(events.len(), 2);
    let (_, topics, _) = events.get(events.len() - 1).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("UPGRADE_AUTH"));
}

// -- Negative (input validation) --

#[test]
fn test_approve_wasm_hash_rejects_zero_digest() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    let result = client.try_approve_wasm_hash(&admin, &hash(&env, &[0; 32]));
    assert_eq!(result, Err(Ok(ContractError::InvalidHash)));
    // Unset approval stays unset.
    assert_eq!(client.approved_wasm_hash(), hash(&env, &[0; 32]));
}

// -- Authorization --

#[test]
fn test_approve_wasm_hash_requires_admin() {
    let (env, contract_id) = super::setup();
    let (client, _, policy_manager) = bootstrapped(&env, &contract_id);

    let result = client.try_approve_wasm_hash(&policy_manager, &hash(&env, &HASH_A));
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// -- Boundary --

#[test]
fn test_deploy_info_is_self_consistent_across_upgrades() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    let before = client.deploy_info();
    // The schema/ABI surface does not change because of a WASM approval;
    // only the approved digest reflects the (single) governed mutation.
    client.approve_wasm_hash(&admin, &hash(&env, &HASH_A));
    let after = client.deploy_info();

    assert_eq!(before.schema_version, after.schema_version);
    assert_eq!(before.abi_version, after.abi_version);
    assert_ne!(before.approved_wasm_hash, after.approved_wasm_hash);
}