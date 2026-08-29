use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, BytesN, Env, String,
};

use crate::{LibraryLicensingClient, LicenseError, LicenseStatus};

/// Grants a license with an explicit `total_seats` budget.
fn grant_with_seats(
    env: &Env,
    client: &LibraryLicensingClient,
    admin: &Address,
    licensee: &Address,
    not_before: u64,
    expires_at: u64,
    total_seats: u32,
) -> BytesN<32> {
    let work_id = BytesN::from_array(env, &[9u8; 32]);
    let rights = String::from_str(env, "read");
    client.grant_license(
        admin,
        &work_id,
        licensee,
        &rights,
        &not_before,
        &expires_at,
        &total_seats,
    )
}

fn setup_license(total_seats: u32) -> (Env, Address, Address, BytesN<32>) {
    let (env, contract_id) = super::setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1500);
    let licensee = Address::generate(&env);
    let id = grant_with_seats(&env, &client, &admin, &licensee, 1000, 2000, total_seats);
    (env, contract_id, licensee, id)
}

// ===== Positive =====

#[test]
fn test_allocate_and_release_seat_round_trip() {
    let (env, contract_id, licensee, id) = setup_license(3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    assert_eq!(client.available_seats(&id), 3);
    client.allocate_seat(&licensee, &id);
    assert_eq!(client.available_seats(&id), 2);
    assert_eq!(client.license(&id).allocated_seats, 1);

    // Release restores exactly one seat.
    client.release_seat(&licensee, &id);
    assert_eq!(client.available_seats(&id), 3);
    assert_eq!(client.license(&id).allocated_seats, 0);
}

#[test]
fn test_seat_budget_stored_at_issuance() {
    let (env, contract_id, _licensee, id) = setup_license(7);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let license = client.license(&id);
    assert_eq!(license.total_seats, 7);
    assert_eq!(license.allocated_seats, 0);
}

// ===== Negative (supply limits) =====

#[test]
fn test_allocation_cannot_exceed_supply() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.allocate_seat(&licensee, &id);
    client.allocate_seat(&licensee, &id);
    // The budget is exhausted; competing calls must fail.
    let res = client.try_allocate_seat(&licensee, &id);
    assert_eq!(res, Err(Ok(LicenseError::NoSeatsAvailable)));
    assert_eq!(client.license(&id).allocated_seats, 2);
    assert_eq!(client.available_seats(&id), 0);
}

#[test]
fn test_release_with_none_allocated_rejected() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let res = client.try_release_seat(&licensee, &id);
    assert_eq!(res, Err(Ok(LicenseError::NoSeatsAllocated)));
}

#[test]
fn test_zero_seat_license_rejected() {
    let (env, contract_id) = super::setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let licensee = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[9u8; 32]);
    let rights = String::from_str(&env, "read");

    let res = client.try_grant_license(
        &admin, &work_id, &licensee, &rights, &1000u64, &2000u64, &0u32,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidSeats)));
}

// ===== Authorization =====

#[test]
fn test_allocate_seat_unauthorized() {
    let (env, contract_id, _licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let stranger = Address::generate(&env);

    let res = client.try_allocate_seat(&stranger, &id);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

#[test]
fn test_release_seat_unauthorized() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    client.allocate_seat(&licensee, &id);
    let stranger = Address::generate(&env);

    let res = client.try_release_seat(&stranger, &id);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

// ===== Boundary (lifecycle paths) =====

#[test]
fn test_allocate_before_window_rejected() {
    let (env, contract_id) = super::setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let licensee = Address::generate(&env);
    let id = grant_with_seats(&env, &client, &admin, &licensee, 1000, 2000, 2);

    let res = client.try_allocate_seat(&licensee, &id);
    assert_eq!(res, Err(Ok(LicenseError::NotYetActive)));
}

#[test]
fn test_allocate_after_expiry_rejected() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2000);

    let res = client.try_allocate_seat(&licensee, &id);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_allocate_after_revocation_rejected_but_release_allowed() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin: Address = client.license(&id).licensor;

    client.allocate_seat(&licensee, &id);
    client.revoke_license(&admin, &id);
    assert_eq!(client.license(&id).status, LicenseStatus::Revoked);

    // Allocation is rejected on a revoked license...
    let res = client.try_allocate_seat(&licensee, &id);
    assert_eq!(res, Err(Ok(LicenseError::LicenseRevoked)));

    // ...but release stays available so seats can be cleaned up.
    client.release_seat(&licensee, &id);
    assert_eq!(client.license(&id).allocated_seats, 0);
    assert_eq!(client.available_seats(&id), 2);
}

#[test]
fn test_release_after_expiry_allowed_for_cleanup() {
    let (env, contract_id, licensee, id) = setup_license(2);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    client.allocate_seat(&licensee, &id);
    env.ledger().set_timestamp(2000);
    // The license is expired, but the allocated seat can still be returned.
    client.release_seat(&licensee, &id);
    assert_eq!(client.license(&id).allocated_seats, 0);
}

#[test]
fn test_allocate_release_repeat_within_supply() {
    let (env, contract_id, licensee, id) = setup_license(1);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    // A single-seat license can be used repeatedly as long as seats are
    // returned between allocations.
    for _ in 0..3 {
        client.allocate_seat(&licensee, &id);
        assert_eq!(client.available_seats(&id), 0);
        client.release_seat(&licensee, &id);
        assert_eq!(client.available_seats(&id), 1);
    }
}

#[test]
fn test_available_seats_never_negative() {
    let (env, contract_id, licensee, id) = setup_license(3);
    let client = LibraryLicensingClient::new(&env, &contract_id);

    for expected in (0..=3).rev() {
        assert_eq!(client.available_seats(&id), expected);
        if expected > 0 {
            client.allocate_seat(&licensee, &id);
        }
    }
}
