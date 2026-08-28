use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

use crate::{
    AttestationReason, AttestationStatus, LibraryPhysical, LibraryPhysicalClient, PhysicalError,
};

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LibraryPhysical, ());
    (env, contract_id)
}

fn setup_with_admin() -> (Env, Address, Address) {
    let (env, contract_id) = setup();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, contract_id, admin)
}

fn loan_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn evidence(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

// ===== Positive paths =====

#[test]
fn test_initialize_sets_admin() {
    let (env, contract_id) = setup();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(PhysicalError::AlreadyInitialized))
    );
}

#[test]
fn test_attest_creates_open_record() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let lid = loan_id(&env);
    let eh = evidence(&env, 5);
    let id = client.attest(&admin, &lid, &AttestationReason::Lost, &eh);
    let rec = client.get_attestation(&id);
    assert_eq!(rec.loan_id, lid);
    assert_eq!(rec.reason, AttestationReason::Lost);
    assert_eq!(rec.status, AttestationStatus::Open);
    assert!(!rec.charged);
}

#[test]
fn test_history_starts_with_initial_hash() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let eh = evidence(&env, 7);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Damaged, &eh);
    let history = client.get_history(&id);
    assert_eq!(history.len(), 1);
    assert_eq!(history.get(0).unwrap(), eh);
}

#[test]
fn test_append_correction_grows_history() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 1));
    let correction = evidence(&env, 2);
    client.append_correction(&admin, &id, &correction);
    let history = client.get_history(&id);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(1).unwrap(), correction);
}

#[test]
fn test_mark_charged_sets_flag() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Damaged, &evidence(&env, 3));
    client.mark_charged(&admin, &id);
    let rec = client.get_attestation(&id);
    assert!(rec.charged);
}

#[test]
fn test_resolve_changes_status_to_resolved() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 9));
    client.resolve(&admin, &id);
    let rec = client.get_attestation(&id);
    assert_eq!(rec.status, AttestationStatus::Resolved);
}

#[test]
fn test_added_librarian_can_attest() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let librarian = Address::generate(&env);
    client.add_librarian(&admin, &librarian);
    assert!(client
        .try_attest(&librarian, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 4))
        .is_ok());
}

// ===== Authorization tests =====

#[test]
fn test_non_librarian_cannot_attest() {
    let (env, contract_id, _) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_attest(
            &stranger,
            &loan_id(&env),
            &AttestationReason::Lost,
            &evidence(&env, 0)
        ),
        Err(Ok(PhysicalError::Unauthorized))
    );
}

#[test]
fn test_non_librarian_cannot_resolve() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 1));
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_resolve(&stranger, &id),
        Err(Ok(PhysicalError::Unauthorized))
    );
}

#[test]
fn test_non_admin_cannot_add_librarian() {
    let (env, contract_id, _) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    let librarian = Address::generate(&env);
    assert_eq!(
        client.try_add_librarian(&attacker, &librarian),
        Err(Ok(PhysicalError::Unauthorized))
    );
}

#[test]
fn test_removed_librarian_cannot_attest() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let librarian = Address::generate(&env);
    client.add_librarian(&admin, &librarian);
    client.remove_librarian(&admin, &librarian);
    assert_eq!(
        client.try_attest(
            &librarian,
            &loan_id(&env),
            &AttestationReason::Lost,
            &evidence(&env, 0)
        ),
        Err(Ok(PhysicalError::Unauthorized))
    );
}

// ===== Negative tests =====

#[test]
fn test_resolve_twice_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 1));
    client.resolve(&admin, &id);
    assert_eq!(
        client.try_resolve(&admin, &id),
        Err(Ok(PhysicalError::AlreadyResolved))
    );
}

#[test]
fn test_mark_charged_twice_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Damaged, &evidence(&env, 2));
    client.mark_charged(&admin, &id);
    assert_eq!(
        client.try_mark_charged(&admin, &id),
        Err(Ok(PhysicalError::AlreadyCharged))
    );
}

#[test]
fn test_mark_charged_after_resolve_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 3));
    client.resolve(&admin, &id);
    // A resolved item cannot be charged.
    assert_eq!(
        client.try_mark_charged(&admin, &id),
        Err(Ok(PhysicalError::AlreadyResolved))
    );
}

#[test]
fn test_get_attestation_not_found() {
    let (env, contract_id, _) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    assert_eq!(
        client.try_get_attestation(&missing),
        Err(Ok(PhysicalError::NotFound))
    );
}

#[test]
fn test_get_history_not_found() {
    let (env, contract_id, _) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    assert_eq!(
        client.try_get_history(&missing),
        Err(Ok(PhysicalError::NotFound))
    );
}

#[test]
fn test_attest_before_initialize_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(LibraryPhysical, ());
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let caller = Address::generate(&env);
    assert_eq!(
        client.try_attest(
            &caller,
            &loan_id(&env),
            &AttestationReason::Lost,
            &evidence(&env, 0)
        ),
        Err(Ok(PhysicalError::NotInitialized))
    );
}

// ===== Boundary tests =====

#[test]
fn test_two_attestations_same_loan_get_unique_ids() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let lid = loan_id(&env);
    let id_a = client.attest(&admin, &lid, &AttestationReason::Lost, &evidence(&env, 1));
    let id_b = client.attest(&admin, &lid, &AttestationReason::Damaged, &evidence(&env, 2));
    assert_ne!(id_a, id_b);
}

#[test]
fn test_correction_does_not_erase_prior_hashes() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let eh1 = evidence(&env, 10);
    let eh2 = evidence(&env, 20);
    let eh3 = evidence(&env, 30);
    let id = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &eh1);
    client.append_correction(&admin, &id, &eh2);
    client.append_correction(&admin, &id, &eh3);
    let history = client.get_history(&id);
    assert_eq!(history.len(), 3);
    assert_eq!(history.get(0).unwrap(), eh1);
    assert_eq!(history.get(1).unwrap(), eh2);
    assert_eq!(history.get(2).unwrap(), eh3);
}

#[test]
fn test_damaged_and_lost_reasons_both_accepted() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryPhysicalClient::new(&env, &contract_id);
    let id_lost = client.attest(&admin, &loan_id(&env), &AttestationReason::Lost, &evidence(&env, 1));
    let id_dmg = client.attest(&admin, &loan_id(&env), &AttestationReason::Damaged, &evidence(&env, 2));
    assert_eq!(client.get_attestation(&id_lost).reason, AttestationReason::Lost);
    assert_eq!(client.get_attestation(&id_dmg).reason, AttestationReason::Damaged);
}
