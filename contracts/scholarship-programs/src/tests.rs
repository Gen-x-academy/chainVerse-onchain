#![cfg(test)]
use crate::{ContractError, ScholarshipProgramsContract};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipProgramsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn program_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// ── #1062 — application windows ─────────────────────────────────────────────

#[test]
fn test_set_program_window_rejects_inverted_range() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let result = client.try_set_program_window(&admin, &pid, &1000u64, &500u64, &0i32, &0u64);
    assert_eq!(result, Err(Ok(ContractError::InvalidWindow)));
}

#[test]
fn test_non_admin_cannot_set_window() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    let attacker = Address::generate(&env);

    let result =
        client.try_set_program_window(&attacker, &pid, &0u64, &1000u64, &0i32, &0u64);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_is_open_true_within_window() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &0u64);

    env.ledger().set_timestamp(150);
    assert!(client.is_open(&pid));
}

#[test]
fn test_is_open_false_before_opening() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &0u64);

    env.ledger().set_timestamp(50);
    assert!(!client.is_open(&pid));
}

#[test]
fn test_is_open_false_after_closing() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &0u64);

    env.ledger().set_timestamp(201);
    assert!(!client.is_open(&pid));
}

#[test]
fn test_is_within_grace_true_during_grace_period() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &50u64);

    env.ledger().set_timestamp(220); // after close, within +50 grace
    assert!(!client.is_open(&pid));
    assert!(client.is_within_grace(&pid));
}

#[test]
fn test_is_within_grace_false_after_grace_expires() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &50u64);

    env.ledger().set_timestamp(300); // well past close + grace
    assert!(!client.is_within_grace(&pid));
}

#[test]
fn test_is_within_grace_false_during_open_window() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &50u64);

    env.ledger().set_timestamp(150);
    assert!(!client.is_within_grace(&pid));
}

// Changing the window must not touch anything else — there is no
// application state in this contract for it to invalidate.
#[test]
fn test_updating_window_replaces_prior_config() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.set_program_window(&admin, &pid, &100u64, &200u64, &0i32, &0u64);
    client.set_program_window(&admin, &pid, &300u64, &400u64, &0i32, &0u64);

    let window = client.get_program_window(&pid);
    assert_eq!(window.opens_at, 300);
    assert_eq!(window.closes_at, 400);
}

// ── #1064 — award inventory / budget ceilings ────────────────────────────────

#[test]
fn test_configure_budget_rejects_insufficient_total_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    // 10 recipients * 100 = 1000 required, but only 500 budgeted.
    let result = client.try_configure_award_budget(&admin, &pid, &10u32, &100i128, &500i128);
    assert_eq!(result, Err(Ok(ContractError::InvalidBudgetConfig)));
}

#[test]
fn test_configure_budget_rejects_zero_max_recipients() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let result = client.try_configure_award_budget(&admin, &pid, &0u32, &100i128, &1000i128);
    assert_eq!(result, Err(Ok(ContractError::InvalidBudgetConfig)));
}

#[test]
fn test_reserve_award_increments_counters() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.configure_award_budget(&admin, &pid, &3u32, &100i128, &300i128);

    let idx = client.reserve_award(&admin, &pid);
    assert_eq!(idx, 1);

    let budget = client.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 1);
    assert_eq!(budget.committed_amount, 100);
    assert_eq!(client.remaining_capacity(&pid), 2);
}

#[test]
fn test_reserve_award_cannot_exceed_max_recipients() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.configure_award_budget(&admin, &pid, &2u32, &100i128, &1000i128);

    client.reserve_award(&admin, &pid);
    client.reserve_award(&admin, &pid);

    let result = client.try_reserve_award(&admin, &pid);
    assert_eq!(result, Err(Ok(ContractError::CapacityExceeded)));
    assert_eq!(client.remaining_capacity(&pid), 0);
}

#[test]
fn test_reserve_award_cannot_exceed_total_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    // max_recipients is generously large, but total_budget only covers 2 awards.
    client.configure_award_budget(&admin, &pid, &100u32, &100i128, &200i128);

    client.reserve_award(&admin, &pid);
    client.reserve_award(&admin, &pid);

    let result = client.try_reserve_award(&admin, &pid);
    assert_eq!(result, Err(Ok(ContractError::BudgetExceeded)));
}

#[test]
fn test_release_award_returns_capacity_and_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.configure_award_budget(&admin, &pid, &2u32, &100i128, &200i128);

    client.reserve_award(&admin, &pid);
    client.reserve_award(&admin, &pid);
    assert_eq!(client.remaining_capacity(&pid), 0);

    client.release_award(&admin, &pid);
    let budget = client.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 1);
    assert_eq!(budget.committed_amount, 100);
    assert_eq!(client.remaining_capacity(&pid), 1);

    // Released capacity can be reserved again.
    client.reserve_award(&admin, &pid);
    assert_eq!(client.remaining_capacity(&pid), 0);
}

#[test]
fn test_release_award_with_none_reserved_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.configure_award_budget(&admin, &pid, &2u32, &100i128, &200i128);

    let result = client.try_release_award(&admin, &pid);
    assert_eq!(result, Err(Ok(ContractError::NoAwardsReserved)));
}

#[test]
fn test_non_admin_cannot_reserve_award() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);
    client.configure_award_budget(&admin, &pid, &2u32, &100i128, &200i128);
    let attacker = Address::generate(&env);

    let result = client.try_reserve_award(&attacker, &pid);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn test_remaining_capacity_requires_configured_budget() {
    let (env, contract_id, admin) = setup();
    let client = crate::ScholarshipProgramsContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let pid = program_id(&env, 1);

    let result = client.try_remaining_capacity(&pid);
    assert_eq!(result, Err(Ok(ContractError::BudgetNotFound)));
}
