use ed25519_dalek::{Signer, SigningKey};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env,
};

use crate::{FineError, FineStatus, LibraryFines, LibraryFinesClient};

// ===== Test infrastructure =====

struct Keys {
    signing_key: SigningKey,
    pubkey_bytes: [u8; 32],
}

impl Keys {
    fn new(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let pubkey_bytes = *signing_key.verifying_key().as_bytes();
        Keys { signing_key, pubkey_bytes }
    }

    fn sign_fine(
        &self,
        env: &Env,
        loan_id: &BytesN<32>,
        policy_id: &BytesN<32>,
        rule_version: u32,
        amount: i128,
        loan_start: u64,
        nonce: &BytesN<32>,
        expiry: u64,
    ) -> BytesN<64> {
        // Fixed layout: 16 + 32 + 32 + 4 + 16 + 8 + 32 + 8 = 148 bytes
        let mut msg = [0u8; 148];
        let mut pos = 0usize;
        msg[pos..pos + 16].copy_from_slice(b"CHAINVERSE_FINE:");
        pos += 16;
        msg[pos..pos + 32].copy_from_slice(&loan_id.to_array());
        pos += 32;
        msg[pos..pos + 32].copy_from_slice(&policy_id.to_array());
        pos += 32;
        msg[pos..pos + 4].copy_from_slice(&rule_version.to_be_bytes());
        pos += 4;
        msg[pos..pos + 16].copy_from_slice(&amount.to_be_bytes());
        pos += 16;
        msg[pos..pos + 8].copy_from_slice(&loan_start.to_be_bytes());
        pos += 8;
        msg[pos..pos + 32].copy_from_slice(&nonce.to_array());
        pos += 32;
        msg[pos..pos + 8].copy_from_slice(&expiry.to_be_bytes());
        let _ = pos;
        let sig = self.signing_key.sign(&msg);
        BytesN::from_array(env, &sig.to_bytes())
    }
}

struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    keys: Keys,
    policy_id: BytesN<32>,
    loan_id: BytesN<32>,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let keys = Keys::new([7u8; 32]);
    let admin = Address::generate(&env);
    let pubkey = BytesN::from_array(&env, &keys.pubkey_bytes);

    let contract = env.register(LibraryFines, ());
    LibraryFinesClient::new(&env, &contract).initialize(&admin, &pubkey);

    let policy_id = BytesN::from_array(&env, &[1u8; 32]);
    // per_assessment_cap=500, cumulative_cap=1000, grace_period=100s
    LibraryFinesClient::new(&env, &contract).set_policy(&admin, &policy_id, &500, &1_000, &100);

    let loan_id = BytesN::from_array(&env, &[2u8; 32]);

    Ctx { env, contract, admin, keys, policy_id, loan_id }
}

fn client(ctx: &Ctx) -> LibraryFinesClient {
    LibraryFinesClient::new(&ctx.env, &ctx.contract)
}

fn nonce(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

/// Assess a fine with a validly signed payload.
/// `loan_start=0` satisfies the grace period (grace_end = 0+100 = 100 < now=1000).
fn assess(ctx: &Ctx, amount: i128, nonce_seed: u8) -> BytesN<32> {
    let n = nonce(&ctx.env, nonce_seed);
    let loan_start = 0u64;
    let expiry = 9_999u64;
    let sig = ctx.keys.sign_fine(
        &ctx.env,
        &ctx.loan_id,
        &ctx.policy_id,
        1,
        amount,
        loan_start,
        &n,
        expiry,
    );
    client(ctx).assess_fine(
        &ctx.loan_id,
        &ctx.policy_id,
        &1u32,
        &amount,
        &loan_start,
        &n,
        &expiry,
        &sig,
    )
}

// ===== Positive paths =====

#[test]
fn test_assess_fine_valid_sig_creates_record() {
    let ctx = setup();
    let fine_id = assess(&ctx, 200, 1);
    let rec = client(&ctx).get_fine(&fine_id);
    assert_eq!(rec.loan_id, ctx.loan_id);
    assert_eq!(rec.policy_id, ctx.policy_id);
    assert_eq!(rec.amount, 200);
    assert_eq!(rec.rule_version, 1);
    assert_eq!(rec.status, FineStatus::Active);
}

#[test]
fn test_cumulative_debt_increases_with_assessments() {
    let ctx = setup();
    assess(&ctx, 200, 1);
    assess(&ctx, 150, 2);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 350);
}

#[test]
fn test_waive_fine_reduces_amount_and_debt() {
    let ctx = setup();
    let fine_id = assess(&ctx, 300, 1);
    client(&ctx).waive_fine(&ctx.admin, &fine_id, &100);
    let rec = client(&ctx).get_fine(&fine_id);
    assert_eq!(rec.amount, 200);
    assert_eq!(rec.status, FineStatus::Active);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 200);
}

#[test]
fn test_full_waiver_sets_status_waived() {
    let ctx = setup();
    let fine_id = assess(&ctx, 300, 1);
    client(&ctx).waive_fine(&ctx.admin, &fine_id, &300);
    let rec = client(&ctx).get_fine(&fine_id);
    assert_eq!(rec.amount, 0);
    assert_eq!(rec.status, FineStatus::Waived);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 0);
}

#[test]
fn test_rotate_institution_key_new_sig_accepted() {
    let ctx = setup();
    let new_keys = Keys::new([42u8; 32]);
    let new_pubkey = BytesN::from_array(&ctx.env, &new_keys.pubkey_bytes);
    client(&ctx).rotate_institution_key(&ctx.admin, &new_pubkey);
    let n = nonce(&ctx.env, 99);
    let sig = new_keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, 0, &n, 9_999);
    assert!(client(&ctx)
        .try_assess_fine(&ctx.loan_id, &ctx.policy_id, &1u32, &100, &0, &n, &9_999, &sig)
        .is_ok());
}

// ===== #978 — Signature verification =====

#[test]
fn test_invalid_signature_changes_no_state() {
    let ctx = setup();
    let n = nonce(&ctx.env, 10);
    let bad_sig = BytesN::from_array(&ctx.env, &[0xFF_u8; 64]);
    let result = client(&ctx).try_assess_fine(
        &ctx.loan_id,
        &ctx.policy_id,
        &1u32,
        &100,
        &0,
        &n,
        &9_999,
        &bad_sig,
    );
    assert_eq!(result, Err(Ok(FineError::InvalidSignature)));
    // The same nonce must still be usable — invalid sig does not consume it.
    assert!(try_assess(&ctx, 100, &n));
}

#[test]
fn test_nonce_replay_rejected() {
    let ctx = setup();
    let n = nonce(&ctx.env, 20);
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, 0, &n, 9_999);
    client(&ctx).assess_fine(&ctx.loan_id, &ctx.policy_id, &1u32, &100, &0, &n, &9_999, &sig);
    let sig2 = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, 0, &n, 9_999);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &100,
            &0,
            &n,
            &9_999,
            &sig2
        ),
        Err(Ok(FineError::NonceAlreadyUsed))
    );
}

#[test]
fn test_expired_assessment_rejected_before_nonce_consumed() {
    let ctx = setup();
    // now=1000, expiry=999 → expired
    let n = nonce(&ctx.env, 30);
    let expiry = 999u64;
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, 0, &n, expiry);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &100,
            &0,
            &n,
            &expiry,
            &sig
        ),
        Err(Ok(FineError::AssessmentExpired))
    );
    // Nonce was NOT consumed by the expired attempt — same nonce must succeed.
    assert!(try_assess(&ctx, 100, &n));
}

// ===== #978 — Per-assessment cap =====

#[test]
fn test_amount_at_per_assessment_cap_accepted() {
    let ctx = setup();
    // per_assessment_cap = 500; amount = 500 is exactly at the boundary.
    let fine_id = assess(&ctx, 500, 50);
    assert_eq!(client(&ctx).get_fine(&fine_id).amount, 500);
}

#[test]
fn test_amount_exceeding_per_assessment_cap_rejected() {
    let ctx = setup();
    let n = nonce(&ctx.env, 51);
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 501, 0, &n, 9_999);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &501,
            &0,
            &n,
            &9_999,
            &sig
        ),
        Err(Ok(FineError::CapExceeded))
    );
}

// ===== #979 — Grace period =====

#[test]
fn test_grace_period_active_rejects_assessment() {
    let ctx = setup();
    // now=1000, loan_start=950, grace_period=100 → grace_end=1050 > now → in grace
    let loan_start = 950u64;
    let n = nonce(&ctx.env, 60);
    let sig =
        ctx.keys
            .sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, loan_start, &n, 9_999);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &100,
            &loan_start,
            &n,
            &9_999,
            &sig
        ),
        Err(Ok(FineError::GracePeriodActive))
    );
}

#[test]
fn test_grace_period_boundary_at_grace_end_accepted() {
    let ctx = setup();
    // loan_start=900, grace_period=100 → grace_end=1000 = now → grace has ended
    let loan_start = 900u64;
    ctx.env.ledger().set_timestamp(1_000);
    let n = nonce(&ctx.env, 61);
    let sig =
        ctx.keys
            .sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 100, loan_start, &n, 9_999);
    assert!(client(&ctx)
        .try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &100,
            &loan_start,
            &n,
            &9_999,
            &sig
        )
        .is_ok());
}

#[test]
fn test_zero_grace_period_allows_immediate_assessment() {
    let ctx = setup();
    let no_grace_policy = BytesN::from_array(&ctx.env, &[50u8; 32]);
    client(&ctx).set_policy(&ctx.admin, &no_grace_policy, &500, &1_000, &0);
    let n = nonce(&ctx.env, 70);
    // loan_start = 999; grace_period = 0 → grace_end = 999 ≤ now (1000) → OK
    let loan_start = 999u64;
    let sig = ctx.keys.sign_fine(
        &ctx.env,
        &ctx.loan_id,
        &no_grace_policy,
        1,
        100,
        loan_start,
        &n,
        9_999,
    );
    assert!(client(&ctx)
        .try_assess_fine(
            &ctx.loan_id,
            &no_grace_policy,
            &1u32,
            &100,
            &loan_start,
            &n,
            &9_999,
            &sig
        )
        .is_ok());
}

// ===== #979 — Cumulative cap =====

#[test]
fn test_cumulative_cap_enforced() {
    let ctx = setup();
    // cumulative_cap = 1000; fill it with two 500 assessments.
    assess(&ctx, 500, 80);
    assess(&ctx, 500, 81);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 1_000);
    // Any further assessment is rejected.
    let n = nonce(&ctx.env, 82);
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 1, 0, &n, 9_999);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &1,
            &0,
            &n,
            &9_999,
            &sig
        ),
        Err(Ok(FineError::CumulativeCapExceeded))
    );
}

#[test]
fn test_cumulative_cap_boundary_at_exact_cap_accepted() {
    let ctx = setup();
    // cumulative_cap = 1000; two assessments of 500 exactly fill it.
    assess(&ctx, 500, 90);
    let fine_id = assess(&ctx, 500, 91);
    assert_eq!(client(&ctx).get_fine(&fine_id).amount, 500);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 1_000);
}

#[test]
fn test_waiver_reduces_cumulative_debt_allowing_further_assessment() {
    let ctx = setup();
    let fid = assess(&ctx, 500, 110);
    assess(&ctx, 400, 111);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 900);
    // Waive 200 → debt drops to 700, freeing 300 headroom within 1000 cap.
    client(&ctx).waive_fine(&ctx.admin, &fid, &200);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 700);
    let n = nonce(&ctx.env, 112);
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, 300, 0, &n, 9_999);
    assert!(client(&ctx)
        .try_assess_fine(
            &ctx.loan_id,
            &ctx.policy_id,
            &1u32,
            &300,
            &0,
            &n,
            &9_999,
            &sig
        )
        .is_ok());
}

// ===== #979 — Waivers and negative-debt prevention =====

#[test]
fn test_waiver_exceeding_fine_amount_rejected() {
    let ctx = setup();
    let fine_id = assess(&ctx, 100, 120);
    assert_eq!(
        client(&ctx).try_waive_fine(&ctx.admin, &fine_id, &101),
        Err(Ok(FineError::WaiverExceedsDebt))
    );
    // Fine amount must be unchanged.
    assert_eq!(client(&ctx).get_fine(&fine_id).amount, 100);
}

#[test]
fn test_waiver_at_exact_fine_amount_is_full_waiver() {
    let ctx = setup();
    let fine_id = assess(&ctx, 100, 121);
    client(&ctx).waive_fine(&ctx.admin, &fine_id, &100);
    assert_eq!(client(&ctx).get_fine(&fine_id).amount, 0);
    assert_eq!(client(&ctx).cumulative_debt(&ctx.loan_id), 0);
}

#[test]
fn test_zero_waiver_amount_rejected() {
    let ctx = setup();
    let fine_id = assess(&ctx, 200, 122);
    assert_eq!(
        client(&ctx).try_waive_fine(&ctx.admin, &fine_id, &0),
        Err(Ok(FineError::InvalidAmount))
    );
}

#[test]
fn test_non_admin_cannot_waive() {
    let ctx = setup();
    let fine_id = assess(&ctx, 200, 130);
    let attacker = Address::generate(&ctx.env);
    assert_eq!(
        client(&ctx).try_waive_fine(&attacker, &fine_id, &50),
        Err(Ok(FineError::Unauthorized))
    );
}

// ===== Authorization / initialization =====

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    let pubkey = BytesN::from_array(&ctx.env, &ctx.keys.pubkey_bytes);
    assert_eq!(
        client(&ctx).try_initialize(&ctx.admin, &pubkey),
        Err(Ok(FineError::AlreadyInitialized))
    );
}

#[test]
fn test_set_policy_non_admin_rejected() {
    let ctx = setup();
    let attacker = Address::generate(&ctx.env);
    let pid = BytesN::from_array(&ctx.env, &[9u8; 32]);
    assert_eq!(
        client(&ctx).try_set_policy(&attacker, &pid, &100, &500, &0),
        Err(Ok(FineError::Unauthorized))
    );
}

#[test]
fn test_assess_fine_unknown_policy_rejected() {
    let ctx = setup();
    let unknown_policy = BytesN::from_array(&ctx.env, &[99u8; 32]);
    let n = nonce(&ctx.env, 140);
    let sig = ctx
        .keys
        .sign_fine(&ctx.env, &ctx.loan_id, &unknown_policy, 1, 100, 0, &n, 9_999);
    assert_eq!(
        client(&ctx).try_assess_fine(
            &ctx.loan_id,
            &unknown_policy,
            &1u32,
            &100,
            &0,
            &n,
            &9_999,
            &sig
        ),
        Err(Ok(FineError::PolicyNotFound))
    );
}

#[test]
fn test_get_fine_not_found() {
    let ctx = setup();
    let missing = BytesN::from_array(&ctx.env, &[99u8; 32]);
    assert_eq!(
        client(&ctx).try_get_fine(&missing),
        Err(Ok(FineError::NotFound))
    );
}

#[test]
fn test_zero_debt_for_untouched_loan() {
    let ctx = setup();
    let other_loan = BytesN::from_array(&ctx.env, &[77u8; 32]);
    assert_eq!(client(&ctx).cumulative_debt(&other_loan), 0);
}

// ===== Helpers =====

fn try_assess(ctx: &Ctx, amount: i128, n: &BytesN<32>) -> bool {
    let sig = ctx.keys.sign_fine(&ctx.env, &ctx.loan_id, &ctx.policy_id, 1, amount, 0, n, 9_999);
    client(ctx)
        .try_assess_fine(&ctx.loan_id, &ctx.policy_id, &1u32, &amount, &0, n, &9_999, &sig)
        .is_ok()
}
