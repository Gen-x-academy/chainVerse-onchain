//! Boundary, authorization, and state-matrix tests for the global
//! emergency pause (#997).

use super::*;
use soroban_sdk::{
    testutils::Address as _, Address, BytesN, Env, String, Symbol,
};

const DAY: u64 = 24 * 60 * 60;

fn setup() -> (
    Env,
    LibraryRightsContractClient,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LibraryRightsContract);
    let client = LibraryRightsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let policy_manager = Address::generate(&env);
    let emergency = Address::generate(&env);
    let librarian = Address::generate(&env);

    client.bootstrap(&admin, &treasury, &policy_manager, &emergency, &librarian);

    (
        env,
        client,
        admin,
        treasury,
        policy_manager,
        emergency,
        librarian,
    )
}

fn reason(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn zero(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0; 32])
}

/// Registers a policy + a work so obligation flows have valid inputs.
fn put_work(
    env: &Env,
    client: &LibraryRightsContractClient,
    policy_manager: &Address,
) -> (BytesN<32>, BytesN<32>) {
    let work_id = BytesN::from_array(env, &[1; 32]);
    let work_hash = BytesN::from_array(env, &[2; 32]);
    let custodian = Address::generate(env);
    let policy = Symbol::new(env, "P1");
    client.put_policy(policy_manager, &policy, &1, &3);
    client.put_work(policy_manager, &work_id, &work_hash, &custodian, &policy);
    (work_id, work_hash)
}

#[test]
fn test_pause_blocks_new_obligations_but_allows_returns() {
    let (env, client, _admin, _treasury, pm, emergency, _librarian) = setup();

    // Establish a live loan before pausing.
    let (work_id, _) = put_work(&env, &client, &pm);
    let borrower = Address::generate(&env);
    let other = Address::generate(&env);
    client.borrow_work(&borrower, &work_id, &borrower);

    client.pause_library(&emergency, &reason(&env, 9));
    assert!(client.is_paused());
    let status = client.pause_status();
    assert!(status.paused);
    assert_eq!(status.paused_by, Some(emergency));
    assert_eq!(status.reason_hash, Some(reason(&env, 9)));

    // New obligations are blocked...
    assert_eq!(
        client.try_borrow_work(&other, &work_id, &other),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        client.try_place_hold(&other, &work_id, &other),
        Err(Ok(ContractError::Paused))
    );

    // ...but returns are still allowed and unwind the position.
    client.return_work(&borrower, &work_id, &borrower);
}

#[test]
fn test_unpause_requires_admin_designation() {
    let (env, client, admin, _treasury, pm, emergency, _librarian) = setup();
    let (work_id, _) = put_work(&env, &client, &pm);

    client.pause_library(&emergency, &reason(&env, 9));

    // PolicyManager is not eligible to lift the pause.
    assert_eq!(
        client.try_unpause_library(&pm, &reason(&env, 10)),
        Err(Ok(ContractError::NotAdmin))
    );
    assert!(client.is_paused());

    // Admin restores operation.
    client.unpause_library(&admin, &reason(&env, 11));
    assert!(!client.is_paused());

    // Obligations work again after the governed unpause.
    let borrower = Address::generate(&env);
    client.borrow_work(&borrower, &work_id, &borrower);
}

#[test]
fn test_pause_rejects_zero_reason_and_policy_manager_cannot_pause() {
    let (env, client, admin, _treasury, pm, emergency, _librarian) = setup();

    // Zero reason hashes are rejected at both ends of the lifecycle.
    assert_eq!(
        client.try_pause_library(&emergency, &zero(&env)),
        Err(Ok(ContractError::InvalidHash))
    );
    client.pause_library(&emergency, &reason(&env, 1));
    assert_eq!(
        client.try_unpause_library(&admin, &zero(&env)),
        Err(Ok(ContractError::InvalidHash))
    );

    // A role that is neither Admin nor Emergency cannot pause.
    assert_eq!(
        client.try_pause_library(&pm, &reason(&env, 2)),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_pause_state_machine_boundaries() {
    let (env, client, admin, _treasury, _pm, emergency, _librarian) = setup();

    // Pausing twice is rejected; the second call is not a no-op.
    client.pause_library(&emergency, &reason(&env, 1));
    assert_eq!(
        client.try_pause_library(&admin, &reason(&env, 2)),
        Err(Ok(ContractError::AlreadyPaused))
    );

    // Unpausing an already-running library is rejected.
    client.unpause_library(&admin, &reason(&env, 3));
    assert_eq!(
        client.try_unpause_library(&admin, &reason(&env, 4)),
        Err(Ok(ContractError::NotPaused))
    );
}

#[test]
fn test_reads_and_version_stay_available_while_paused() {
    let (env, client, _admin, _treasury, pm, emergency, _librarian) = setup();
    let (work_id, _) = put_work(&env, &client, &pm);

    client.pause_library(&emergency, &reason(&env, 5));

    // Read-only queries remain fully available during the incident.
    client.get_work(&work_id).expect("read still available");
    client
        .content_status(&work_id)
        .expect("status read still available");
    assert_eq!(client.version(), String::from_str(&env, "1.0.0"));
}

#[test]
fn test_every_obligation_entrypoint_is_boundary_covered() {
    let (env, client, admin, _treasury, pm, emergency, _librarian) = setup();
    let borrower = Address::generate(&env);
    let (work_id, work_hash) = put_work(&env, &client, &pm);
    let custodian = Address::generate(&env);
    let policy = Symbol::new(&env, "P1");

    client.pause_library(&emergency, &reason(&env, 6));

    // Loans / holds / reserves.
    assert_eq!(
        client.try_borrow_work(&borrower, &work_id, &borrower),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        client.try_checkout_work(&borrower, &work_id, &1000, &false, &0, &5000),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        client.try_place_hold(&borrower, &work_id, &borrower),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        client.try_claim_hold(&borrower, &work_id, &borrower),
        Err(Ok(ContractError::Paused))
    );

    // Catalog / policy / licensing writes.
    assert_eq!(
        client.try_put_policy(&pm, &Symbol::new(&env, "P2"), &2, &4),
        Err(Ok(ContractError::Paused))
    );
    assert_eq!(
        client.try_put_work(&pm, &work_id, &work_hash, &custodian, &policy),
        Err(Ok(ContractError::Paused))
    );
    let reserve_course = BytesN::from_array(&env, &[3; 32]);
    assert_eq!(
        client.try_create_reserve(
            &pm,
            &work_id,
            &reserve_course,
            &1,
            &(env.ledger().timestamp() + DAY),
        ),
        Err(Ok(ContractError::Paused))
    );

    // Keeper configuration is also frozen during the pause.
    assert_eq!(
        client.try_add_keeper(&admin, &borrower),
        Err(Ok(ContractError::Paused))
    );
}