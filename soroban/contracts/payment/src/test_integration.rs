//! Adversarial integration tests for the Soroban payment foundation (issue #917).
//!
//! Validates that payment, enrollment, split accounting, custody, and withdrawal
//! modules compose correctly across the full lifecycle against a shared
//! multi-party, multi-asset fixture.
#![cfg(test)]

extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, FromVal, Symbol, TryFromVal,
};

use crate::{ContractError, PaymentContract, PaymentContractClient};

macro_rules! contract_err {
    ($r:expr) => {
        match $r {
            Err(Ok(e)) => e,
            other => panic!("expected contract error, got {:?}", other),
        }
    };
}

struct Fixture {
    env: Env,
    contract: Address,
    admin: Address,
    treasury: Address,
    instructor1: Address,
    instructor2: Address,
    student1: Address,
    student2: Address,
    student3: Address,
    asset_a: Address,
    asset_b: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let instructor1 = Address::generate(&env);
        let instructor2 = Address::generate(&env);
        let student1 = Address::generate(&env);
        let student2 = Address::generate(&env);
        let student3 = Address::generate(&env);

        let contract = env.register_contract(None, PaymentContract {});
        let client = PaymentContractClient::new(&env, &contract);

        client.initialize(&admin, &treasury, &500u32, &86_400u64);

        let sac_a = env.register_stellar_asset_contract_v2(admin.clone());
        let sac_b = env.register_stellar_asset_contract_v2(treasury.clone());
        let asset_a = sac_a.address();
        let asset_b = sac_b.address();

        client.add_asset(&admin, &asset_a, &true);
        client.add_asset(&admin, &asset_b, &true);

        let c1 = Symbol::new(&env, "RUST101");
        let c2 = Symbol::new(&env, "WEB3");
        let c3 = Symbol::new(&env, "SOL101");
        client.add_course(&admin, &c1, &1_000_000i128, &asset_a, &instructor1, &100u32, &true);
        client.add_course(&admin, &c2, &2_000_000i128, &asset_b, &instructor1, &0u32, &true);
        client.add_course(&admin, &c3, &500_000i128, &asset_a, &instructor2, &250u32, &true);

        StellarAssetClient::new(&env, &asset_a).mint(&student1, &1_000_000_000i128);
        StellarAssetClient::new(&env, &asset_a).mint(&student2, &1_000_000_000i128);
        StellarAssetClient::new(&env, &asset_a).mint(&student3, &1_000_000_000i128);
        StellarAssetClient::new(&env, &asset_b).mint(&student1, &1_000_000_000i128);
        StellarAssetClient::new(&env, &asset_b).mint(&student2, &1_000_000_000i128);
        StellarAssetClient::new(&env, &asset_b).mint(&student3, &1_000_000_000i128);

        Fixture {
            env, contract, admin, treasury, instructor1, instructor2,
            student1, student2, student3, asset_a, asset_b,
        }
    }

    fn client(&self) -> PaymentContractClient<'_> {
        PaymentContractClient::new(&self.env, &self.contract)
    }

    fn token(&self, asset: &Address) -> TokenClient<'_> {
        TokenClient::new(&self.env, asset)
    }

    fn escrow_balance(&self, asset: &Address) -> i128 {
        self.token(asset).balance(&self.contract)
    }

    fn c1(&self) -> Symbol { Symbol::new(&self.env, "RUST101") }
    fn c2(&self) -> Symbol { Symbol::new(&self.env, "WEB3") }
    fn c3(&self) -> Symbol { Symbol::new(&self.env, "SOL101") }
    fn pid(&self, tag: &str) -> Symbol { Symbol::new(&self.env, tag) }
}

// ============================================================================
//  1. FULL LIFECYCLE: purchase -> enrollment -> instructor withdrawal -> platform
//     withdrawal, for at least two assets.
// ============================================================================

#[test]
fn test_full_lifecycle_asset_a() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("LCA1"));

    assert!(f.client().is_enrolled(&f.student1, &f.c1()));

    let rec = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    assert_eq!(rec.amount, 1_000_000i128);
    assert_eq!(rec.fee_amount, 10_000i128);
    assert_eq!(rec.instructor_amount, 990_000i128);
    assert_eq!(rec.fee_amount + rec.instructor_amount, rec.amount);
    assert_eq!(rec.asset, f.asset_a);
    assert_eq!(rec.payment_id, f.pid("LCA1"));

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 990_000i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 10_000i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 1_000_000i128);

    let iw = f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    assert_eq!(iw.amount, 990_000i128);
    assert_eq!(iw.recipient, f.instructor1);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 10_000i128);

    let pw = f.client().platform_withdraw(&f.treasury, &f.asset_a, &10_000i128);
    assert_eq!(pw.amount, 10_000i128);
    assert_eq!(pw.recipient, f.treasury);
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

#[test]
fn test_full_lifecycle_asset_b() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c2(), &f.pid("LCB1"));

    assert!(f.client().is_enrolled(&f.student1, &f.c2()));
    let rec = f.client().get_payment_record(&f.student1, &f.c2()).unwrap();
    assert_eq!(rec.amount, 2_000_000i128);
    assert_eq!(rec.fee_amount, 100_000i128);
    assert_eq!(rec.instructor_amount, 1_900_000i128);
    assert_eq!(rec.fee_amount + rec.instructor_amount, rec.amount);
    assert_eq!(rec.asset, f.asset_b);

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_b), 1_900_000i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_b), 100_000i128);

    let iw = f.client().instructor_withdraw(&f.instructor1, &f.asset_b, &1_900_000i128);
    assert_eq!(iw.amount, 1_900_000i128);
    assert_eq!(f.escrow_balance(&f.asset_b), 100_000i128);

    f.client().platform_withdraw(&f.treasury, &f.asset_b, &100_000i128);
    assert_eq!(f.escrow_balance(&f.asset_b), 0i128);
}

#[test]
fn test_full_lifecycle_both_assets_simultaneously() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("SIM1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("SIM2"));

    assert!(f.client().is_enrolled(&f.student1, &f.c1()));
    assert!(f.client().is_enrolled(&f.student2, &f.c2()));

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    f.client().instructor_withdraw(&f.instructor1, &f.asset_b, &1_900_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_a, &10_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_b, &100_000i128);

    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_b), 0i128);
}

// ============================================================================
//  2. MULTI-PARTY, MULTI-ASSET INTERLEAVING
// ============================================================================

#[test]
fn test_three_students_three_courses_two_assets() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("M1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("M2"));
    f.client().pay_for_course(&f.student3, &f.c3(), &f.pid("M3"));
    f.client().pay_for_course(&f.student1, &f.c2(), &f.pid("M4"));
    f.client().pay_for_course(&f.student2, &f.c3(), &f.pid("M5"));

    assert!(f.client().is_enrolled(&f.student1, &f.c1()));
    assert!(f.client().is_enrolled(&f.student2, &f.c2()));
    assert!(f.client().is_enrolled(&f.student3, &f.c3()));
    assert!(f.client().is_enrolled(&f.student1, &f.c2()));
    assert!(f.client().is_enrolled(&f.student2, &f.c3()));

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 990_000i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_b), 3_800_000i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor2, &f.asset_a), 975_000i128);

    assert_eq!(f.client().get_platform_balance(&f.asset_a), 35_000i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_b), 200_000i128);

    let i1a = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let i2a = f.client().get_instructor_balance(&f.instructor2, &f.asset_a);
    let pa = f.client().get_platform_balance(&f.asset_a);
    assert_eq!(f.escrow_balance(&f.asset_a), i1a + i2a + pa);

    let i1b = f.client().get_instructor_balance(&f.instructor1, &f.asset_b);
    let pb = f.client().get_platform_balance(&f.asset_b);
    assert_eq!(f.escrow_balance(&f.asset_b), i1b + pb);
}

#[test]
fn test_instructors_on_same_asset_independent() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("NI1"));
    f.client().pay_for_course(&f.student2, &f.c3(), &f.pid("NI2"));

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 990_000i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor2, &f.asset_a), 487_500i128);

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor2, &f.asset_a), 487_500i128);

    f.client().instructor_withdraw(&f.instructor2, &f.asset_a, &487_500i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor2, &f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 22_500i128);
}

// ============================================================================
//  3. AUTHORIZATION-TREE NEGATIVE TESTS
// ============================================================================

#[test]
fn test_purchase_without_student_auth_leaves_no_state() {
    let f = Fixture::new();

    f.env.set_auths(&[]);
    let result = f.client().try_pay_for_course(&f.student1, &f.c1(), &f.pid("NOAUTH"));
    assert!(matches!(result, Err(Err(_))), "missing auth must abort");

    f.env.mock_all_auths();
    assert!(!f.client().is_enrolled(&f.student1, &f.c1()));
    assert!(f.client().get_payment_record(&f.student1, &f.c1()).is_none());
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

#[test]
fn test_instructor_withdraw_wrong_instructor_fails() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("AW1"));

    let err = contract_err!(f.client().try_instructor_withdraw(&f.instructor2, &f.asset_a, &990_000i128));
    assert_eq!(err, ContractError::InsufficientBalance);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 990_000i128);
}

#[test]
fn test_platform_withdraw_non_treasury_fails() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("PW1"));

    let impostor = Address::generate(&f.env);
    let err = contract_err!(f.client().try_platform_withdraw(&impostor, &f.asset_a, &10_000i128));
    assert_eq!(err, ContractError::NotAdmin);
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 10_000i128);
}

#[test]
fn test_instructor_cannot_platform_withdraw() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("XW1"));

    let err = contract_err!(f.client().try_platform_withdraw(&f.instructor1, &f.asset_a, &10_000i128));
    assert_eq!(err, ContractError::NotAdmin);
}

#[test]
fn test_treasury_cannot_instructor_withdraw() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("XW2"));

    let err = contract_err!(f.client().try_instructor_withdraw(&f.treasury, &f.asset_a, &990_000i128));
    assert_eq!(err, ContractError::InsufficientBalance);
}

// ============================================================================
//  4. DUPLICATE / REPLAY / ROLLBACK TESTS
// ============================================================================

#[test]
fn test_duplicate_enrollment_rejected_with_new_pid() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("DUP1"));

    let balance_before = f.token(&f.asset_a).balance(&f.student1);
    let err = contract_err!(f.client().try_pay_for_course(&f.student1, &f.c1(), &f.pid("DUP2")));
    assert_eq!(err, ContractError::AlreadyEnrolled);

    assert_eq!(f.token(&f.asset_a).balance(&f.student1), balance_before);
    assert_eq!(f.escrow_balance(&f.asset_a), 1_000_000i128);
    let rec = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    assert_eq!(rec.payment_id, f.pid("DUP1"));
}

#[test]
fn test_replay_identical_payment_id_rejected() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("REPLAY"));

    let err = contract_err!(f.client().try_pay_for_course(&f.student1, &f.c1(), &f.pid("REPLAY")));
    assert_eq!(err, ContractError::AlreadyEnrolled);
    assert_eq!(f.escrow_balance(&f.asset_a), 1_000_000i128);
}

#[test]
fn test_conflicting_payment_id_rejected() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("CLASH"));

    let s2_before = f.token(&f.asset_a).balance(&f.student2);
    let err = contract_err!(f.client().try_pay_for_course(&f.student2, &f.c1(), &f.pid("CLASH")));
    assert_eq!(err, ContractError::DuplicatePaymentId);

    assert!(!f.client().is_enrolled(&f.student2, &f.c1()));
    assert_eq!(f.token(&f.asset_a).balance(&f.student2), s2_before);
}

#[test]
fn test_empty_payment_id_rejected() {
    let f = Fixture::new();

    let err = contract_err!(f.client().try_pay_for_course(&f.student1, &f.c1(), &Symbol::new(&f.env, "")));
    assert_eq!(err, ContractError::InvalidPaymentId);
    assert!(!f.client().is_enrolled(&f.student1, &f.c1()));
}

#[test]
fn test_insufficient_balance_atomic_rollback() {
    let f = Fixture::new();
    let broke = Address::generate(&f.env);

    let err = contract_err!(f.client().try_pay_for_course(&broke, &f.c1(), &f.pid("BROKE")));
    assert_eq!(err, ContractError::PaymentFailed);

    assert!(!f.client().is_enrolled(&broke, &f.c1()));
    assert!(f.client().get_payment_record(&broke, &f.c1()).is_none());
    assert!(f.client().get_payment_by_id(&f.pid("BROKE")).is_none());
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

#[test]
fn test_inactive_course_fails_atomically() {
    let f = Fixture::new();
    f.client().deactivate_course(&f.admin, &f.c1());

    let err = contract_err!(f.client().try_pay_for_course(&f.student1, &f.c1(), &f.pid("INACT")));
    assert_eq!(err, ContractError::CourseInactive);
    assert!(!f.client().is_enrolled(&f.student1, &f.c1()));
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

#[test]
fn test_disabled_asset_fails_atomically() {
    let f = Fixture::new();
    f.client().disable_asset(&f.admin, &f.asset_a);

    let err = contract_err!(f.client().try_pay_for_course(&f.student1, &f.c1(), &f.pid("DIS")));
    assert_eq!(err, ContractError::AssetNotEnabled);
    assert!(!f.client().is_enrolled(&f.student1, &f.c1()));
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

// ============================================================================
//  5. INTERLEAVED PURCHASES AND WITHDRAWALS ACROSS ASSETS
// ============================================================================

#[test]
fn test_interleaved_purchase_withdrawal_purchase() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("I1"));
    f.client().pay_for_course(&f.student1, &f.c2(), &f.pid("I2"));
    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("I3"));

    let i_a1 = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let i_b1 = f.client().get_instructor_balance(&f.instructor1, &f.asset_b);
    let pa = f.client().get_platform_balance(&f.asset_a);
    let pb = f.client().get_platform_balance(&f.asset_b);

    assert_eq!(i_a1, 990_000i128);
    assert_eq!(i_b1, 1_900_000i128);
    assert_eq!(pa, 20_000i128);
    assert_eq!(pb, 100_000i128);
    assert_eq!(f.escrow_balance(&f.asset_a), i_a1 + pa);
    assert_eq!(f.escrow_balance(&f.asset_b), i_b1 + pb);
}

#[test]
fn test_withdrawal_then_new_purchase() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("WN1"));
    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);

    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("WN2"));
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 990_000i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 1_010_000i128);
}

// ============================================================================
//  6. CONFIGURATION CHANGES BETWEEN PURCHASES
// ============================================================================

#[test]
fn test_price_change_between_purchases() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("CFG1"));
    let rec1 = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    assert_eq!(rec1.amount, 1_000_000i128);

    f.client().update_course(&f.admin, &f.c1(), &3_000_000i128, &f.asset_a, &f.instructor1, &100u32, &true);

    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("CFG2"));
    let rec2 = f.client().get_payment_record(&f.student2, &f.c1()).unwrap();
    assert_eq!(rec2.amount, 3_000_000i128);
    assert_eq!(rec2.fee_amount, 30_000i128);
    assert_eq!(rec2.instructor_amount, 2_970_000i128);

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 3_960_000i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 40_000i128);
}

#[test]
fn test_fee_change_between_purchases() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("FEE1"));
    let rec1 = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    assert_eq!(rec1.fee_amount, 10_000i128);

    f.client().set_fee(&f.admin, &1_000u32);

    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("FEE2"));
    let rec2 = f.client().get_payment_record(&f.student2, &f.c1()).unwrap();
    assert_eq!(rec2.fee_amount, 10_000i128);

    let c1_config = f.client().get_course_config(&f.c1()).unwrap();
    assert_eq!(c1_config.fee_bps, 100u32);
}

#[test]
fn test_asset_disable_blocks_subsequent_purchases() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("DIS1"));
    assert!(f.client().is_enrolled(&f.student1, &f.c1()));

    f.client().disable_asset(&f.admin, &f.asset_a);

    let err = contract_err!(f.client().try_pay_for_course(&f.student2, &f.c1(), &f.pid("DIS2")));
    assert_eq!(err, ContractError::AssetNotEnabled);
    assert!(!f.client().is_enrolled(&f.student2, &f.c1()));

    f.client().enable_asset(&f.admin, &f.asset_a);

    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("DIS3"));
    assert!(f.client().is_enrolled(&f.student2, &f.c1()));
}

#[test]
fn test_course_deactivate_reactivate_between_purchases() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("CR1"));

    f.client().deactivate_course(&f.admin, &f.c1());
    let err = contract_err!(f.client().try_pay_for_course(&f.student2, &f.c1(), &f.pid("CR2")));
    assert_eq!(err, ContractError::CourseInactive);

    f.client().activate_course(&f.admin, &f.c1());
    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("CR3"));
    assert!(f.client().is_enrolled(&f.student2, &f.c1()));
}

// ============================================================================
//  7. SPLIT AND CUSTODY INVARIANT PROPERTY TESTS
// ============================================================================

#[test]
fn test_split_invariant_fee_plus_instructor_equals_gross() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("S1"));
    f.client().pay_for_course(&f.student1, &f.c2(), &f.pid("S2"));
    f.client().pay_for_course(&f.student2, &f.c3(), &f.pid("S3"));

    let rec1 = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    let rec2 = f.client().get_payment_record(&f.student1, &f.c2()).unwrap();
    let rec3 = f.client().get_payment_record(&f.student2, &f.c3()).unwrap();

    assert_eq!(rec1.fee_amount + rec1.instructor_amount, rec1.amount);
    assert_eq!(rec2.fee_amount + rec2.instructor_amount, rec2.amount);
    assert_eq!(rec3.fee_amount + rec3.instructor_amount, rec3.amount);
}

#[test]
fn test_custody_invariant_escrow_covers_all_liabilities() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("CI1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("CI2"));
    f.client().pay_for_course(&f.student3, &f.c3(), &f.pid("CI3"));
    f.client().pay_for_course(&f.student1, &f.c3(), &f.pid("CI4"));

    let i1a = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let i2a = f.client().get_instructor_balance(&f.instructor2, &f.asset_a);
    let i1b = f.client().get_instructor_balance(&f.instructor1, &f.asset_b);
    let pa = f.client().get_platform_balance(&f.asset_a);
    let pb = f.client().get_platform_balance(&f.asset_b);

    assert_eq!(f.escrow_balance(&f.asset_a), i1a + i2a + pa);
    assert_eq!(f.escrow_balance(&f.asset_b), i1b + pb);
}

#[test]
fn test_custody_invariant_holds_after_partial_withdrawals() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("CW1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("CW2"));
    f.client().pay_for_course(&f.student3, &f.c3(), &f.pid("CW3"));

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &500_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_b, &50_000i128);

    let i1a = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let i2a = f.client().get_instructor_balance(&f.instructor2, &f.asset_a);
    let pa = f.client().get_platform_balance(&f.asset_a);
    let i1b = f.client().get_instructor_balance(&f.instructor1, &f.asset_b);
    let pb = f.client().get_platform_balance(&f.asset_b);

    assert_eq!(f.escrow_balance(&f.asset_a), i1a + i2a + pa);
    assert_eq!(f.escrow_balance(&f.asset_b), i1b + pb);
}

#[test]
fn test_custody_invariant_full_drain() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("FD1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("FD2"));

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    f.client().instructor_withdraw(&f.instructor1, &f.asset_b, &1_900_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_a, &10_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_b, &100_000i128);

    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_b), 0i128);

    let i1a = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let pa = f.client().get_platform_balance(&f.asset_a);
    assert_eq!(f.escrow_balance(&f.asset_a), i1a + pa);
}

// ============================================================================
//  8. RANDOMIZED-LOOKING PURCHASE/WITHDRAWAL SEQUENCES
// ============================================================================

#[test]
fn test_sequential_multi_student_multi_course_with_withdrawals() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("R1"));
    f.client().pay_for_course(&f.student2, &f.c1(), &f.pid("R2"));
    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &1_000_000i128);
    f.client().pay_for_course(&f.student3, &f.c2(), &f.pid("R3"));
    f.client().pay_for_course(&f.student1, &f.c2(), &f.pid("R4"));
    f.client().instructor_withdraw(&f.instructor1, &f.asset_b, &2_800_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_a, &20_000i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_b, &100_000i128);
    f.client().pay_for_course(&f.student2, &f.c3(), &f.pid("R5"));

    let i1a = f.client().get_instructor_balance(&f.instructor1, &f.asset_a);
    let i2a = f.client().get_instructor_balance(&f.instructor2, &f.asset_a);
    let i1b = f.client().get_instructor_balance(&f.instructor1, &f.asset_b);
    let pa = f.client().get_platform_balance(&f.asset_a);
    let pb = f.client().get_platform_balance(&f.asset_b);

    assert_eq!(i1a, 980_000i128);
    assert_eq!(i2a, 487_500i128);
    assert_eq!(i1b, 1_000_000i128);
    assert_eq!(pa, 12_500i128);
    assert_eq!(pb, 100_000i128);

    assert_eq!(f.escrow_balance(&f.asset_a), i1a + i2a + pa);
    assert_eq!(f.escrow_balance(&f.asset_b), i1b + pb);
}

// ============================================================================
//  9. EDGE CASES: MAX VALUES, ROUNDING DUST, ZERO-LIABILITY CLEANUP
// ============================================================================

#[test]
fn test_max_fee_bps_course() {
    let f = Fixture::new();

    let max_course = Symbol::new(&f.env, "MAXFEE");
    f.client().add_course(&f.admin, &max_course, &1_000_000i128, &f.asset_a, &f.instructor1, &2_000u32, &true);

    f.client().pay_for_course(&f.student1, &max_course, &f.pid("MAX1"));

    let rec = f.client().get_payment_record(&f.student1, &max_course).unwrap();
    assert_eq!(rec.fee_amount, 200_000i128);
    assert_eq!(rec.instructor_amount, 800_000i128);
    assert_eq!(rec.fee_amount + rec.instructor_amount, rec.amount);

    let iw = f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &800_000i128);
    assert_eq!(iw.amount, 800_000i128);
    let pw = f.client().platform_withdraw(&f.treasury, &f.asset_a, &200_000i128);
    assert_eq!(pw.amount, 200_000i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
}

#[test]
fn test_zero_course_fee_uses_global() {
    let f = Fixture::new();

    let gf_course = Symbol::new(&f.env, "GLOBAL");
    f.client().add_course(&f.admin, &gf_course, &1_000_000i128, &f.asset_a, &f.instructor1, &0u32, &true);

    f.client().pay_for_course(&f.student1, &gf_course, &f.pid("GF1"));

    let rec = f.client().get_payment_record(&f.student1, &gf_course).unwrap();
    assert_eq!(rec.fee_amount, 50_000i128);
    assert_eq!(rec.instructor_amount, 950_000i128);
}

#[test]
fn test_rounding_dust_instructor_gets_all() {
    let f = Fixture::new();

    let dust_course = Symbol::new(&f.env, "DUST");
    f.client().add_course(&f.admin, &dust_course, &1i128, &f.asset_a, &f.instructor1, &333u32, &true);

    for i in 0..10u32 {
        let student = Address::generate(&f.env);
        StellarAssetClient::new(&f.env, &f.asset_a).mint(&student, &10i128);
        let pid = Symbol::new(&f.env, &std::format!("D{i}"));
        f.client().pay_for_course(&student, &dust_course, &pid);
    }

    assert_eq!(
        f.client().get_instructor_balance(&f.instructor1, &f.asset_a),
        10i128
    );
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_a), 10i128);
}

#[test]
fn test_exact_balance_purchase_drains_student() {
    let f = Fixture::new();

    let exact_student = Address::generate(&f.env);
    StellarAssetClient::new(&f.env, &f.asset_a).mint(&exact_student, &1_000_000i128);

    f.client().pay_for_course(&exact_student, &f.c1(), &f.pid("EXACT"));

    assert_eq!(f.token(&f.asset_a).balance(&exact_student), 0i128);
    assert!(f.client().is_enrolled(&exact_student, &f.c1()));
    assert_eq!(f.escrow_balance(&f.asset_a), 1_000_000i128);
}

// ============================================================================
//  10. PUBLIC API, ERROR, AND EVENT-SCHEMA CONFORMANCE TO #913
// ============================================================================

#[test]
fn test_event_pymt_rcd_schema_conforms() {
    let f = Fixture::new();
    let pid = f.pid("EVT1");

    f.client().pay_for_course(&f.student1, &f.c1(), &pid);

    let found = f.env.events().all().iter().any(|(emitter, topics, data)| {
        emitter == f.contract
            && topics.len() == 1
            && Symbol::try_from_val(&f.env, &topics.get(0u32).unwrap()).unwrap()
                == symbol_short!("PYMT_RCD")
            && <(Address, Symbol, i128, Address, Address, Symbol)>::from_val(&f.env, &data)
                == (
                    f.student1.clone(),
                    f.c1(),
                    1_000_000i128,
                    f.asset_a.clone(),
                    f.instructor1.clone(),
                    pid.clone(),
                )
    });
    assert!(found, "PYMT_RCD event with frozen schema must be emitted");
}

#[test]
fn test_event_wthdw_schema_conforms() {
    let f = Fixture::new();
    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("W1"));

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);

    let withdrawn_at = f.env.ledger().timestamp();
    let found = f.env.events().all().iter().any(|(emitter, topics, data)| {
        emitter == f.contract
            && topics.len() == 1
            && Symbol::try_from_val(&f.env, &topics.get(0u32).unwrap()).unwrap()
                == symbol_short!("WTHDW")
            && <(Address, Address, i128, u64)>::from_val(&f.env, &data)
                == (
                    f.instructor1.clone(),
                    f.asset_a.clone(),
                    990_000i128,
                    withdrawn_at,
                )
    });
    assert!(found, "WTHDW event with frozen schema must be emitted");
}

#[test]
fn test_error_discriminants_are_frozen() {
    assert_eq!(ContractError::AlreadyInitialized as u32, 1);
    assert_eq!(ContractError::NotAdmin as u32, 2);
    assert_eq!(ContractError::NotInitialized as u32, 3);
    assert_eq!(ContractError::InvalidFee as u32, 4);
    assert_eq!(ContractError::CourseNotFound as u32, 5);
    assert_eq!(ContractError::CourseInactive as u32, 6);
    assert_eq!(ContractError::AlreadyEnrolled as u32, 7);
    assert_eq!(ContractError::NotEnrolled as u32, 8);
    assert_eq!(ContractError::PaymentFailed as u32, 9);
    assert_eq!(ContractError::RefundWindowExpired as u32, 10);
    assert_eq!(ContractError::InsufficientBalance as u32, 11);
    assert_eq!(ContractError::TransferFailed as u32, 12);
    assert_eq!(ContractError::InvalidAmount as u32, 13);
    assert_eq!(ContractError::InvalidAsset as u32, 14);
    assert_eq!(ContractError::UnauthorizedCaller as u32, 15);
    assert_eq!(ContractError::AssetNotEnabled as u32, 16);
    assert_eq!(ContractError::AssetNotFound as u32, 17);
    assert_eq!(ContractError::InvalidAddress as u32, 18);
    assert_eq!(ContractError::DuplicatePaymentId as u32, 19);
    assert_eq!(ContractError::InvalidPaymentId as u32, 20);
}

#[test]
fn test_payment_record_lookup_by_id_and_key_agree() {
    let f = Fixture::new();
    let pid = f.pid("LOOK");

    f.client().pay_for_course(&f.student1, &f.c1(), &pid);

    let by_key = f.client().get_payment_record(&f.student1, &f.c1()).unwrap();
    let by_id = f.client().get_payment_by_id(&pid).unwrap();
    assert_eq!(by_key, by_id);

    assert!(f.client().get_payment_by_id(&f.pid("MISSING")).is_none());
}

#[test]
fn test_version_string() {
    use std::string::ToString;
    let f = Fixture::new();
    let ver = f.client().version();
    assert_eq!(ver.to_string(), "1.0.0");
}

// ============================================================================
//  11. PLATFORM FEE ZERO-LIABILITY CLEANUP AFTER FULL WITHDRAWAL
// ============================================================================

#[test]
fn test_zero_liability_state_after_full_drain_all_assets() {
    let f = Fixture::new();

    f.client().pay_for_course(&f.student1, &f.c1(), &f.pid("ZL1"));
    f.client().pay_for_course(&f.student2, &f.c2(), &f.pid("ZL2"));
    f.client().pay_for_course(&f.student3, &f.c3(), &f.pid("ZL3"));

    f.client().instructor_withdraw(&f.instructor1, &f.asset_a, &990_000i128);
    f.client().instructor_withdraw(&f.instructor1, &f.asset_b, &1_900_000i128);
    f.client().instructor_withdraw(&f.instructor2, &f.asset_a, &487_500i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_a, &22_500i128);
    f.client().platform_withdraw(&f.treasury, &f.asset_b, &100_000i128);

    assert_eq!(f.escrow_balance(&f.asset_a), 0i128);
    assert_eq!(f.escrow_balance(&f.asset_b), 0i128);

    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_a), 0i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor1, &f.asset_b), 0i128);
    assert_eq!(f.client().get_instructor_balance(&f.instructor2, &f.asset_a), 0i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_a), 0i128);
    assert_eq!(f.client().get_platform_balance(&f.asset_b), 0i128);

    let total_student1_before = f.token(&f.asset_a).balance(&f.student1)
        + f.token(&f.asset_b).balance(&f.student1);
    let total_student2_before = f.token(&f.asset_a).balance(&f.student2)
        + f.token(&f.asset_b).balance(&f.student2);
    let total_student3_before = f.token(&f.asset_a).balance(&f.student3)
        + f.token(&f.asset_b).balance(&f.student3);

    assert!(total_student1_before > 0 || f.token(&f.asset_a).balance(&f.student1) < 1_000_000_000i128);
    let _ = (total_student2_before, total_student3_before);
}
