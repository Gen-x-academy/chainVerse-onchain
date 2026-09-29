extern crate std;

use soroban_sdk::{testutils::Address as _, vec, Address, BytesN, Env, Symbol};

use crate::{
    ClaimSource, ContractError, ScholarshipIdentityContract, ScholarshipIdentityContractClient,
};

fn setup() -> (Env, Address, ScholarshipIdentityContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipIdentityContract, ());
    let client = ScholarshipIdentityContractClient::new(&env, &id);
    (env, id, client)
}

fn provider_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xABu8; 32])
}

fn scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

fn identity_type(env: &Env) -> Symbol {
    Symbol::new(env, "identity")
}

fn enrollment_type(env: &Env) -> Symbol {
    Symbol::new(env, "enrollment")
}

fn future_expiry(env: &Env) -> u64 {
    env.ledger().timestamp() + 100_000
}

// ── initialization ────────────────────────────────────────────────────────

#[test]
fn test_initialize_idempotent() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, ContractError::AlreadyInitialized);
}

// ── claim-type vocabulary ─────────────────────────────────────────────────

#[test]
fn test_cannot_issue_unknown_claim_type() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    // Register provider but NOT the claim type.
    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let err = client
        .try_issue_claim(
            &issuer,
            &subject,
            &pid,
            &scope(&env),
            &enrollment_type(&env),
            &future_expiry(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::UnknownClaimType);
}

// ── provider management ───────────────────────────────────────────────────

#[test]
fn test_zero_hash_provider_rejected() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let zero = BytesN::from_array(&env, &[0u8; 32]);
    let err = client
        .try_register_provider(&admin, &zero, &Symbol::new(&env, "bad"))
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::ProviderNotFound);
}

#[test]
fn test_revoked_provider_cannot_issue() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));
    client.revoke_provider(&admin, &pid);

    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let err = client
        .try_issue_claim(
            &issuer,
            &subject,
            &pid,
            &scope(&env),
            &enrollment_type(&env),
            &future_expiry(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::ProviderRevoked);
}

// ── claim issuance ────────────────────────────────────────────────────────

#[test]
fn test_issue_claim_and_has_valid_claim() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    let exp = future_expiry(&env);

    client.issue_claim(&issuer, &subject, &pid, &sc, &ct, &exp);
    assert!(client.has_valid_claim(&subject, &issuer, &sc, &ct));
}

#[test]
fn test_expired_claim_is_invalid() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    // expiry equal to current timestamp = not strictly future → rejected.
    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let past = env.ledger().timestamp();
    let err = client
        .try_issue_claim(&issuer, &subject, &pid, &scope(&env), &enrollment_type(&env), &past)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::InvalidExpiry);
}

#[test]
fn test_issuer_scoping_prevents_collision() {
    // Two different issuers for the same subject/scope/type → separate records.
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    let exp = future_expiry(&env);

    client.issue_claim(&issuer_a, &subject, &pid, &sc, &ct, &exp);
    client.issue_claim(&issuer_b, &subject, &pid, &sc, &ct, &exp);

    let claim_a = client.get_claim(&subject, &issuer_a, &sc, &ct);
    let claim_b = client.get_claim(&subject, &issuer_b, &sc, &ct);
    assert_eq!(claim_a.issuer, issuer_a);
    assert_eq!(claim_b.issuer, issuer_b);
}

// ── claim revocation ──────────────────────────────────────────────────────

#[test]
fn test_revoke_claim_by_issuer() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    client.issue_claim(&issuer, &subject, &pid, &sc, &ct, &future_expiry(&env));
    assert!(client.has_valid_claim(&subject, &issuer, &sc, &ct));

    client.revoke_claim(&issuer, &subject, &issuer, &sc, &ct);
    assert!(!client.has_valid_claim(&subject, &issuer, &sc, &ct));
}

#[test]
fn test_third_party_cannot_revoke_claim() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    client.issue_claim(&issuer, &subject, &pid, &sc, &ct, &future_expiry(&env));

    let err = client
        .try_revoke_claim(&attacker, &subject, &issuer, &sc, &ct)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::NotClaimIssuer);
}

// ── manual fallback path ──────────────────────────────────────────────────

#[test]
fn test_manual_claim_by_authorized_verifier() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.register_claim_type(&admin, &identity_type(&env));
    let verifier = Address::generate(&env);
    client.add_verifier(&admin, &verifier);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = identity_type(&env);
    client.issue_manual_claim(&verifier, &subject, &sc, &ct, &future_expiry(&env));
    let claim = client.get_claim(&subject, &verifier, &sc, &ct);
    assert_eq!(claim.source, ClaimSource::ManualVerifier);
}

#[test]
fn test_unauthorized_verifier_cannot_issue_manual_claim() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.register_claim_type(&admin, &identity_type(&env));
    let fake_verifier = Address::generate(&env);
    let subject = Address::generate(&env);
    let err = client
        .try_issue_manual_claim(
            &fake_verifier,
            &subject,
            &scope(&env),
            &identity_type(&env),
            &future_expiry(&env),
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, ContractError::NotAuthorizedIssuer);
}

// ── is_verified (multi-issuer check) ─────────────────────────────────────

#[test]
fn test_is_verified_accepts_any_valid_issuer() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer_a = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    client.issue_claim(&issuer_a, &subject, &pid, &sc, &ct, &future_expiry(&env));

    let issuers = vec![&env, issuer_a.clone()];
    assert!(client.is_verified(&subject, &sc, &ct, &issuers));
}

#[test]
fn test_is_verified_false_after_revocation() {
    let (env, _, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let pid = provider_id(&env);
    client.register_provider(&admin, &pid, &Symbol::new(&env, "uni"));
    client.register_claim_type(&admin, &enrollment_type(&env));

    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    let sc = scope(&env);
    let ct = enrollment_type(&env);
    client.issue_claim(&issuer, &subject, &pid, &sc, &ct, &future_expiry(&env));
    client.revoke_claim(&issuer, &subject, &issuer, &sc, &ct);

    let issuers = vec![&env, issuer];
    assert!(!client.is_verified(&subject, &sc, &ct, &issuers));
}
