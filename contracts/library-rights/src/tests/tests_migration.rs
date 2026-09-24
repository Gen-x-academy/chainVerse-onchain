use crate::{ContractError, LibraryRightsContractClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, Address, BytesN, Env, Symbol};

fn hash(env: &Env, value: u8) -> BytesN<32> {
    BytesN::from_array(env, &[value; 32])
}

fn bootstrapped(
    env: &Env,
    contract_id: &Address,
) -> (LibraryRightsContractClient, Address, Address) {
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
fn test_cursor_bounded_resumable_migration() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);
    let status = client.migration_status();
    assert!(status.active);
    assert_eq!(status.from_version, 2);
    assert_eq!(status.target_version, 3);
    assert_eq!(status.steps_done, 0);
    assert_eq!(status.total_steps, 3);
    assert!(!status.ready_to_finalize);

    // First batch: bounded by the requested limit (2), not finished.
    let (done, finished) = client.migrate_batch(&admin, &2);
    assert_eq!(done, 2);
    assert!(!finished);

    // Second batch resumes from the cursor: applies the remaining step.
    let (done, finished) = client.migrate_batch(&admin, &5);
    assert_eq!(done, 1);
    assert!(finished);

    let status = client.migration_status();
    assert!(status.active);
    assert!(status.ready_to_finalize);

    client.finalize_migration(&admin);
    assert_eq!(client.schema_version(), 3);
    assert_eq!(client.migration_log_count(), 1);
}

#[test]
fn test_writes_are_gated_during_incompatible_phase() {
    let (env, contract_id) = super::setup();
    let (client, admin, policy_manager) = bootstrapped(&env, &contract_id);

    // Register a work before any migration so the checkout path is
    // reachable in the running phase.
    let work_id = hash(&env, 1);
    let work_hash = hash(&env, 2);
    let custodian = Address::generate(&env);
    client.put_work(&policy_manager, &work_id, &work_hash, &custodian);

    client.begin_migration(&admin, &3);
    let borrower = Address::generate(&env);

    // Normal writes are frozen while the incompatible phase is active.
    let result = client.try_borrow_work(&borrower, &work_id, &borrower);
    assert_eq!(result, Err(Ok(ContractError::MigrationInProgress)));

    // The phase is lifted by completing the migration; writes resume.
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);
    assert_eq!(client.schema_version(), 3);
    let result = client.try_borrow_work(&borrower, &work_id, &borrower);
    assert!(result.is_ok());
}

#[test]
fn test_gate_blocks_policy_and_catalog_writes_too() {
    let (env, contract_id) = super::setup();
    let (client, admin, policy_manager) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);

    // put_work (catalog write) is refused before any policy lookup runs.
    let work_id = hash(&env, 5);
    let result = client.try_put_work(
        &policy_manager,
        &work_id,
        &hash(&env, 6),
        &Address::generate(&env),
    );
    assert_eq!(result, Err(Ok(ContractError::MigrationInProgress)));

    // place_hold (lending write) is refused before work lookup runs.
    let holder = Address::generate(&env);
    let result = client.try_place_hold(&holder, &work_id, &holder);
    assert_eq!(result, Err(Ok(ContractError::MigrationInProgress)));
}

#[test]
fn test_migration_logs_append_only_completions() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    // Migration 2 -> 3.
    client.begin_migration(&admin, &3);
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);
    assert_eq!(client.schema_version(), 3);
    assert_eq!(client.migration_log_count(), 1);

    // Migration 3 -> 4, continuing from the new schema.
    client.begin_migration(&admin, &4);
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);
    assert_eq!(client.schema_version(), 4);
    assert_eq!(client.migration_log_count(), 2);

    let status = client.migration_status();
    assert_eq!(status.from_version, 4);
    assert_eq!(status.target_version, 4);
    assert!(!status.active);
}

// -- Negative (cannot skip / double-start) --

#[test]
fn test_cannot_skip_versions() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    let result = client.try_begin_migration(&admin, &5);
    assert_eq!(result, Err(Ok(ContractError::MigrationVersionSkip)));
    assert!(!client.migration_status().active);
}

#[test]
fn test_begin_same_version_is_already_complete() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    let result = client.try_begin_migration(&admin, &2);
    assert_eq!(result, Err(Ok(ContractError::MigrationAlreadyComplete)));
}

#[test]
fn test_cannot_begin_twice() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);
    let result = client.try_begin_migration(&admin, &4);
    assert_eq!(result, Err(Ok(ContractError::MigrationInProgress)));
}

#[test]
fn test_finalize_too_early_fails() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);
    let result = client.try_finalize_migration(&admin);
    assert_eq!(result, Err(Ok(ContractError::MigrationNotReady)));
}

#[test]
fn test_finalize_without_migration_fails() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    let result = client.try_finalize_migration(&admin);
    assert_eq!(result, Err(Ok(ContractError::MigrationNotStarted)));
}

#[test]
fn test_finalize_twice_fails() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);

    let result = client.try_finalize_migration(&admin);
    assert_eq!(result, Err(Ok(ContractError::MigrationAlreadyComplete)));
}

// -- Authorization --

#[test]
fn test_migration_is_admin_only() {
    let (env, contract_id) = super::setup();
    let (client, _, _) = bootstrapped(&env, &contract_id);
    let non_admin = Address::generate(&env);

    assert_eq!(
        client.try_begin_migration(&non_admin, &3),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_migrate_batch(&non_admin, &1),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(
        client.try_finalize_migration(&non_admin),
        Err(Ok(ContractError::NotAdmin))
    );
}

// -- Boundary (idempotency) --

#[test]
fn test_running_migration_without_state_is_a_noop() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    // No migration started: the batch is a harmless no-op, never an error.
    let (done, finished) = client.migrate_batch(&admin, &10);
    assert_eq!(done, 0);
    assert!(!finished);

    // Once everything completed, repeated batches stay no-ops.
    client.begin_migration(&admin, &3);
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);
    let (done, finished) = client.migrate_batch(&admin, &10);
    assert_eq!(done, 0);
    assert!(!finished);
}

// -- Events --

#[test]
fn test_migration_emits_begin_and_complete_events() {
    let (env, contract_id) = super::setup();
    let (client, admin, _) = bootstrapped(&env, &contract_id);

    client.begin_migration(&admin, &3);
    client.migrate_batch(&admin, &3);
    client.finalize_migration(&admin);

    let events = env.events().all();
    // MIG_BEGIN + 3x MIG_STEP + MIG_COMPLETE.
    assert_eq!(events.len(), 5);

    let (_, topics, _) = events.get(0).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("MIG_BEGIN"));

    let (_, topics, _) = events.get(4).unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("MIG_COMPLETE"));
}