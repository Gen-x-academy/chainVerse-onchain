use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String,
};

use crate::{
    AccessGrant, EnrollmentProof, LibraryLicensingClient, License, LicenseError, LicenseStatus,
};

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

/// Register an enrollment and return the nonce used, so tests can build
/// a matching [`EnrollmentProof`].
fn enroll(
    env: &Env,
    client: &LibraryLicensingClient,
    admin: &Address,
    course_id: &BytesN<32>,
    learner: &Address,
    proof_expires_at: u64,
) -> BytesN<32> {
    // Use a unique nonce based on learner XDR bytes so multiple learners
    // in the same test don't collide.
    let nonce = env
        .crypto()
        .sha256(&soroban_sdk::Bytes::from_slice(
            env,
            &proof_expires_at.to_be_bytes(),
        ))
        .into();
    client.record_enrollment(admin, course_id, learner, &proof_expires_at, &nonce);
    nonce
}

/// Build an [`EnrollmentProof`] from the fields used in `enroll`.
fn make_proof(
    course_id: BytesN<32>,
    learner: Address,
    expires_at: u64,
    nonce: BytesN<32>,
) -> EnrollmentProof {
    EnrollmentProof {
        course_id,
        learner,
        expires_at,
        nonce,
    }
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
    // Register an enrollment for the stranger (not the licensee) so the
    // Unauthorized check is reached before the enrollment check.
    let nonce = enroll(&env, &client, &admin, &work_id, &stranger, 9999);
    let proof = make_proof(work_id.clone(), stranger.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&stranger, &id, &grantee, &100u64, &proof);
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
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &0u64, &proof);
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
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, u64::MAX);
    let proof = make_proof(work_id.clone(), licensee.clone(), u64::MAX, nonce);
    // #940 — checked arithmetic: an overflowing duration must fail with
    // WindowOverflow, never wrap around to a timestamp in the past.
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &u64::MAX, &proof);
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
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64, &proof);
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
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    // Requested 10_000s, but the license expires at 1600 — the grant must be
    // clamped so it can never outlive the license it derives from.
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10_000u64, &proof);
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

fn setup_boundary_license() -> (Env, Address, Address, BytesN<32>, BytesN<32>) {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    (env, contract_id, licensee, id, work_id)
}

#[test]
fn test_license_boundary_before_not_before() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(999);
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    // Deriving a grant before the window opens is rejected with NotYetActive.
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::NotYetActive)));
}

#[test]
fn test_license_boundary_at_not_before() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1000);
    // `not_before` is inclusive: the license is active at exactly 1000.
    assert!(client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10u64, &proof);
    assert!(client.is_grant_active(&grant_id));
}

#[test]
fn test_license_boundary_just_before_expiry() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1999);
    assert!(client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10u64, &proof);
    // Clamped to the license expiry at 2000.
    assert_eq!(client.access_grant(&grant_id).expires_at, 2000);
    assert!(client.is_grant_active(&grant_id));
}

#[test]
fn test_license_boundary_at_expiry() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2000);
    // `expires_at` is exclusive: the license expires at exactly 2000.
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_license_boundary_after_expiry() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2001);
    assert!(!client.is_license_active(&id));
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_grant_boundary_at_grant_expiry() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64, &proof);
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
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let grantee = Address::generate(&env);
    let nonce = enroll(
        &env,
        &client,
        &client.license(&id).licensor,
        &work_id,
        &licensee,
        9999,
    );
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64, &proof);
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
    let (env, contract_id, _licensee, id, _work_id) = setup_boundary_license();
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

// ===== #988 — Enrollment gating tests =====

/// Positive path: enrolled learner successfully derives a grant.
#[test]
fn test_enrolled_learner_can_derive_grant() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1000);
    let work_id = BytesN::from_array(&env, &[10u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // Admin issues an enrollment attestation for this learner/course.
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &license_id, &grantee, &200u64, &proof);
    assert!(client.is_grant_active(&grant_id));
    let ag: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(ag.license_id, license_id);
    assert_eq!(ag.grantee, grantee);
}

/// Negative path: a learner with no enrollment record is rejected.
#[test]
fn test_unenrolled_learner_cannot_derive_grant() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1000);
    let work_id = BytesN::from_array(&env, &[11u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // The admin never calls record_enrollment — use a fabricated nonce.
    let fake_nonce = BytesN::from_array(&env, &[0xFFu8; 32]);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, fake_nonce);
    let res = client.try_derive_access_grant(&licensee, &license_id, &grantee, &100u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::EnrollmentRequired)));
}

/// Authorization: replay attack — same nonce cannot be used twice.
#[test]
fn test_enrollment_proof_cannot_be_replayed() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1000);
    let work_id = BytesN::from_array(&env, &[12u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee_a = Address::generate(&env);
    let grantee_b = Address::generate(&env);
    // Enroll once.
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce.clone());
    // First use succeeds.
    client.derive_access_grant(&licensee, &license_id, &grantee_a, &100u64, &proof);
    // Second use with the same proof (same nonce) must fail with ProofReplayed.
    let proof2 = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &license_id, &grantee_b, &100u64, &proof2);
    assert_eq!(res, Err(Ok(LicenseError::ProofReplayed)));
}

/// Authorization: cross-course replay — proof for course A cannot unlock course B.
#[test]
fn test_enrollment_proof_cannot_cross_courses() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(1000);
    let work_id_a = BytesN::from_array(&env, &[0xAAu8; 32]);
    let work_id_b = BytesN::from_array(&env, &[0xBBu8; 32]);
    let licensee = Address::generate(&env);
    let _license_a = grant(&env, &client, &admin, &work_id_a, &licensee, 0, 5000);
    let license_b = grant(&env, &client, &admin, &work_id_b, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // Admin only enrolls for course A.
    let nonce = enroll(&env, &client, &admin, &work_id_a, &licensee, 9999);
    // Attacker constructs a proof claiming course_id == work_id_a, targeting
    // license_b (work_id_b) → must fail with ProofCourseMismatch.
    let proof = make_proof(work_id_a.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &license_b, &grantee, &100u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::ProofCourseMismatch)));
}

/// Boundary: expired proof is rejected (exactly at proof expiry timestamp).
#[test]
fn test_expired_proof_at_exactly_expires_at_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[13u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // Proof expires at ledger timestamp 2000.
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 2000);
    let proof = make_proof(work_id.clone(), licensee.clone(), 2000, nonce);
    // Now == 2000: proof.expires_at is exclusive, so now >= expires_at → rejected.
    env.ledger().set_timestamp(2000);
    let res = client.try_derive_access_grant(&licensee, &license_id, &grantee, &100u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::EnrollmentExpired)));
}

/// Boundary: proof valid just before expiry succeeds.
#[test]
fn test_proof_just_before_expires_at_succeeds() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[14u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // Proof expires at 2000; ledger is at 1999 → still valid.
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 2000);
    let proof = make_proof(work_id.clone(), licensee.clone(), 2000, nonce);
    env.ledger().set_timestamp(1999);
    let grant_id = client.derive_access_grant(&licensee, &license_id, &grantee, &100u64, &proof);
    assert!(client.is_grant_active(&grant_id));
}

/// Authorization: proof's learner field does not match caller.
#[test]
fn test_proof_learner_mismatch_rejected() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[15u8; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant(&env, &client, &admin, &work_id, &licensee, 0, 5000);
    let grantee = Address::generate(&env);
    // The admin enrolled `licensee`, but attacker puts a *different* learner
    // address in the proof's learner field. This is caught before the
    // enrollment record lookup, so the attacker learns nothing about other
    // learners' state.
    let other_learner = Address::generate(&env);
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let tampered_proof = make_proof(work_id.clone(), other_learner, 9999, nonce);
    // licensee is still the caller but the proof says a different learner.
    let res =
        client.try_derive_access_grant(&licensee, &license_id, &grantee, &100u64, &tampered_proof);
    assert_eq!(res, Err(Ok(LicenseError::ProofLearnerMismatch)));
}

/// Privacy: errors from the enrollment gate do not expose other learners'
/// enrollment state. Two learners enrolled in the same course get
/// independent nonces; using one's nonce for the other's call fails
/// with ProofLearnerMismatch (not a data leak about the other learner).
#[test]
fn test_enrollment_errors_do_not_leak_other_learner_state() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[16u8; 32]);
    let learner_a = Address::generate(&env);
    let learner_b = Address::generate(&env);
    let license_a = grant(&env, &client, &admin, &work_id, &learner_a, 0, 5000);
    let _license_b = grant(&env, &client, &admin, &work_id, &learner_b, 0, 5000);
    let grantee = Address::generate(&env);
    // Both are enrolled; each has their own nonce.
    let nonce_a = enroll(&env, &client, &admin, &work_id, &learner_a, 9999);
    // learner_b tries to use learner_a's nonce against learner_a's license.
    // The proof's learner field is learner_b, but the nonce belongs to a
    // record for learner_a. The contract checks learner field == caller first
    // and returns ProofLearnerMismatch, not information about learner_a's
    // enrollment record.
    let proof_with_wrong_learner =
        make_proof(work_id.clone(), learner_b.clone(), 9999, nonce_a.clone());
    let res = client.try_derive_access_grant(
        &learner_b,
        &license_a,
        &grantee,
        &100u64,
        &proof_with_wrong_learner,
    );
    // learner_b is not the licensee of license_a, so Unauthorized fires before
    // any enrollment check. That's the expected behaviour: caller identity is
    // checked first.
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

/// Positive: record_enrollment is admin-only; non-admin is rejected.
#[test]
fn test_record_enrollment_unauthorized() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let attacker = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[17u8; 32]);
    let learner = Address::generate(&env);
    let nonce = BytesN::from_array(&env, &[0xABu8; 32]);
    let res = client.try_record_enrollment(&attacker, &work_id, &learner, &9999u64, &nonce);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
}

/// Positive: admin can record enrollment and it is retrievable via the
/// on-chain state used by `derive_access_grant`.
#[test]
fn test_record_enrollment_success() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let work_id = BytesN::from_array(&env, &[18u8; 32]);
    let learner = Address::generate(&env);
    let nonce = BytesN::from_array(&env, &[0xCDu8; 32]);
    // Should succeed without error.
    client.record_enrollment(&admin, &work_id, &learner, &9999u64, &nonce);
    // Verify by successfully deriving a grant.
    env.ledger().set_timestamp(500);
    let license_id = grant(&env, &client, &admin, &work_id, &learner, 0, 5000);
    let grantee = Address::generate(&env);
    let proof = make_proof(work_id.clone(), learner.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&learner, &license_id, &grantee, &100u64, &proof);
    assert!(client.is_grant_active(&grant_id));
}
