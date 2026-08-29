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
use crate::{
    ContractError, EntryKind, LibraryFinesContract, LibraryFinesContractClient, SettlementState,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String};

// ---- helpers ----------------------------------------------------------------

fn setup() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register(LibraryFinesContract, ());
    (env, contract_id)
}

fn bootstrapped<'a>(
    env: &'a Env,
    cid: &Address,
) -> (LibraryFinesContractClient<'a>, Address, Address, Address) {
    env.mock_all_auths();
    let client = LibraryFinesContractClient::new(env, cid);
    let admin = Address::generate(env);
    let librarian = Address::generate(env);
    let asset = Address::generate(env);
    client.initialize(&admin, &librarian);
    client.add_supported_asset(&admin, &asset);
    (client, admin, librarian, asset)
}

fn rid(env: &Env, b: u8) -> BytesN<32> {
    BytesN::from_array(env, &[b; 32])
}

fn patron(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0xAA; 32])
}

// ============================================================
// #980 — Immutable charge ledger
// ============================================================

// -- Positive --

#[test]
fn test_assess_creates_entry_and_increases_balance() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &500i128, &rid(&env, 99));

    assert_eq!(client.get_balance(&p), 500i128);
    let entry = client.get_entry(&p, &0u32);
    assert_eq!(entry.delta, 500i128);
    assert!(matches!(entry.kind, EntryKind::Assessment));
}

#[test]
fn test_multiple_assessments_accumulate_balance() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 91));
    client.assess(&librarian, &p, &rid(&env, 2), &300i128, &rid(&env, 92));

    assert_eq!(client.get_balance(&p), 500i128);
}

#[test]
fn test_balance_derivable_from_entry_deltas() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &500i128, &rid(&env, 90));
    client.waive(&librarian, &p, &rid(&env, 2), &100i128, &rid(&env, 91));

    let balance = client.get_balance(&p);
    let entries = client.get_entries(&p, &0u32, &50u32);
    let mut derived: i128 = 0;
    for i in 0..entries.len() {
        derived += entries.get(i).unwrap().delta;
    }
    assert_eq!(balance, derived);
    assert_eq!(balance, 400i128);
}

#[test]
fn test_get_entries_returns_correct_slice() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    for i in 1u8..=5u8 {
        client.assess(&librarian, &p, &rid(&env, i), &100i128, &rid(&env, 50 + i));
    }
    let page = client.get_entries(&p, &2u32, &2u32);
    assert_eq!(page.len(), 2u32);
}

#[test]
fn test_admin_can_also_assess() {
    let (env, cid) = setup();
    let (client, admin, _, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&admin, &p, &rid(&env, 1), &250i128, &rid(&env, 90));
    assert_eq!(client.get_balance(&p), 250i128);
}

// -- Negative --

#[test]
fn test_assess_duplicate_ref_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    let result = client.try_assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 91));

    assert_eq!(result, Err(Ok(ContractError::DuplicateReference)));
}

#[test]
fn test_assess_zero_amount_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);

    let result = client.try_assess(&librarian, &patron(&env), &rid(&env, 1), &0i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn test_assess_negative_amount_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);

    let result = client.try_assess(&librarian, &patron(&env), &rid(&env, 1), &-1i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

// -- Authorization --

#[test]
fn test_assess_unauthorized_caller_fails() {
    let (env, cid) = setup();
    let (client, _, _, _) = bootstrapped(&env, &cid);
    let intruder = Address::generate(&env);

    let result = client.try_assess(&intruder, &patron(&env), &rid(&env, 1), &100i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// -- Boundary --

#[test]
fn test_get_entries_cursor_beyond_count_returns_empty() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    let page = client.get_entries(&p, &999u32, &10u32);
    assert_eq!(page.len(), 0u32);
}

#[test]
fn test_get_entries_limit_capped_silently() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    for i in 1u8..=3u8 {
        client.assess(&librarian, &p, &rid(&env, i), &100i128, &rid(&env, 50 + i));
    }
    // Request 200 entries but only 3 exist; must return 3, not panic.
    let page = client.get_entries(&p, &0u32, &200u32);
    assert_eq!(page.len(), 3u32);
}

#[test]
fn test_get_entry_not_found_fails() {
    let (env, cid) = setup();
    let (client, _, _, _) = bootstrapped(&env, &cid);

    let result = client.try_get_entry(&patron(&env), &0u32);
    assert_eq!(result, Err(Ok(ContractError::EntryNotFound)));
}

// ============================================================
// #981 — Governed fine waivers
// ============================================================

// -- Positive --

#[test]
fn test_full_waiver_zeroes_balance() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &300i128, &rid(&env, 90));
    client.waive(&librarian, &p, &rid(&env, 2), &300i128, &rid(&env, 91));

    assert_eq!(client.get_balance(&p), 0i128);
}

#[test]
fn test_partial_waiver_reduces_balance() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &400i128, &rid(&env, 90));
    client.waive(&librarian, &p, &rid(&env, 2), &150i128, &rid(&env, 91));

    assert_eq!(client.get_balance(&p), 250i128);
}

#[test]
fn test_waiver_preserves_assessment_history() {
    // The Assessment entry must still exist after a Waiver is appended.
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    client.waive(&librarian, &p, &rid(&env, 2), &200i128, &rid(&env, 91));

    let assessment = client.get_entry(&p, &0u32);
    assert!(matches!(assessment.kind, EntryKind::Assessment));

    let waiver = client.get_entry(&p, &1u32);
    assert!(matches!(waiver.kind, EntryKind::Waiver));
}

// -- Negative --

#[test]
fn test_waiver_exceeds_balance_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    let result = client.try_waive(&librarian, &p, &rid(&env, 2), &101i128, &rid(&env, 91));

    assert_eq!(result, Err(Ok(ContractError::WaiverExceedsBalance)));
}

#[test]
fn test_waiver_on_zero_balance_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);

    let result = client.try_waive(&librarian, &patron(&env), &rid(&env, 1), &50i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::WaiverExceedsBalance)));
}

#[test]
fn test_waiver_zero_amount_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);

    let result = client.try_waive(&librarian, &patron(&env), &rid(&env, 1), &0i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

#[test]
fn test_waiver_duplicate_ref_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    // Attempt to use the same ref_id as the existing assessment.
    let result = client.try_waive(&librarian, &p, &rid(&env, 1), &50i128, &rid(&env, 91));
    assert_eq!(result, Err(Ok(ContractError::DuplicateReference)));
}

// -- Authorization --

#[test]
fn test_waiver_unauthorized_caller_fails() {
    let (env, cid) = setup();
    let (client, _, _, _) = bootstrapped(&env, &cid);
    let intruder = Address::generate(&env);

    let result = client.try_waive(&intruder, &patron(&env), &rid(&env, 1), &50i128, &rid(&env, 90));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// -- Boundary --

#[test]
fn test_waiver_of_exact_balance_succeeds() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &77i128, &rid(&env, 90));
    client.waive(&librarian, &p, &rid(&env, 2), &77i128, &rid(&env, 91));

    assert_eq!(client.get_balance(&p), 0i128);
}

// ============================================================
// #982 — Accept Stellar assets for library balances
// ============================================================

// -- Positive --

#[test]
fn test_initiate_payment_creates_pending_settlement() {
    let (env, cid) = setup();
    let (client, _, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &200i128);

    let settlement = client.get_settlement(&rid(&env, 10));
    assert!(matches!(settlement.state, SettlementState::Pending));
    assert_eq!(settlement.amount, 200i128);
}

#[test]
fn test_confirm_payment_reduces_balance_atomically() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &200i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    assert_eq!(client.get_balance(&p), 0i128);
    assert!(matches!(
        client.get_settlement(&rid(&env, 10)).state,
        SettlementState::Confirmed
    ));
}

#[test]
fn test_partial_payment_reduces_balance_by_amount() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &80i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    assert_eq!(client.get_balance(&p), 120i128);
}

#[test]
fn test_confirm_receipt_identifies_ledger_entry() {
    // The confirmed Payment entry must exist in the patron ledger.
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    let payment_entry = client.get_entry(&p, &1u32); // seq 1 (after assessment)
    assert!(matches!(payment_entry.kind, EntryKind::Payment));
    assert_eq!(payment_entry.delta, -100i128);
}

// -- Negative --

#[test]
fn test_initiate_unsupported_asset_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, _) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let bad_asset = Address::generate(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    let result = client.try_initiate_payment(&payer, &p, &rid(&env, 10), &bad_asset, &100i128);
    assert_eq!(result, Err(Ok(ContractError::UnsupportedAsset)));
}

#[test]
fn test_initiate_duplicate_settlement_fails() {
    let (env, cid) = setup();
    let (client, _, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &200i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    let result = client.try_initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    assert_eq!(result, Err(Ok(ContractError::DuplicateSettlement)));
}

#[test]
fn test_initiate_zero_amount_fails() {
    let (env, cid) = setup();
    let (client, _, _, asset) = bootstrapped(&env, &cid);
    let payer = Address::generate(&env);

    let result = client.try_initiate_payment(&payer, &patron(&env), &rid(&env, 10), &asset, &0i128);
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

// -- Authorization --

#[test]
fn test_confirm_payment_requires_admin() {
    let (env, cid) = setup();
    let (client, _, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    let result = client.try_confirm_payment(&librarian, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// -- Boundary --

#[test]
fn test_confirm_payment_exceeding_balance_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    // Initiate with amount greater than the assessed balance.
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &200i128);
    let result = client.try_confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    assert_eq!(result, Err(Ok(ContractError::PaymentExceedsBalance)));
}

// ============================================================
// #983 — Reconcile failed or reversed fine payments
// ============================================================

// -- Positive --

#[test]
fn test_fail_pending_transitions_state_and_leaves_balance_unchanged() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.fail_payment(&admin, &rid(&env, 10));

    assert!(matches!(
        client.get_settlement(&rid(&env, 10)).state,
        SettlementState::Failed
    ));
    assert_eq!(client.get_balance(&p), 100i128);
}

#[test]
fn test_refund_confirmed_restores_balance() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &150i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &150i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    assert_eq!(client.get_balance(&p), 0i128);

    client.refund_payment(&admin, &rid(&env, 10), &rid(&env, 12), &rid(&env, 93));
    assert_eq!(client.get_balance(&p), 150i128);
    assert!(matches!(
        client.get_settlement(&rid(&env, 10)).state,
        SettlementState::Refunded
    ));
}

#[test]
fn test_reverse_confirmed_restores_balance() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &80i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &80i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    client.reverse_payment(&admin, &rid(&env, 10), &rid(&env, 12), &rid(&env, 93));

    assert_eq!(client.get_balance(&p), 80i128);
    assert!(matches!(
        client.get_settlement(&rid(&env, 10)).state,
        SettlementState::Reversed
    ));
}

// -- Negative --

#[test]
fn test_confirm_already_confirmed_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    let result = client.try_confirm_payment(&admin, &rid(&env, 10), &rid(&env, 13), &rid(&env, 94));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

#[test]
fn test_fail_confirmed_settlement_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    let result = client.try_fail_payment(&admin, &rid(&env, 10));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

#[test]
fn test_refund_pending_settlement_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);

    let result = client.try_refund_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

#[test]
fn test_reverse_pending_settlement_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);

    let result = client.try_reverse_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

#[test]
fn test_fail_failed_settlement_fails() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.fail_payment(&admin, &rid(&env, 10));

    let result = client.try_fail_payment(&admin, &rid(&env, 10));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

#[test]
fn test_confirm_unknown_settlement_fails() {
    let (env, cid) = setup();
    let (client, admin, _, _) = bootstrapped(&env, &cid);

    let result = client.try_confirm_payment(&admin, &rid(&env, 99), &rid(&env, 1), &rid(&env, 2));
    assert_eq!(result, Err(Ok(ContractError::SettlementNotFound)));
}

// -- Authorization --

#[test]
fn test_fail_payment_requires_admin() {
    let (env, cid) = setup();
    let (client, _, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    let result = client.try_fail_payment(&librarian, &rid(&env, 10));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_refund_payment_requires_admin() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    let result = client.try_refund_payment(&librarian, &rid(&env, 10), &rid(&env, 12), &rid(&env, 93));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn test_reverse_payment_requires_admin() {
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    let result = client.try_reverse_payment(&librarian, &rid(&env, 10), &rid(&env, 12), &rid(&env, 93));
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// -- Boundary --

#[test]
fn test_balance_not_credited_twice() {
    // Confirming the same settlement_id a second time must be rejected;
    // the balance must not go below zero.
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &100i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &100i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));

    let result = client.try_confirm_payment(&admin, &rid(&env, 10), &rid(&env, 13), &rid(&env, 94));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
    assert_eq!(client.get_balance(&p), 0i128); // unchanged, not double-credited
}

#[test]
fn test_refund_then_reverse_same_settlement_fails() {
    // Once Refunded, the settlement is terminal: Reversed must be rejected.
    let (env, cid) = setup();
    let (client, admin, librarian, asset) = bootstrapped(&env, &cid);
    let p = patron(&env);
    let payer = Address::generate(&env);

    client.assess(&librarian, &p, &rid(&env, 1), &50i128, &rid(&env, 90));
    client.initiate_payment(&payer, &p, &rid(&env, 10), &asset, &50i128);
    client.confirm_payment(&admin, &rid(&env, 10), &rid(&env, 11), &rid(&env, 92));
    client.refund_payment(&admin, &rid(&env, 10), &rid(&env, 12), &rid(&env, 93));

    let result = client.try_reverse_payment(&admin, &rid(&env, 10), &rid(&env, 14), &rid(&env, 95));
    assert_eq!(result, Err(Ok(ContractError::InvalidStateTransition)));
}

// ============================================================
// General
// ============================================================

#[test]
fn test_double_initialize_fails() {
    let (env, cid) = setup();
    env.mock_all_auths();
    let client = LibraryFinesContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    let librarian = Address::generate(&env);
    client.initialize(&admin, &librarian);

    let result = client.try_initialize(&Address::generate(&env), &Address::generate(&env));
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

#[test]
fn test_version_is_semver() {
    let (env, cid) = setup();
    let client = LibraryFinesContractClient::new(&env, &cid);
    assert_eq!(client.version(), String::from_str(&env, "0.1.0"));
}
