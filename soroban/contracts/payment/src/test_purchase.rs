//! Native tests for authorized Stellar purchases (issue #915).
//!
//! Coverage:
//! - Real Soroban token (Stellar Asset Contract) fixtures – funds actually
//!   move between student, contract escrow, and accounting records.
//! - Authorization: missing student authorization rejects with no mutation.
//! - Idempotency: duplicate enrollments and duplicate payment IDs (identical
//!   or conflicting arguments) are rejected.
//! - Atomicity: failed transfers leave no payment, enrollment, or
//!   accounting state behind.
//! - Split accounting: truncating fee rounding, per-course override,
//!   global-fee fallback, maximum-fee boundary, large amounts.
//! - Configuration races: price/asset changes are honoured at execution.
//!
//! Uses Soroban's native test environment (`Env::default()`).
#![cfg(test)]

extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, FromVal, Symbol, TryFromVal,
};

use crate::{ContractError, PaymentContract, PaymentContractClient};

// ─── Helper: extract inner error from try_* call ─────────────────────────────

macro_rules! contract_err {
    ($r:expr) => {
        match $r {
            Err(Ok(e)) => e,
            other => panic!("expected contract error, got {:?}", other),
        }
    };
}

// ─── Fixture ─────────────────────────────────────────────────────────────────

/// Test fixture backed by two real Stellar Asset Contracts.
struct Fixture {
    env: Env,
    contract: Address,
    admin: Address,
    /// Platform treasury (kept for completeness of the initialization args).
    #[allow(dead_code)]
    treasury: Address,
    instructor: Address,
    student: Address,
    student2: Address,
    /// SAC for asset 1 – backs the default course `RUST101`.
    asset1: Address,
    /// SAC for asset 2 – backs the secondary course `WEB3`.
    asset2: Address,
}

impl Fixture {
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let instructor = Address::generate(&env);
        let student = Address::generate(&env);
        let student2 = Address::generate(&env);

        let contract = env.register_contract(None, PaymentContract {});
        let client = PaymentContractClient::new(&env, &contract);

        // Global platform fee: 500 bps (5 %).
        client.initialize(&admin, &treasury, &500u32, &86_400u64);

        // Two distinct issued assets, each deployed as a real SAC.
        let sac1 = env.register_stellar_asset_contract_v2(admin.clone());
        let sac2 = env.register_stellar_asset_contract_v2(treasury.clone());
        let asset1 = sac1.address();
        let asset2 = sac2.address();

        client.add_asset(&admin, &asset1, &true);
        client.add_asset(&admin, &asset2, &true);

        // Default course: price 1_000_000, course fee override 100 bps (1 %).
        let course_id = Symbol::new(&env, "RUST101");
        client.add_course(
            &admin,
            &course_id,
            &1_000_000i128,
            &asset1,
            &instructor,
            &100u32,
            &true,
        );

        // Fund students generously with both assets.
        StellarAssetClient::new(&env, &asset1).mint(&student, &100_000_000_000i128);
        StellarAssetClient::new(&env, &asset1).mint(&student2, &100_000_000_000i128);
        StellarAssetClient::new(&env, &asset2).mint(&student, &100_000_000_000i128);

        Fixture {
            env,
            contract,
            admin,
            treasury,
            instructor,
            student,
            student2,
            asset1,
            asset2,
        }
    }

    fn client(&self) -> PaymentContractClient<'_> {
        PaymentContractClient::new(&self.env, &self.contract)
    }

    fn token(&self, asset: &Address) -> TokenClient<'_> {
        TokenClient::new(&self.env, asset)
    }

    fn course_id(&self) -> Symbol {
        Symbol::new(&self.env, "RUST101")
    }

    fn payment_id(&self, tag: &str) -> Symbol {
        Symbol::new(&self.env, tag)
    }

    /// Total gross amount ever received by the contract escrow in `asset`.
    fn escrow_balance(&self, asset: &Address) -> i128 {
        self.token(asset).balance(&self.contract)
    }
}

// ─── Happy path ──────────────────────────────────────────────────────────────

#[test]
fn test_purchase_success_moves_funds_creates_enrollment_and_record() {
    let f = Fixture::new();
    let course_id = f.course_id();
    let pid = f.payment_id("PID_ONE");

    let student_before = f.token(&f.asset1).balance(&f.student);
    f.client().pay_for_course(&f.student, &course_id, &pid);

    // Exact configured amount left the student and reached the escrow.
    assert_eq!(
        f.token(&f.asset1).balance(&f.student),
        student_before - 1_000_000i128
    );
    assert_eq!(f.escrow_balance(&f.asset1), 1_000_000i128);

    // Enrollment created.
    assert!(f.client().is_enrolled(&f.student, &course_id));

    // Payment record persisted with the frozen schema and split allocation.
    let rec = f
        .client()
        .get_payment_record(&f.student, &course_id)
        .expect("payment record must exist");
    assert_eq!(rec.student, f.student);
    assert_eq!(rec.course_id, course_id);
    assert_eq!(rec.amount, 1_000_000i128);
    assert_eq!(rec.asset, f.asset1);
    assert_eq!(rec.paid_at, f.env.ledger().timestamp());
    assert_eq!(rec.payment_id, pid);
    assert_eq!(rec.fee_amount, 10_000i128); // 100 bps × 1_000_000
    assert_eq!(rec.instructor_amount, 990_000i128);
    assert_eq!(rec.fee_amount + rec.instructor_amount, rec.amount);

    // Instructor credited with the net proceeds.
    assert_eq!(
        f.client().get_instructor_balance(&f.instructor),
        990_000i128
    );
}

#[test]
fn test_purchase_emits_frozen_pymt_rcd_event() {
    let f = Fixture::new();
    let course_id = f.course_id();
    let pid = f.payment_id("EVT_PID");

    f.client().pay_for_course(&f.student, &course_id, &pid);

    let found = f.env.events().all().iter().any(|(emitter, topics, data)| {
        emitter == f.contract
            && topics.len() == 1
            && Symbol::try_from_val(&f.env, &topics.get(0u32).unwrap()).unwrap()
                == symbol_short!("PYMT_RCD")
            && <(Address, Symbol, i128, Address, Address, Symbol)>::from_val(&f.env, &data)
                == (
                    f.student.clone(),
                    course_id.clone(),
                    1_000_000i128,
                    f.asset1.clone(),
                    f.instructor.clone(),
                    pid.clone(),
                )
    });
    assert!(found, "PYMT_RCD event with frozen payload must be emitted");
}

#[test]
fn test_second_course_on_different_asset_for_same_student() {
    let f = Fixture::new();

    // Secondary course priced in asset2 with the global fee (fee_bps = 0).
    let web3 = Symbol::new(&f.env, "WEB3");
    f.client().add_course(
        &f.admin,
        &web3,
        &2_000_000i128,
        &f.asset2,
        &f.instructor,
        &0u32,
        &true,
    );

    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("C1"));
    f.client()
        .pay_for_course(&f.student, &web3, &f.payment_id("C2"));

    assert!(f.client().is_enrolled(&f.student, &f.course_id()));
    assert!(f.client().is_enrolled(&f.student, &web3));

    // asset2 purchase uses the global 500 bps fee: 100_000 fee / 1_900_000 net.
    assert_eq!(
        f.client().get_instructor_balance(&f.instructor),
        2_890_000i128
    );
    let rec = f.client().get_payment_record(&f.student, &web3).unwrap();
    assert_eq!(rec.asset, f.asset2);
    assert_eq!(rec.fee_amount, 100_000i128);
    assert_eq!(f.escrow_balance(&f.asset2), 2_000_000i128);
}

#[test]
fn test_multiple_students_can_buy_the_same_course() {
    let f = Fixture::new();

    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("S1"));
    f.client()
        .pay_for_course(&f.student2, &f.course_id(), &f.payment_id("S2"));

    assert!(f.client().is_enrolled(&f.student, &f.course_id()));
    assert!(f.client().is_enrolled(&f.student2, &f.course_id()));

    let rec1 = f
        .client()
        .get_payment_record(&f.student, &f.course_id())
        .unwrap();
    let rec2 = f
        .client()
        .get_payment_record(&f.student2, &f.course_id())
        .unwrap();
    assert_eq!(rec1.payment_id, f.payment_id("S1"));
    assert_eq!(rec2.payment_id, f.payment_id("S2"));

    // Both net proceeds accumulate for the instructor.
    assert_eq!(
        f.client().get_instructor_balance(&f.instructor),
        1_980_000i128
    );
    assert_eq!(f.escrow_balance(&f.asset1), 2_000_000i128);
}

// ─── Authorization ───────────────────────────────────────────────────────────

#[test]
fn test_purchase_without_student_authorization_fails_with_no_state_change() {
    let f = Fixture::new();

    // Strip every authorization entry so `require_auth` cannot be satisfied.
    f.env.set_auths(&[]);

    let result = f
        .client()
        .try_pay_for_course(&f.student, &f.course_id(), &f.payment_id("NOAUTH"));
    assert!(
        matches!(result, Err(Err(_))),
        "missing authorization must abort the invocation"
    );

    // Restore mocking and prove nothing mutated.
    f.env.mock_all_auths();
    assert!(!f.client().is_enrolled(&f.student, &f.course_id()));
    assert!(f
        .client()
        .get_payment_record(&f.student, &f.course_id())
        .is_none());
    assert_eq!(f.client().get_instructor_balance(&f.instructor), 0i128);
    assert_eq!(f.escrow_balance(&f.asset1), 0i128);
}

// ─── Validation failures ─────────────────────────────────────────────────────

#[test]
fn test_purchase_missing_course_fails() {
    let f = Fixture::new();
    let ghost = Symbol::new(&f.env, "GHOST404");

    let err = contract_err!(f
        .client()
        .try_pay_for_course(&f.student, &ghost, &f.payment_id("X1")));
    assert_eq!(err, ContractError::CourseNotFound);
}

#[test]
fn test_purchase_inactive_course_fails() {
    let f = Fixture::new();
    f.client().deactivate_course(&f.admin, &f.course_id());

    let err = contract_err!(f.client().try_pay_for_course(
        &f.student,
        &f.course_id(),
        &f.payment_id("X2")
    ));
    assert_eq!(err, ContractError::CourseInactive);
    assert!(!f.client().is_enrolled(&f.student, &f.course_id()));
    assert_eq!(f.escrow_balance(&f.asset1), 0i128);
}

#[test]
fn test_purchase_disabled_asset_fails() {
    let f = Fixture::new();
    f.client().disable_asset(&f.admin, &f.asset1);

    let err = contract_err!(f.client().try_pay_for_course(
        &f.student,
        &f.course_id(),
        &f.payment_id("X3")
    ));
    assert_eq!(err, ContractError::AssetNotEnabled);
    assert!(!f.client().is_enrolled(&f.student, &f.course_id()));
}

#[test]
fn test_purchase_with_exact_balance_succeeds() {
    let f = Fixture::new();

    // Minimum viable balance: exactly the configured price, nothing more.
    let exact = Address::generate(&f.env);
    StellarAssetClient::new(&f.env, &f.asset1).mint(&exact, &1_000_000i128);

    f.client()
        .pay_for_course(&exact, &f.course_id(), &f.payment_id("EXACT"));

    assert_eq!(f.token(&f.asset1).balance(&exact), 0i128);
    assert!(f.client().is_enrolled(&exact, &f.course_id()));
    assert_eq!(f.escrow_balance(&f.asset1), 1_000_000i128);
}

#[test]
fn test_empty_payment_id_fails() {
    let f = Fixture::new();

    let err = contract_err!(f.client().try_pay_for_course(
        &f.student,
        &f.course_id(),
        &Symbol::new(&f.env, "")
    ));
    assert_eq!(err, ContractError::InvalidPaymentId);
    assert!(!f.client().is_enrolled(&f.student, &f.course_id()));
}

// ─── Token-transfer failure & atomicity ──────────────────────────────────────

#[test]
fn test_insufficient_balance_fails_atomically() {
    let f = Fixture::new();

    // student2 keeps its funds; drain nothing – use an unfunded third student.
    let broke = Address::generate(&f.env);

    let err = contract_err!(f.client().try_pay_for_course(
        &broke,
        &f.course_id(),
        &f.payment_id("BROKE")
    ));
    assert_eq!(err, ContractError::PaymentFailed);

    // No partial state may survive the failed transfer.
    assert!(!f.client().is_enrolled(&broke, &f.course_id()));
    assert!(f
        .client()
        .get_payment_record(&broke, &f.course_id())
        .is_none());
    assert!(f
        .client()
        .get_payment_by_id(&f.payment_id("BROKE"))
        .is_none());
    assert_eq!(f.client().get_instructor_balance(&f.instructor), 0i128);
    assert_eq!(f.escrow_balance(&f.asset1), 0i128);
}

// ─── Business-level idempotency ──────────────────────────────────────────────

#[test]
fn test_duplicate_enrollment_rejected_even_with_new_payment_id() {
    let f = Fixture::new();
    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("FIRST"));

    let balance_before = f.token(&f.asset1).balance(&f.student);
    let err = contract_err!(f.client().try_pay_for_course(
        &f.student,
        &f.course_id(),
        &f.payment_id("SECOND")
    ));
    assert_eq!(err, ContractError::AlreadyEnrolled);

    // Funds moved exactly once; original receipt untouched.
    assert_eq!(f.token(&f.asset1).balance(&f.student), balance_before);
    let rec = f
        .client()
        .get_payment_record(&f.student, &f.course_id())
        .unwrap();
    assert_eq!(rec.payment_id, f.payment_id("FIRST"));
    assert_eq!(
        f.client().get_instructor_balance(&f.instructor),
        990_000i128
    );
}

#[test]
fn test_duplicate_payment_id_with_identical_arguments_rejected() {
    let f = Fixture::new();
    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("REPLAY"));

    // Exact replay: enrollment guard fires first (ADR precedence).
    let err = contract_err!(f.client().try_pay_for_course(
        &f.student,
        &f.course_id(),
        &f.payment_id("REPLAY")
    ));
    assert_eq!(err, ContractError::AlreadyEnrolled);
    assert_eq!(f.escrow_balance(&f.asset1), 1_000_000i128);
}

#[test]
fn test_duplicate_payment_id_with_conflicting_arguments_rejected() {
    let f = Fixture::new();
    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("CLASH"));

    let student2_before = f.token(&f.asset1).balance(&f.student2);
    let err = contract_err!(f.client().try_pay_for_course(
        &f.student2,
        &f.course_id(),
        &f.payment_id("CLASH")
    ));
    assert_eq!(err, ContractError::DuplicatePaymentId);

    // Conflicting caller neither enrolled nor charged.
    assert!(!f.client().is_enrolled(&f.student2, &f.course_id()));
    assert_eq!(f.token(&f.asset1).balance(&f.student2), student2_before);
    assert!(f
        .client()
        .get_payment_by_id(&f.payment_id("CLASH"))
        .is_some()); // owned by student
}

// ─── Price/asset configuration races ────────────────────────────────────────

#[test]
fn test_price_changed_before_execution_charges_current_configured_price() {
    let f = Fixture::new();
    f.client().update_course(
        &f.admin,
        &f.course_id(),
        &2_500_000i128,
        &f.asset1,
        &f.instructor,
        &100u32,
        &true,
    );

    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("LATE"));

    let rec = f
        .client()
        .get_payment_record(&f.student, &f.course_id())
        .unwrap();
    assert_eq!(rec.amount, 2_500_000i128);
    assert_eq!(rec.fee_amount, 25_000i128);
    assert_eq!(rec.instructor_amount, 2_475_000i128);
    assert_eq!(
        f.client().get_instructor_balance(&f.instructor),
        2_475_000i128
    );
}

// ─── Split accounting edge cases ─────────────────────────────────────────────

#[test]
fn test_fee_rounding_truncates_to_floor() {
    let f = Fixture::new();

    // 999 × 333 bps = 332_667 / 10_000 → fee truncated to 33, instructor 966.
    let tiny = Symbol::new(&f.env, "TINY101");
    f.client().add_course(
        &f.admin,
        &tiny,
        &999i128,
        &f.asset1,
        &f.instructor,
        &333u32,
        &true,
    );
    f.client()
        .pay_for_course(&f.student, &tiny, &f.payment_id("TRUNC"));

    let rec = f.client().get_payment_record(&f.student, &tiny).unwrap();
    assert_eq!(rec.amount, 999i128);
    assert_eq!(rec.fee_amount, 33i128);
    assert_eq!(rec.instructor_amount, 966i128);
    assert_eq!(rec.fee_amount + rec.instructor_amount, rec.amount);
    assert_eq!(f.client().get_instructor_balance(&f.instructor), 966i128);
}

#[test]
fn test_maximum_fee_boundary_is_respected() {
    let f = Fixture::new();

    let max_fee_course = Symbol::new(&f.env, "MAXFEE");
    f.client().add_course(
        &f.admin,
        &max_fee_course,
        &1_000_000i128,
        &f.asset1,
        &f.instructor,
        &2_000u32, // 20 % ceiling
        &true,
    );
    f.client()
        .pay_for_course(&f.student, &max_fee_course, &f.payment_id("MAXP"));

    let rec = f
        .client()
        .get_payment_record(&f.student, &max_fee_course)
        .unwrap();
    assert_eq!(rec.fee_amount, 200_000i128);
    assert_eq!(rec.instructor_amount, 800_000i128);
}

#[test]
fn test_zero_course_fee_falls_back_to_global_fee() {
    let f = Fixture::new();

    let global_fee_course = Symbol::new(&f.env, "GLOBAL");
    f.client().add_course(
        &f.admin,
        &global_fee_course,
        &1_000_000i128,
        &f.asset1,
        &f.instructor,
        &0u32, // override disabled → global 500 bps applies
        &true,
    );
    f.client()
        .pay_for_course(&f.student, &global_fee_course, &f.payment_id("GLBL"));

    let rec = f
        .client()
        .get_payment_record(&f.student, &global_fee_course)
        .unwrap();
    assert_eq!(rec.fee_amount, 50_000i128);
    assert_eq!(rec.instructor_amount, 950_000i128);
}

#[test]
fn test_large_amounts_do_not_overflow_split_math() {
    let f = Fixture::new();

    let big = 4_000_000_000_000_000_000i128; // 4e18 stroops
    let big_course = Symbol::new(&f.env, "BIG101");
    f.client().add_course(
        &f.admin,
        &big_course,
        &big,
        &f.asset1,
        &f.instructor,
        &333u32,
        &true,
    );

    // Top up the student so the transfer succeeds.
    StellarAssetClient::new(&f.env, &f.asset1).mint(&f.student, &big);
    f.client()
        .pay_for_course(&f.student, &big_course, &f.payment_id("BIGP"));

    let rec = f
        .client()
        .get_payment_record(&f.student, &big_course)
        .unwrap();
    assert_eq!(rec.amount, big);
    assert_eq!(rec.fee_amount, 133_200_000_000_000_000i128); // floor(4e18×333/1e4)
    assert_eq!(rec.fee_amount + rec.instructor_amount, big);
}

// ─── Payment-ID lookup & custody invariant ───────────────────────────────────

#[test]
fn test_get_payment_by_id_returns_receipt() {
    let f = Fixture::new();
    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("LOOK"));

    let by_key = f
        .client()
        .get_payment_record(&f.student, &f.course_id())
        .unwrap();
    let by_id = f.client().get_payment_by_id(&f.payment_id("LOOK")).unwrap();
    assert_eq!(by_key, by_id);

    let unknown = f.payment_id("MISSING");
    assert!(f.client().get_payment_by_id(&unknown).is_none());
}

#[test]
fn test_escrow_holds_at_least_sum_of_instructor_balances() {
    let f = Fixture::new();

    // Two students × two courses on two assets.
    let web3 = Symbol::new(&f.env, "WEB3");
    f.client().add_course(
        &f.admin,
        &web3,
        &2_000_000i128,
        &f.asset2,
        &f.instructor,
        &250u32,
        &true,
    );
    f.client()
        .pay_for_course(&f.student, &f.course_id(), &f.payment_id("A1"));
    f.client()
        .pay_for_course(&f.student2, &f.course_id(), &f.payment_id("A2"));
    f.client()
        .pay_for_course(&f.student, &web3, &f.payment_id("A3"));

    // RUST101: 990_000 × 2 ; WEB3: 2_000_000 − 50_000 fee = 1_950_000.
    let instructor_total = f.client().get_instructor_balance(&f.instructor);
    assert_eq!(instructor_total, 3_930_000i128);

    // Custody invariant: tokens held in each asset ≥ the claimable share
    // attributable to that asset (the difference is the retained platform fee).
    let net_asset1 = 990_000i128 * 2;
    let net_asset2 = 1_950_000i128;
    assert_eq!(f.escrow_balance(&f.asset1), 2_000_000i128);
    assert!(f.escrow_balance(&f.asset1) >= net_asset1);
    assert_eq!(f.escrow_balance(&f.asset2), 2_000_000i128);
    assert!(f.escrow_balance(&f.asset2) >= net_asset2);
    assert_eq!(
        f.escrow_balance(&f.asset1) + f.escrow_balance(&f.asset2),
        instructor_total + 20_000i128 + 50_000i128
    );
}
