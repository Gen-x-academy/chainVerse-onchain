use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String,
};

use crate::{AccessGrant, LibraryLicensingClient, License, LicenseError, LicenseStatus};

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(crate::LibraryLicensing, ());
    (env, contract_id)
}

/// Grant a "read" license for `work_id` to `licensee` with the given window.
fn grant(
    env: &Env,
    client: &LibraryLicensingClient,
    admin: &Address,
    work_id: &BytesN<32>,
    licensee: &Address,
    not_before: u64,
    expires_at: u64,
) -> BytesN<32> {
    let rights = String::from_str(env, "read");
    client.grant_license(admin, work_id, licensee, &rights, &not_before, &expires_at)
}

// ===== Authorization =====

#[test]
fn test_set_admin_twice_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let other = Address::generate(&env);
    assert_eq!(
        client.try_set_admin(&other),
        Err(Ok(LicenseError::AlreadyInitialized))
    );
}

#[test]
fn test_grant_license_not_initialized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(&admin, &work_id, &licensee, &rights, &1000u64, &2000u64);
    assert_eq!(res, Err(Ok(LicenseError::NotInitialized)));
}

#[test]
fn test_grant_license_unauthorized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let attacker = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(&attacker, &work_id, &licensee, &rights, &1000u64, &2000u64);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

#[test]
fn test_revoke_license_unauthorized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 0, 1000);
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_revoke_license(&attacker, &id),
        Err(Ok(LicenseError::Unauthorized))
    );
}

#[test]
fn test_upgrade_unauthorized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let attacker = Address::generate(&env);
    let wasm = BytesN::from_array(&env, &[9u8; 32]);
    assert_eq!(
        client.try_upgrade(&attacker, &wasm),
        Err(Ok(LicenseError::Unauthorized))
    );
}

#[test]
fn test_derive_access_grant_unauthorized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 100, 1000);
    let stranger = Address::generate(&env);
    let grantee = Address::generate(&env);
    let res = client.try_derive_access_grant(&stranger, &id, &grantee, &100u64);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

// ===== Negative validation =====

#[test]
fn test_grant_license_empty_rights_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "");
    let res = client.try_grant_license(&admin, &work_id, &licensee, &rights, &1000u64, &2000u64);
    assert_eq!(res, Err(Ok(LicenseError::InvalidRights)));
}

#[test]
fn test_grant_license_zero_length_window_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    // not_before == expires_at: a zero-length window is never active because
    // the end is exclusive.
    let res = client.try_grant_license(&admin, &work_id, &licensee, &rights, &1000u64, &1000u64);
    assert_eq!(res, Err(Ok(LicenseError::InvalidWindow)));
}

#[test]
fn test_grant_license_inverted_window_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(&admin, &work_id, &licensee, &rights, &2000u64, &1000u64);
    assert_eq!(res, Err(Ok(LicenseError::InvalidWindow)));
}

#[test]
fn test_derive_access_grant_zero_duration_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 100, 1000);
    let grantee = Address::generate(&env);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &0u64);
    assert_eq!(res, Err(Ok(LicenseError::InvalidDuration)));
}

#[test]
fn test_derive_access_grant_window_overflow() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    // Non-zero "now" so now + duration can actually overflow u64.
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 100, u64::MAX);
    let grantee = Address::generate(&env);
    // #940 — checked arithmetic: an overflowing duration must fail with
    // WindowOverflow, never wrap around to a timestamp in the past.
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &u64::MAX);
    assert_eq!(res, Err(Ok(LicenseError::WindowOverflow)));
}

// ===== Positive paths =====

#[test]
fn test_grant_license_success_and_storage() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    let license: License = client.license(&id);
    assert_eq!(license.work_id, work_id);
    assert_eq!(license.licensee, licensee);
    assert_eq!(license.licensor, admin);
    assert_eq!(license.not_before, 1000);
    assert_eq!(license.expires_at, 2000);
    assert_eq!(license.status, LicenseStatus::Active);
}

#[test]
fn test_derive_access_grant_within_window() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    let grantee = Address::generate(&env);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64);
    let grant: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(grant.license_id, id);
    assert_eq!(grant.grantee, grantee);
    assert_eq!(grant.not_before, 1500);
    assert_eq!(grant.expires_at, 1600);
}

#[test]
fn test_derive_access_grant_clamps_to_license_window() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 1600);
    let grantee = Address::generate(&env);
    // Requested 10_000s, but the license expires at 1600 — the grant must be
    // clamped so it can never outlive the license it derives from.
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10_000u64);
    let grant: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(grant.expires_at, 1600);
}

#[test]
fn test_same_ledger_licenses_get_unique_ids() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    // Ledger timestamp is fixed in the test env; only the monotonic nonce
    // separates the two license ids.
    let work_id_a = BytesN::from_array(&env, &[1u8; 32]);
    let work_id_b = BytesN::from_array(&env, &[2u8; 32]);
    let licensee_a = Address::generate(&env);
    let licensee_b = Address::generate(&env);
    let id_a = grant(&env, &client, &admin, &work_id_a, &licensee_a, 0, 1000);
    let id_b = grant(&env, &client, &admin, &work_id_b, &licensee_b, 0, 1000);
    assert_ne!(id_a, id_b);
}

// ===== Boundary tests (#940): before, at, and after each timestamp =====
//
// Window semantics: `not_before` is INCLUSIVE (active at `not_before`),
// `expires_at` is EXCLUSIVE (inactive at `expires_at`).

fn setup_boundary_license() -> (Env, Address, Address, BytesN<32>) {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    (env, contract_id, licensee, id)
}

#[test]
fn test_license_boundary_before_not_before() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(999);
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    // Deriving a grant before the window opens is rejected with NotYetActive.
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert_eq!(res, Err(Ok(LicenseError::NotYetActive)));
}

#[test]
fn test_license_boundary_at_not_before() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1000);
    // `not_before` is inclusive: the license is active at exactly 1000.
    assert!(client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert!(client.is_grant_active(&grant_id));
}

#[test]
fn test_license_boundary_just_before_expiry() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1999);
    assert!(client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10u64);
    // Clamped to the license expiry at 2000.
    assert_eq!(client.access_grant(&grant_id).expires_at, 2000);
    assert!(client.is_grant_active(&grant_id));
}

#[test]
fn test_license_boundary_at_expiry() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2000);
    // `expires_at` is exclusive: the license expires at exactly 2000.
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_license_boundary_after_expiry() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2001);
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_grant_boundary_at_grant_expiry() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let grantee = Address::generate(&env);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64);
    // Grant window: [1500, 1600).
    env.ledger().set_timestamp(1599);
    assert!(client.is_grant_active(&grant_id));
    env.ledger().set_timestamp(1600);
    assert!(!client.is_grant_active(&grant_id));
    // The parent license is still valid at 1600, so only the grant expired.
    assert!(client.is_license_active(&id));
}

// ===== Revocation =====

#[test]
fn test_revoke_license_kills_derived_grants() {
    let (env, contract_id, licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let grantee = Address::generate(&env);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64);
    assert!(client.is_grant_active(&grant_id));
    let admin: Address = client.license(&id).licensor;
    client.revoke_license(&admin, &id);
    assert!(!client.is_license_active(&id));
    // Existing grants are invalidated immediately — the parent license is
    // re-checked on every read.
    assert!(!client.is_grant_active(&grant_id));
}

#[test]
fn test_revoke_license_twice_rejected() {
    let (env, contract_id, _licensee, id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin: Address = client.license(&id).licensor;
    client.revoke_license(&admin, &id);
    assert_eq!(
        client.try_revoke_license(&admin, &id),
        Err(Ok(LicenseError::LicenseRevoked))
    );
}

#[test]
fn test_revoke_license_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let missing = BytesN::from_array(&env, &[42u8; 32]);
    assert_eq!(
        client.try_revoke_license(&admin, &missing),
        Err(Ok(LicenseError::NotFound))
    );
}

// ===== NotFound reads =====

#[test]
fn test_is_license_active_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[42u8; 32]);
    assert_eq!(
        client.try_is_license_active(&missing),
        Err(Ok(LicenseError::NotFound))
    );
    assert_eq!(
        client.try_license(&missing),
        Err(Ok(LicenseError::NotFound))
    );
}

#[test]
fn test_is_grant_active_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[43u8; 32]);
    assert_eq!(
        client.try_is_grant_active(&missing),
        Err(Ok(LicenseError::NotFound))
    );
    assert_eq!(
        client.try_access_grant(&missing),
        Err(Ok(LicenseError::NotFound))
    );
}
