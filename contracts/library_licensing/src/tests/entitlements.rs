use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, Symbol,
};

use crate::{AccessMode, Entitlement, LibraryLicensingClient, LicenseError, LicenseStatus};

use super::grant;

fn setup() -> (Env, Address, Address, BytesN<32>) {
    let (env, contract_id) = super::setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    (env, contract_id, admin, id)
}

fn epub(env: &Env) -> Symbol {
    Symbol::new(env, "EPUB")
}

fn pdf(env: &Env) -> Symbol {
    Symbol::new(env, "PDF")
}

fn audio(env: &Env) -> Symbol {
    Symbol::new(env, "AUDIO")
}

// ===== Positive =====

#[test]
fn test_grant_entitlement_and_is_entitled_exact_pair() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    assert!(client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
    let ent: Entitlement = client.entitlement(&id, &epub(&env), &AccessMode::Borrow);
    assert_eq!(ent.license_id, id);
    assert_eq!(ent.rendition_id, epub(&env));
    assert_eq!(ent.access_mode, AccessMode::Borrow);
    assert_eq!(client.entitlements_len(&id), 1);
}

#[test]
fn test_borrowing_one_format_does_not_unlock_another() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    // The PDF format was never granted, so it must not be usable -- even
    // though the license itself is fully active.
    assert!(!client.is_entitled(&id, &pdf(&env), &AccessMode::Borrow));
    assert!(!client.is_entitled(&id, &audio(&env), &AccessMode::Borrow));
    assert!(!client.is_entitled(&id, &epub(&env), &AccessMode::AccessibleAlternative));
}

#[test]
fn test_accessible_alternative_granted_intentionally() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    // The accessible alternative is granted on purpose, as its own
    // (rendition, mode) pair...
    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::AccessibleAlternative);
    assert!(client.is_entitled(&id, &epub(&env), &AccessMode::AccessibleAlternative));
    // ...and the primary Borrow entitlement is untouched: granting the
    // alternative never unlocks the primary mode and vice versa.
    assert!(client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
    assert_eq!(client.entitlements_len(&id), 2);
}

#[test]
fn test_granting_same_pair_twice_is_idempotent() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    // No duplicate entry is created.
    assert_eq!(client.entitlements_len(&id), 1);
    assert!(client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
}

#[test]
fn test_multiple_renditions_are_queryable_within_bounds() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    client.grant_entitlement(&admin, &id, &pdf(&env), &AccessMode::Borrow);
    client.grant_entitlement(
        &admin,
        &id,
        &audio(&env),
        &AccessMode::AccessibleAlternative,
    );

    assert_eq!(client.entitlements_len(&id), 3);
    let all = client.entitlements(&id, &0u32, &10u32);
    assert_eq!(all.len(), 3);

    // Pagination is bounded: out-of-range windows clamp instead of panicking.
    let page = client.entitlements(&id, &1u32, &1u32);
    assert_eq!(page.len(), 1);
    assert_eq!(client.entitlements(&id, &3u32, &10u32).len(), 0);
    assert_eq!(client.entitlements(&id, &0u32, &0u32).len(), 0);
}

#[test]
fn test_revoke_entitlement() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    client.grant_entitlement(&admin, &id, &pdf(&env), &AccessMode::Borrow);

    client.revoke_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    assert!(!client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
    assert_eq!(
        client.try_entitlement(&id, &epub(&env), &AccessMode::Borrow),
        Err(Ok(LicenseError::EntitlementNotFound))
    );
    // The other rendition is untouched.
    assert!(client.is_entitled(&id, &pdf(&env), &AccessMode::Borrow));
    assert_eq!(client.entitlements_len(&id), 1);
}

// ===== Negative =====

#[test]
fn test_grant_entitlement_license_not_found() {
    let (env, contract_id, admin, _id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[42u8; 32]);

    let res = client.try_grant_entitlement(&admin, &missing, &epub(&env), &AccessMode::Borrow);
    assert_eq!(res, Err(Ok(LicenseError::NotFound)));
}

#[test]
fn test_revoke_entitlement_not_found() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let res = client.try_revoke_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    assert_eq!(res, Err(Ok(LicenseError::EntitlementNotFound)));
}

#[test]
fn test_entitlement_not_found_read() {
    let (env, contract_id, _admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    assert_eq!(
        client.try_entitlement(&id, &epub(&env), &AccessMode::Borrow),
        Err(Ok(LicenseError::EntitlementNotFound))
    );
}

#[test]
fn test_is_entitled_license_not_found() {
    let (env, contract_id, _admin, _id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[43u8; 32]);

    assert_eq!(
        client.try_is_entitled(&missing, &epub(&env), &AccessMode::Borrow),
        Err(Ok(LicenseError::NotFound))
    );
}

// ===== Authorization =====

#[test]
fn test_grant_entitlement_unauthorized() {
    let (env, contract_id, _admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);

    let res = client.try_grant_entitlement(&attacker, &id, &epub(&env), &AccessMode::Borrow);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

#[test]
fn test_revoke_entitlement_unauthorized() {
    let (env, contract_id, _admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);

    let res = client.try_revoke_entitlement(&attacker, &id, &epub(&env), &AccessMode::Borrow);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

// ===== Boundary =====

#[test]
fn test_is_entitled_false_outside_license_window() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    // Inside the window: entitled.
    env.ledger().set_timestamp(1500);
    assert!(client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));

    // After expiry: the entitlement is inert.
    env.ledger().set_timestamp(2000);
    assert!(!client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
}

#[test]
fn test_is_entitled_false_after_revocation() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);
    client.revoke_license(&admin, &id);

    assert_eq!(client.license(&id).status, LicenseStatus::Revoked);
    assert!(!client.is_entitled(&id, &epub(&env), &AccessMode::Borrow));
}

#[test]
fn test_entitlements_query_clamps_to_stored_length() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    // Querying past the end yields an empty slice, never a panic.
    assert_eq!(client.entitlements(&id, &5u32, &10u32).len(), 0);
    assert_eq!(client.entitlements_len(&id), 1);
}

#[test]
fn test_entitlement_granted_at_recorded() {
    let (env, contract_id, admin, id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1600);

    client.grant_entitlement(&admin, &id, &epub(&env), &AccessMode::Borrow);

    let ent: Entitlement = client.entitlement(&id, &epub(&env), &AccessMode::Borrow);
    assert_eq!(ent.granted_at, 1600);
}
