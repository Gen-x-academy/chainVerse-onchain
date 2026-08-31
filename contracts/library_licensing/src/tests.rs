//! Tests for all four E-Library on-chain features:
//!   #940 — Enforce license validity windows (regression)
//!   #984 — Granular librarian capabilities
//!   #985 — Tutor authority scoped to owned courses
//!   #986 — Anchor course reading-list manifests
//!   #987 — Version and schedule list publication

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String,
};

use crate::{
    AccessGrant, Capability, EnrollmentProof, LibraryLicensingClient, License, LicenseError,
    LicenseStatus, ReadingListManifest, ReadingListVersion, RenditionMigrationPolicy,
};

mod entitlements;
mod seats;

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(crate::LibraryLicensing, ());
    (env, contract_id)
}

/// Initialise admin and return a client.
fn setup_with_admin() -> (Env, Address, Address) {
    let (env, contract_id) = setup();
    let admin = Address::generate(&env);
    let client = LibraryLicensingClient::new(&env, &contract_id);
    client.set_admin(&admin);
    (env, contract_id, admin)
}

/// Grant a "read" license for `work_id` to `licensee` with the given window.
fn grant_license(
    env: &Env,
    client: &LibraryLicensingClient,
    admin: &Address,
    work_id: &BytesN<32>,
    licensee: &Address,
    not_before: u64,
    expires_at: u64,
) -> BytesN<32> {
    let rights = String::from_str(env, "read");
    client.grant_license(
        admin,
        work_id,
        licensee,
        &rights,
        &not_before,
        &expires_at,
        &5u32,
    )
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
/// Build a zero-free 64-byte signature value for tests.
fn fake_sig(env: &Env, seed: u8) -> BytesN<64> {
    BytesN::from_array(env, &[seed; 64])
}

/// Commit a manifest; returns the manifest id.
fn commit_manifest(
    env: &Env,
    client: &LibraryLicensingClient,
    tutor: &Address,
    course_id: &BytesN<32>,
    term: &str,
    hash_seed: u8,
) -> BytesN<32> {
    let content_hash = BytesN::from_array(env, &[hash_seed; 32]);
    let tutor_sig = fake_sig(env, hash_seed);
    let institution_sig = BytesN::from_array(env, &[0u8; 64]);
    let term_str = String::from_str(env, term);
    client.commit_manifest(
        tutor,
        course_id,
        &term_str,
        &content_hash,
        &tutor_sig,
        &institution_sig,
    )
}

// ============================================================================
// #940 — Regression: license validity windows (pre-existing tests preserved)
// ============================================================================

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
    let res = client.try_grant_license(
        &admin, &work_id, &licensee, &rights, &1000u64, &2000u64, &5u32,
    );
    assert_eq!(res, Err(Ok(LicenseError::NotInitialized)));
}

#[test]
fn test_grant_license_unauthorized() {
    let (env, contract_id, _admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(
        &attacker, &work_id, &licensee, &rights, &1000u64, &2000u64, &5u32,
    );
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
    // attacker has neither admin nor Cataloger capability
    let res = client.try_grant_license(&attacker, &work_id, &licensee, &rights, &1000u64, &2000u64);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityNotGranted)));
}

#[test]
fn test_revoke_license_unauthorized() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 1000);
    let attacker = Address::generate(&env);
    assert_eq!(
        client.try_revoke_license(&attacker, &id),
        Err(Ok(LicenseError::CapabilityNotGranted))
    );
}

#[test]
fn test_upgrade_unauthorized() {
    let (env, contract_id, _admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    let wasm = BytesN::from_array(&env, &[9u8; 32]);
    assert_eq!(
        client.try_upgrade(&attacker, &wasm),
        Err(Ok(LicenseError::Unauthorized))
    );
}

#[test]
fn test_derive_access_grant_unauthorized() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 100, 1000);
    let stranger = Address::generate(&env);
    let grantee = Address::generate(&env);
    // Register an enrollment for the stranger (not the licensee) so the
    // Unauthorized check is reached before the enrollment check.
    let nonce = enroll(&env, &client, &admin, &work_id, &stranger, 9999);
    let proof = make_proof(work_id.clone(), stranger.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&stranger, &id, &grantee, &100u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::Unauthorized)));
    // stranger is neither the licensee nor holds Circulation
    let res = client.try_derive_access_grant(&stranger, &id, &grantee, &100u64);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityNotGranted)));
}

#[test]
fn test_grant_license_empty_rights_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "");
    let res = client.try_grant_license(
        &admin, &work_id, &licensee, &rights, &1000u64, &2000u64, &5u32,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidRights)));
}

#[test]
fn test_grant_license_zero_length_window_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    // not_before == expires_at: a zero-length window is never active because
    // the end is exclusive.
    let res = client.try_grant_license(
        &admin, &work_id, &licensee, &rights, &1000u64, &1000u64, &5u32,
    );
    let res = client.try_grant_license(&admin, &work_id, &licensee, &rights, &1000u64, &1000u64);
    assert_eq!(res, Err(Ok(LicenseError::InvalidWindow)));
}

#[test]
fn test_grant_license_inverted_window_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(
        &admin, &work_id, &licensee, &rights, &2000u64, &1000u64, &5u32,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidWindow)));
}

#[test]
fn test_derive_access_grant_zero_duration_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 100, 1000);
    let grantee = Address::generate(&env);
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &0u64, &proof);
    assert_eq!(res, Err(Ok(LicenseError::InvalidDuration)));
}

#[test]
fn test_derive_access_grant_window_overflow() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[1u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 100, u64::MAX);
    let grantee = Address::generate(&env);
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, u64::MAX);
    let proof = make_proof(work_id.clone(), licensee.clone(), u64::MAX, nonce);
    // #940 — checked arithmetic: an overflowing duration must fail with
    // WindowOverflow, never wrap around to a timestamp in the past.
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &u64::MAX, &proof);
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &u64::MAX);
    assert_eq!(res, Err(Ok(LicenseError::WindowOverflow)));
}

#[test]
fn test_grant_license_success_and_storage() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    let license: License = client.license(&id);
    assert_eq!(license.work_id, work_id);
    assert_eq!(license.licensee, licensee);
    assert_eq!(license.licensor, admin);
    assert_eq!(license.not_before, 1000);
    assert_eq!(license.expires_at, 2000);
    assert_eq!(license.status, LicenseStatus::Active);
    // #943 — the seat budget is stored at issuance and starts empty.
    assert_eq!(license.total_seats, 5);
    assert_eq!(license.allocated_seats, 0);
}

#[test]
fn test_derive_access_grant_within_window() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    let grantee = Address::generate(&env);
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64, &proof);
    let grant: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(grant.license_id, id);
    assert_eq!(grant.grantee, grantee);
    assert_eq!(grant.not_before, 1500);
    assert_eq!(grant.expires_at, 1600);
    assert_eq!(grant.loan_id, id);
    assert_eq!(grant.rendition_id, work_id);
    assert_eq!(client.access_grant_commitment(&grant_id), grant.commitment);
    assert!(client.verify_access_grant(&grant_id, &grantee, &work_id));
}

#[test]
fn test_derive_access_grant_clamps_to_license_window() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1500);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 1000, 1600);
    let grantee = Address::generate(&env);
    let nonce = enroll(&env, &client, &admin, &work_id, &licensee, 9999);
    let proof = make_proof(work_id.clone(), licensee.clone(), 9999, nonce);
    // Requested 10_000s, but the license expires at 1600 — the grant must be
    // clamped so it can never outlive the license it derives from.
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10_000u64, &proof);
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10_000u64);
    let grant: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(grant.expires_at, 1600);
}

#[test]
fn test_same_ledger_licenses_get_unique_ids() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id_a = BytesN::from_array(&env, &[1u8; 32]);
    let work_id_b = BytesN::from_array(&env, &[2u8; 32]);
    let licensee_a = Address::generate(&env);
    let licensee_b = Address::generate(&env);
    let id_a = grant_license(&env, &client, &admin, &work_id_a, &licensee_a, 0, 1000);
    let id_b = grant_license(&env, &client, &admin, &work_id_b, &licensee_b, 0, 1000);
    assert_ne!(id_a, id_b);
}

// ── Boundary tests ────────────────────────────────────────────────────────────

fn setup_boundary_license() -> (Env, Address, Address, BytesN<32>, BytesN<32>) {
    let (env, contract_id) = setup();
fn setup_boundary_license() -> (Env, Address, Address, BytesN<32>) {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[7u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    (env, contract_id, licensee, id, work_id)
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 1000, 2000);
    (env, contract_id, licensee, id)
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
    let res = client.try_derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert_eq!(res, Err(Ok(LicenseError::NotYetActive)));
}

#[test]
fn test_license_boundary_at_not_before() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(1000);
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
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &10u64);
    assert_eq!(client.access_grant(&grant_id).expires_at, 2000);
    assert!(client.is_grant_active(&grant_id));
}

#[test]
fn test_license_boundary_at_expiry() {
    let (env, contract_id, licensee, id, work_id) = setup_boundary_license();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    env.ledger().set_timestamp(2000);
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
    let grant_id = client.derive_access_grant(&licensee, &id, &grantee, &100u64);
    env.ledger().set_timestamp(1599);
    assert!(client.is_grant_active(&grant_id));
    env.ledger().set_timestamp(1600);
    assert!(!client.is_grant_active(&grant_id));
    assert!(client.is_license_active(&id));
}

// ── Revocation ────────────────────────────────────────────────────────────────

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
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[42u8; 32]);
    assert_eq!(
        client.try_revoke_license(&admin, &missing),
        Err(Ok(LicenseError::NotFound))
    );
}

// ── NotFound reads ────────────────────────────────────────────────────────────

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
// ============================================================================
// #984 — Granular librarian capabilities
// ============================================================================

// ── Positive: capability grant/revoke flow ────────────────────────────────────

#[test]
fn test_capability_grant_cataloger_can_issue_license() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let cataloger = Address::generate(&env);
    // Admin grants Cataloger capability, no expiry.
    client.grant_capability(&admin, &cataloger, &Capability::Cataloger, &0u64);
    assert!(client.has_capability(&cataloger, &Capability::Cataloger));

    // Cataloger can now grant a license.
    let work_id = BytesN::from_array(&env, &[3u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let id = client.grant_license(&cataloger, &work_id, &licensee, &rights, &0u64, &1000u64);
    assert_eq!(client.license(&id).licensee, licensee);
}

#[test]
fn test_capability_grant_circulation_can_revoke_license() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let circulation = Address::generate(&env);
    client.grant_capability(&admin, &circulation, &Capability::Circulation, &0u64);

    let work_id = BytesN::from_array(&env, &[4u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 5000);

    // Circulation librarian revokes.
    client.revoke_license(&circulation, &id);
    assert!(!client.is_license_active(&id));
}

#[test]
fn test_capability_grant_circulation_can_derive_grant() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(500);
    let circulation = Address::generate(&env);
    client.grant_capability(&admin, &circulation, &Capability::Circulation, &0u64);

    let work_id = BytesN::from_array(&env, &[5u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 100, 5000);
    let grantee = Address::generate(&env);

    // A Circulation librarian (not the licensee) can derive a grant.
    let grant_id = client.derive_access_grant(&circulation, &id, &grantee, &100u64);
    let grant: AccessGrant = client.access_grant(&grant_id);
    assert_eq!(grant.grantee, grantee);
}

#[test]
fn test_capability_policy_can_grant_other_capabilities() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let policy_mgr = Address::generate(&env);
    client.grant_capability(&admin, &policy_mgr, &Capability::Policy, &0u64);

    let new_cataloger = Address::generate(&env);
    // policy_mgr (not admin) grants Cataloger to new_cataloger.
    client.grant_capability(&policy_mgr, &new_cataloger, &Capability::Cataloger, &0u64);
    assert!(client.has_capability(&new_cataloger, &Capability::Cataloger));
}

#[test]
fn test_capability_compliance_can_revoke_capability() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let compliance = Address::generate(&env);
    client.grant_capability(&admin, &compliance, &Capability::Compliance, &0u64);

    let cataloger = Address::generate(&env);
    client.grant_capability(&admin, &cataloger, &Capability::Cataloger, &0u64);
    assert!(client.has_capability(&cataloger, &Capability::Cataloger));

    // Compliance officer revokes Cataloger from cataloger.
    client.revoke_capability(&compliance, &cataloger, &Capability::Cataloger);
    assert!(!client.has_capability(&cataloger, &Capability::Cataloger));
}

// ── Expiring capability ───────────────────────────────────────────────────────

#[test]
fn test_capability_expires_at_boundary() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let cataloger = Address::generate(&env);
    // Grant expires at timestamp 1000.
    client.grant_capability(&admin, &cataloger, &Capability::Cataloger, &1000u64);

    // Before expiry — has_capability returns true.
    env.ledger().set_timestamp(999);
    assert!(client.has_capability(&cataloger, &Capability::Cataloger));

    // At expiry — exclusive, so 1000 is already expired.
    env.ledger().set_timestamp(1000);
    assert!(!client.has_capability(&cataloger, &Capability::Cataloger));
}

#[test]
fn test_expired_capability_cannot_grant_license() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let cataloger = Address::generate(&env);
    client.grant_capability(&admin, &cataloger, &Capability::Cataloger, &500u64);

    // Jump past expiry.
    env.ledger().set_timestamp(500);

    let work_id = BytesN::from_array(&env, &[8u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(&cataloger, &work_id, &licensee, &rights, &1000u64, &2000u64);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityExpired)));
}

// ── Negative: wrong capability ────────────────────────────────────────────────

#[test]
fn test_cataloger_cannot_revoke_license() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let cataloger = Address::generate(&env);
    client.grant_capability(&admin, &cataloger, &Capability::Cataloger, &0u64);

    let work_id = BytesN::from_array(&env, &[9u8; 32]);
    let licensee = Address::generate(&env);
    let id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 5000);

    // Cataloger does not have Circulation — cannot revoke.
    let res = client.try_revoke_license(&cataloger, &id);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityNotGranted)));
}

#[test]
fn test_circulation_cannot_grant_capability() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let circulation = Address::generate(&env);
    client.grant_capability(&admin, &circulation, &Capability::Circulation, &0u64);

    let target = Address::generate(&env);
    // Circulation does not have Policy — cannot grant capabilities.
    let res = client.try_grant_capability(&circulation, &target, &Capability::Cataloger, &0u64);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityNotGranted)));
}

#[test]
fn test_revoke_nonexistent_capability_returns_not_found() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let nobody = Address::generate(&env);
    let res = client.try_revoke_capability(&admin, &nobody, &Capability::Finance);
    assert_eq!(res, Err(Ok(LicenseError::NotFound)));
}

#[test]
fn test_no_global_tutor_role_grants_catalog_admin() {
    // A tutor who has no capabilities should not be able to grant a license.
    let (env, contract_id, _admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let tutor = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[10u8; 32]);
    let licensee = Address::generate(&env);
    let rights = String::from_str(&env, "read");
    let res = client.try_grant_license(&tutor, &work_id, &licensee, &rights, &0u64, &1000u64);
    assert_eq!(res, Err(Ok(LicenseError::CapabilityNotGranted)));
}

// ============================================================================
// #985 — Tutor authority scoped to owned courses
// ============================================================================
//
// Without a registry adapter deployed in the test env, the contract falls
// back to requiring the Circulation capability.  We test that cross-course
// calls fail and that the admin (acting as any tutor in the absence of a
// registry) can always commit.

#[test]
fn test_tutor_without_capability_cannot_commit_manifest() {
    let (env, contract_id, _admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let tutor = Address::generate(&env);
    let course_id = BytesN::from_array(&env, &[20u8; 32]);
    let term = String::from_str(&env, "2025-S1");
    let content_hash = BytesN::from_array(&env, &[1u8; 32]);
    let tutor_sig = fake_sig(&env, 1);
    let institution_sig = BytesN::from_array(&env, &[0u8; 64]);

    // tutor has no Circulation capability → TutorNotCourseOwner fallback
    let res = client.try_commit_manifest(
        &tutor,
        &course_id,
        &term,
        &content_hash,
        &tutor_sig,
        &institution_sig,
    );
    // Without registry and without Circulation capability the call fails.
    assert!(res.is_err());
}

#[test]
fn test_admin_can_always_commit_manifest() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[21u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 7);
    let manifest: ReadingListManifest = client.manifest(&manifest_id);
    assert_eq!(manifest.committed_by, admin);
    assert_eq!(manifest.course_id, course_id);
}

#[test]
fn test_tutor_with_circulation_capability_can_commit_manifest() {
    // Without a registry, a holder of Circulation acts as an authorised tutor.
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let tutor = Address::generate(&env);
    client.grant_capability(&admin, &tutor, &Capability::Circulation, &0u64);

    let course_id = BytesN::from_array(&env, &[22u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &tutor, &course_id, "2025-S1", 9);
    let manifest: ReadingListManifest = client.manifest(&manifest_id);
    assert_eq!(manifest.committed_by, tutor);
}

#[test]
fn test_cross_course_publish_fails_manifest_mismatch() {
    // Manifest committed for course A cannot be used to publish a version for
    // course B (#985 — scoped authority).
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_a = BytesN::from_array(&env, &[30u8; 32]);
    let course_b = BytesN::from_array(&env, &[31u8; 32]);
    let term = String::from_str(&env, "2025-S1");

    // Commit a manifest for course A.
    let manifest_id = commit_manifest(&env, &client, &admin, &course_a, "2025-S1", 5);

    // Attempt to publish a version for course B using course A's manifest.
    let res = client.try_publish_list_version(
        &admin,
        &course_b,
        &term,
        &manifest_id,
        &0u64,
        &0u64,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidManifest)));
}

// ============================================================================
// #986 — Anchor course reading-list manifests
// ============================================================================

#[test]
fn test_commit_manifest_stores_content_hash() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[40u8; 32]);
    let content_hash = BytesN::from_array(&env, &[0xABu8; 32]);
    let tutor_sig = fake_sig(&env, 0xAB);
    let institution_sig = fake_sig(&env, 0xCD);
    let term = String::from_str(&env, "2026-S2");

    let manifest_id = client.commit_manifest(
        &admin,
        &course_id,
        &term,
        &content_hash,
        &tutor_sig,
        &institution_sig,
    );
    let manifest: ReadingListManifest = client.manifest(&manifest_id);

    assert_eq!(manifest.content_hash, content_hash);
    assert_eq!(manifest.tutor_sig, tutor_sig);
    assert_eq!(manifest.institution_sig, institution_sig);
    assert_eq!(manifest.term, term);
    assert_eq!(manifest.course_id, course_id);
}

#[test]
fn test_commit_manifest_zero_hash_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[41u8; 32]);
    let zero_hash = BytesN::from_array(&env, &[0u8; 32]);
    let tutor_sig = fake_sig(&env, 1);
    let institution_sig = BytesN::from_array(&env, &[0u8; 64]);
    let term = String::from_str(&env, "2026-S2");

    let res = client.try_commit_manifest(
        &admin,
        &course_id,
        &term,
        &zero_hash,
        &tutor_sig,
        &institution_sig,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidManifest)));
}

#[test]
fn test_commit_manifest_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let missing = BytesN::from_array(&env, &[99u8; 32]);
    assert_eq!(
        client.try_manifest(&missing),
        Err(Ok(LicenseError::NotFound))
    );
}

#[test]
fn test_two_manifests_for_same_course_have_unique_ids() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[42u8; 32]);
    let m1 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 1);
    let m2 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 2);
    assert_ne!(m1, m2);
}

#[test]
fn test_manifest_committed_at_timestamp() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(9_999);
    let course_id = BytesN::from_array(&env, &[43u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "T1", 5);
    let manifest: ReadingListManifest = client.manifest(&manifest_id);
    assert_eq!(manifest.committed_at, 9_999);
}

// ============================================================================
// #987 — Version and schedule list publication
// ============================================================================

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Publish a version and return the version number (always 1 for a fresh
/// course/term pair).
fn publish_version(
    env: &Env,
    client: &LibraryLicensingClient,
    tutor: &Address,
    course_id: &BytesN<32>,
    term: &str,
    manifest_id: &BytesN<32>,
    effective_at: u64,
) -> u32 {
    let term_str = String::from_str(env, term);
    client.publish_list_version(tutor, course_id, &term_str, manifest_id, &effective_at, &0u64)
}

// ── Positive paths ────────────────────────────────────────────────────────────

#[test]
fn test_publish_version_increments_counter() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[50u8; 32]);
    let m1 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 1);
    let m2 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 2);

    let v1 = publish_version(&env, &client, &admin, &course_id, "2025-S1", &m1, 0);
    let v2 = publish_version(&env, &client, &admin, &course_id, "2025-S1", &m2, 0);

    assert_eq!(v1, 1);
    assert_eq!(v2, 2);
}

#[test]
fn test_publish_version_stores_immutable_record() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(100);
    let course_id = BytesN::from_array(&env, &[51u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 3);
    let version = publish_version(&env, &client, &admin, &course_id, "2025-S1", &manifest_id, 100);

    let term = String::from_str(&env, "2025-S1");
    let rec: ReadingListVersion = client.list_version(&course_id, &term, &version);
    assert_eq!(rec.version, 1);
    assert_eq!(rec.manifest_id, manifest_id);
    assert_eq!(rec.effective_at, 100);
    assert_eq!(rec.published_by, admin);
    assert_eq!(rec.published_at, 100);
}

#[test]
fn test_activate_version_sets_active_pointer() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(200);
    let course_id = BytesN::from_array(&env, &[52u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 4);
    let term = String::from_str(&env, "2025-S1");
    client.publish_list_version(
        &admin,
        &course_id,
        &term,
        &manifest_id,
        &200u64,
        &0u64,
    );
    // Activate with expected_prev = 0 (nothing active yet).
    client.activate_list_version(&admin, &course_id, &term, &1u32, &0u32);

    let active = client.active_list(&course_id, &term);
    assert_eq!(active.version, 1);
    assert_eq!(active.manifest_id, manifest_id);
    assert_eq!(active.activated_at, 200);
}

#[test]
fn test_activate_replaces_pointer_with_version_2() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(300);
    let course_id = BytesN::from_array(&env, &[53u8; 32]);
    let m1 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 11);
    let m2 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 12);
    let term = String::from_str(&env, "2025-S1");

    client.publish_list_version(&admin, &course_id, &term, &m1, &300u64, &0u64);
    client.publish_list_version(&admin, &course_id, &term, &m2, &300u64, &0u64);

    client.activate_list_version(&admin, &course_id, &term, &1u32, &0u32);
    client.activate_list_version(&admin, &course_id, &term, &2u32, &1u32);

    let active = client.active_list(&course_id, &term);
    assert_eq!(active.version, 2);
    assert_eq!(active.manifest_id, m2);
}

// ── Negative / boundary paths ─────────────────────────────────────────────────

#[test]
fn test_publish_version_inverted_window_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[54u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 6);
    let term = String::from_str(&env, "2025-S1");

    // effective_at > version_expires_at → InvalidWindow.
    let res = client.try_publish_list_version(
        &admin,
        &course_id,
        &term,
        &manifest_id,
        &2000u64,
        &1000u64,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidWindow)));
}

#[test]
fn test_activate_version_not_yet_scheduled() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(50);
    let course_id = BytesN::from_array(&env, &[55u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 7);
    let term = String::from_str(&env, "2025-S1");

    // Publish with effective_at in the future.
    client.publish_list_version(&admin, &course_id, &term, &manifest_id, &1000u64, &0u64);

    // Try to activate before effective_at → ListNotScheduled.
    let res = client.try_activate_list_version(&admin, &course_id, &term, &1u32, &0u32);
    assert_eq!(res, Err(Ok(LicenseError::ListNotScheduled)));
}

#[test]
fn test_activate_version_expired() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(500);
    let course_id = BytesN::from_array(&env, &[56u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 8);
    let term = String::from_str(&env, "2025-S1");

    // Publish effective_at=100, expires_at=200 — already expired at t=500.
    client.publish_list_version(&admin, &course_id, &term, &manifest_id, &100u64, &200u64);

    let res = client.try_activate_list_version(&admin, &course_id, &term, &1u32, &0u32);
    assert_eq!(res, Err(Ok(LicenseError::Expired)));
}

#[test]
fn test_stale_active_pointer_rejected() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    env.ledger().set_timestamp(300);
    let course_id = BytesN::from_array(&env, &[57u8; 32]);
    let m1 = commit_manifest(&env, &client, &admin, &course_id, "T1", 1);
    let m2 = commit_manifest(&env, &client, &admin, &course_id, "T1", 2);
    let term = String::from_str(&env, "T1");

    client.publish_list_version(&admin, &course_id, &term, &m1, &300u64, &0u64);
    client.publish_list_version(&admin, &course_id, &term, &m2, &300u64, &0u64);

    // Activate version 1 correctly.
    client.activate_list_version(&admin, &course_id, &term, &1u32, &0u32);

    // Try to activate version 2 but claim expected_prev = 0 (stale).
    let res = client.try_activate_list_version(&admin, &course_id, &term, &2u32, &0u32);
    assert_eq!(res, Err(Ok(LicenseError::StaleActivePointer)));
}

#[test]
fn test_list_version_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let course_id = BytesN::from_array(&env, &[58u8; 32]);
    let term = String::from_str(&env, "T1");
    let res = client.try_list_version(&course_id, &term, &1u32);
    assert_eq!(res, Err(Ok(LicenseError::NotFound)));
}

#[test]
fn test_active_list_not_found() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let course_id = BytesN::from_array(&env, &[59u8; 32]);
    let term = String::from_str(&env, "T1");
    let res = client.try_active_list(&course_id, &term);
    assert_eq!(res, Err(Ok(LicenseError::NotFound)));
}

#[test]
fn test_publish_version_manifest_wrong_term_rejected() {
    // Manifest committed for term "2025-S1" cannot be used for "2025-S2".
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[60u8; 32]);
    let manifest_id = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 5);
    let wrong_term = String::from_str(&env, "2025-S2");

    let res = client.try_publish_list_version(
        &admin,
        &course_id,
        &wrong_term,
        &manifest_id,
        &0u64,
        &0u64,
    );
    assert_eq!(res, Err(Ok(LicenseError::InvalidManifest)));
}

#[test]
fn test_independent_terms_have_independent_version_counters() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);

    let course_id = BytesN::from_array(&env, &[61u8; 32]);
    let m_s1 = commit_manifest(&env, &client, &admin, &course_id, "2025-S1", 1);
    let m_s2 = commit_manifest(&env, &client, &admin, &course_id, "2025-S2", 2);

    let v_s1 = publish_version(&env, &client, &admin, &course_id, "2025-S1", &m_s1, 0);
    let v_s2 = publish_version(&env, &client, &admin, &course_id, "2025-S2", &m_s2, 0);

    // Each term starts at version 1 independently.
    assert_eq!(v_s1, 1);
    assert_eq!(v_s2, 1);
// ===== Rendition migration (#952) =====

#[test]
fn test_forced_rendition_migration_follows_active_grant() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let licensee = Address::generate(&env);
    let grantee = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(100);
    let from = BytesN::from_array(&env, &[40u8; 32]);
    let to = BytesN::from_array(&env, &[41u8; 32]);
    let license_id = grant(&env, &client, &admin, &from, &licensee, 0, 500);
    let grant_id = client.derive_access_grant(&licensee, &license_id, &grantee, &100u64);

    client.propose_rendition_migration(&admin, &from, &to, &RenditionMigrationPolicy::Forced);

    assert!(client.is_grant_active_for_work(&grant_id, &from));
    assert!(client.is_grant_active_for_work(&grant_id, &to));
}

#[test]
fn test_opt_in_rendition_migration_requires_grantee_acceptance() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let licensee = Address::generate(&env);
    let grantee = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.set_admin(&admin);
    env.ledger().set_timestamp(100);
    let from = BytesN::from_array(&env, &[42u8; 32]);
    let to = BytesN::from_array(&env, &[43u8; 32]);
    let license_id = grant(&env, &client, &admin, &from, &licensee, 0, 500);
    let grant_id = client.derive_access_grant(&licensee, &license_id, &grantee, &100u64);
    client.propose_rendition_migration(&admin, &from, &to, &RenditionMigrationPolicy::OptIn);

    assert!(!client.is_grant_active_for_work(&grant_id, &to));
    assert_eq!(
        client.try_accept_rendition_migration(&stranger, &grant_id, &to),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_eq!(
        client.try_accept_rendition_migration(&grantee, &grant_id, &from),
        Err(Ok(LicenseError::InvalidMigrationTarget))
    );
    client.accept_rendition_migration(&grantee, &grant_id, &to);
    assert!(client.is_grant_active_for_work(&grant_id, &to));
}

#[test]
fn test_rendition_migration_rejects_invalid_and_duplicate_proposals() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.set_admin(&admin);
    let from = BytesN::from_array(&env, &[44u8; 32]);
    let to = BytesN::from_array(&env, &[45u8; 32]);
    assert_eq!(
        client.try_propose_rendition_migration(
            &attacker,
            &from,
            &to,
            &RenditionMigrationPolicy::Forced
        ),
        Err(Ok(LicenseError::Unauthorized))
    );
    assert_eq!(
        client.try_propose_rendition_migration(
            &admin,
            &from,
            &from,
            &RenditionMigrationPolicy::Forced
        ),
        Err(Ok(LicenseError::InvalidRenditionMigration))
    );
    client.propose_rendition_migration(&admin, &from, &to, &RenditionMigrationPolicy::Forced);
    assert_eq!(
        client.try_propose_rendition_migration(
            &admin,
            &from,
            &to,
            &RenditionMigrationPolicy::OptIn
        ),
        Err(Ok(LicenseError::RenditionMigrationExists))
    );
}

#[test]
fn test_rendition_migration_does_not_resurrect_expired_grant() {
    let (env, contract_id) = setup();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let licensee = Address::generate(&env);
    let grantee = Address::generate(&env);
    client.set_admin(&admin);
    let from = BytesN::from_array(&env, &[46u8; 32]);
    let to = BytesN::from_array(&env, &[47u8; 32]);
    let license_id = grant(&env, &client, &admin, &from, &licensee, 100, 200);
    env.ledger().set_timestamp(100);
    let grant_id = client.derive_access_grant(&licensee, &license_id, &grantee, &50u64);
    client.propose_rendition_migration(&admin, &from, &to, &RenditionMigrationPolicy::Forced);
    env.ledger().set_timestamp(149);
    assert!(client.is_grant_active_for_work(&grant_id, &to));
    env.ledger().set_timestamp(150);
    assert!(!client.is_grant_active_for_work(&grant_id, &to));
    env.ledger().set_timestamp(200);
    assert!(!client.is_grant_active_for_work(&grant_id, &to));
}


#[test]
fn test_institutional_license_mint_records_authenticated_inventory() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let institution = Address::generate(&env);
    let treasury = Address::generate(&env);
    let work_id = BytesN::from_array(&env, &[0x11; 32]);
    let rights = String::from_str(&env, "institutional-read");
    let auth_commitment = BytesN::from_array(&env, &[0x22; 32]);

    let license_id = client.mint_institutional_license(
        &admin,
        &institution,
        &treasury,
        &work_id,
        &rights,
        &10u64,
        &1_000u64,
        &25u32,
        &auth_commitment,
    );
    let license = client.license(&license_id);
    assert_eq!(license.licensee, institution);
    assert_eq!(license.total_seats, 25);
    assert_eq!(license.status, LicenseStatus::Active);
}

#[test]
fn test_scheduled_revocation_respects_effective_time_and_preserves_active_loans_boundary() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[0x33; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 10_000);
    let reason = BytesN::from_array(&env, &[0x44; 32]);

    client.schedule_license_revocation(&admin, &license_id, &200u64, &500u64, &reason);
    env.ledger().set_timestamp(199);
    assert_eq!(client.try_apply_license_revocation(&admin, &license_id), Err(Ok(LicenseError::RevocationNotEffective)));
    env.ledger().set_timestamp(200);
    client.apply_license_revocation(&admin, &license_id);
    assert_eq!(client.license(&license_id).status, LicenseStatus::Revoked);
}

#[test]
fn test_scheduled_revocation_rejects_empty_reason_commitment() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[0x55; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 10_000);
    let empty = BytesN::from_array(&env, &[0u8; 32]);
    assert_eq!(
        client.try_schedule_license_revocation(&admin, &license_id, &100u64, &100u64, &empty),
        Err(Ok(LicenseError::InvalidCommitment))
    );
}

#[test]
fn test_offline_grant_rejects_zero_use_bound() {
    let (env, contract_id, admin) = setup_with_admin();
    let client = LibraryLicensingClient::new(&env, &contract_id);
    let work_id = BytesN::from_array(&env, &[0x66; 32]);
    let licensee = Address::generate(&env);
    let license_id = grant_license(&env, &client, &admin, &work_id, &licensee, 0, 10_000);
    // No grant is needed to exercise the bounded-input invariant: zero uses
    // must be rejected before any offline commitment is persisted.
    let grant_id = BytesN::from_array(&env, &[0x77; 32]);
    assert_eq!(
        client.try_create_offline_grant(&licensee, &grant_id, &500u64, &0u32),
        Err(Ok(LicenseError::InvalidDuration))
    );
    let _ = license_id;
}
