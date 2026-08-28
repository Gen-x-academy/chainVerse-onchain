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
