#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String, Symbol,
};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> Address {
    let admin = Address::generate(env);
    let network_id = BytesN::from_array(env, &[1u8; 32]);
    ScholarshipFinanceContract::initialize(env.clone(), admin.clone(), network_id).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

fn create_sponsor_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[3u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);
    let network_id = BytesN::from_array(&env, &[1u8; 32]);

    assert!(ScholarshipFinanceContract::initialize(env.clone(), admin.clone(), network_id).is_ok());
    assert_eq!(
        ScholarshipFinanceContract::initialize(env, admin, network_id),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_record_deposit_unrestricted() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let sponsor = Address::generate(&env);
    let asset = BytesN::from_array(&env, &[4u8; 32]);

    let deposit = ScholarshipFinanceContract::record_deposit(
        env.clone(),
        sponsor.clone(),
        None,
        asset.clone(),
        1000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    assert_eq!(deposit.amount, 1000);
    assert_eq!(deposit.asset, asset);
    assert!(deposit.program_id.is_none());
}

#[test]
fn test_record_deposit_program_specific() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let sponsor = Address::generate(&env);
    let program_id = create_program_id(&env);
    let asset = BytesN::from_array(&env, &[4u8; 32]);

    let deposit = ScholarshipFinanceContract::record_deposit(
        env.clone(),
        sponsor.clone(),
        Some(program_id.clone()),
        asset.clone(),
        5000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    assert_eq!(deposit.amount, 5000);
    assert_eq!(deposit.program_id, Some(program_id));
}

#[test]
fn test_create_funding_round() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let round = ScholarshipFinanceContract::create_funding_round(
        env.clone(),
        admin.clone(),
        String::from_str(&env, "Fall 2024 Scholarship Fund"),
        String::from_str(&env, "Funding for fall scholarships"),
        10000,
        BytesN::from_array(&env, &[4u8; 32]),
        env.ledger().timestamp() + 86400,
        env.ledger().timestamp() + 86400 * 30,
    ).unwrap();

    assert_eq!(round.target_amount, 10000);
    assert!(round.is_active);
}

#[test]
fn test_record_award() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let asset = BytesN::from_array(&env, &[4u8; 32]);
    let reference_id = BytesN::from_array(&env, &[6u8; 32]);

    // First deposit some funds
    ScholarshipFinanceContract::record_deposit(
        env.clone(),
        Address::generate(&env),
        Some(program_id.clone()),
        asset.clone(),
        10000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    let result = ScholarshipFinanceContract::record_award(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        asset.clone(),
        1000,
        reference_id.clone(),
    );

    assert!(result.is_ok());

    let ledger = ScholarshipFinanceContract::get_program_ledger(env, program_id).unwrap();
    assert_eq!(ledger.total_awards, 1000);
    assert_eq!(ledger.reserved_balance, 1000);
}

#[test]
fn test_record_disbursement() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let asset = BytesN::from_array(&env, &[4u8; 32]);
    let reference_id = BytesN::from_array(&env, &[6u8; 32]);

    // First deposit and award
    ScholarshipFinanceContract::record_deposit(
        env.clone(),
        Address::generate(&env),
        Some(program_id.clone()),
        asset.clone(),
        10000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    ScholarshipFinanceContract::record_award(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        asset.clone(),
        1000,
        BytesN::from_array(&env, &[6u8; 32]),
    ).unwrap();

    let result = ScholarshipFinanceContract::record_disbursement(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        asset.clone(),
        1000,
        reference_id.clone(),
    );

    assert!(result.is_ok());

    let ledger = ScholarshipFinanceContract::get_program_ledger(env, program_id).unwrap();
    assert_eq!(ledger.total_disbursements, 1000);
    assert_eq!(ledger.reserved_balance, 0);
    assert_eq!(ledger.current_balance, 9000);
}

#[test]
fn test_reconcile_treasury_balanced() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let asset = BytesN::from_array(&env, &[4u8; 32]);

    ScholarshipFinanceContract::record_deposit(
        env.clone(),
        Address::generate(&env),
        Some(program_id.clone()),
        asset.clone(),
        10000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    let reconciliation = ScholarshipFinanceContract::reconcile_treasury(
        env.clone(),
        admin.clone(),
        program_id.clone(),
    ).unwrap();

    assert_eq!(reconciliation.status, ReconciliationStatus::Balanced);
    assert_eq!(reconciliation.drift_amount, 0);
}

#[test]
fn test_create_receipt() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[7u8; 32]);

    // Setup: deposit, award, disbursement, execute intent
    let asset = BytesN::from_array(&env, &[4u8; 32]);
    ScholarshipFinanceContract::record_deposit(
        env.clone(),
        Address::generate(&env),
        Some(program_id.clone()),
        asset.clone(),
        10000,
        BytesN::from_array(&env, &[5u8; 32]),
        None,
    ).unwrap();

    ScholarshipFinanceContract::record_award(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        asset.clone(),
        1000,
        BytesN::from_array(&env, &[6u8; 32]),
    ).unwrap();

    ScholarshipFinanceContract::record_disbursement(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        asset.clone(),
        1000,
        BytesN::from_array(&env, &[6u8; 32]),
    ).unwrap();

    // Create receipt
    let receipt = ScholarshipFinanceContract::create_receipt(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        BytesN::from_array(&env, &[8u8; 32]),
        String::from_str(&env, "testnet"),
    ).unwrap();

    assert_eq!(receipt.intent_id, intent_id);
    assert_eq!(receipt.amount, 1000);

    // Verify receipt
    let verified = ScholarshipFinanceContract::verify_receipt(env, receipt.id).unwrap();
    assert!(verified);
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipFinanceContract::version(env), 1);
}