#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String, Symbol,
    Address, BytesN, Env, Symbol, String,
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
fn setup_contract(env: &Env) -> (Address, BytesN<32>) {
    let admin = Address::generate(env);
    let network_id = BytesN::from_array(env, &[1u8; 32]);
    ScholarshipFinanceContract::initialize(env.clone(), admin.clone(), network_id).unwrap();
    (admin, network_id)
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
fn test_configure_fees_percentage() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    let config = ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::Percentage(250), // 2.5%
        1000, // network fee estimate
        Symbol::new(&env, "XLM"),
    ).unwrap();

    assert_eq!(config.platform_fee, FeeBasis::Percentage(250));
    assert_eq!(config.network_fee_estimate, 1000);
    assert_eq!(config.version, 1);
}

#[test]
fn test_configure_fees_fixed() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    let config = ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::Fixed(500),
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    assert_eq!(config.platform_fee, FeeBasis::Fixed(500));
}

#[test]
fn test_configure_fees_percentage_with_cap() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    let config = ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::PercentageWithCap { basis_points: 500, cap: 10000 },
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    assert_eq!(config.platform_fee, FeeBasis::PercentageWithCap { basis_points: 500, cap: 10000 });
}

#[test]
fn test_fee_config_validation_rejects_invalid_percentage() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    assert_eq!(
        ScholarshipFinanceContract::configure_fees(
            env,
            admin,
            program_id,
            FeeBasis::Percentage(15000), // > 100%
            1000,
            Symbol::new(&env, "XLM"),
        ),
        Err(ContractError::InvalidFeeConfig)
    );
}

#[test]
fn test_calculate_fees_percentage() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::Percentage(250), // 2.5%
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    let (platform, network, total, net) = ScholarshipFinanceContract::calculate_fees(
        env,
        program_id,
        10000, // gross amount
    ).unwrap();

    assert_eq!(platform, 250); // 2.5% of 10000
    assert_eq!(network, 1000);
    assert_eq!(total, 1250);
    assert_eq!(net, 8750);
}

#[test]
fn test_calculate_fees_fixed() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::Fixed(500),
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    let (platform, network, total, net) = ScholarshipFinanceContract::calculate_fees(
        env,
        program_id,
        10000,
    ).unwrap();

    assert_eq!(platform, 500);
    assert_eq!(network, 1000);
    assert_eq!(total, 1500);
    assert_eq!(net, 8500);
}

#[test]
fn test_calculate_fees_percentage_with_cap() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::PercentageWithCap { basis_points: 1000, cap: 500 }, // 10% capped at 500
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    let (platform, _, _, _) = ScholarshipFinanceContract::calculate_fees(
        env,
        program_id,
        10000, // 10% would be 1000, but cap is 500
    ).unwrap();

    assert_eq!(platform, 500);
}

#[test]
fn test_calculate_fees_rejects_negative_net() {
    let env = create_test_env();
    let (admin, _) = setup_contract(&env);
    let program_id = create_program_id(&env);

    ScholarshipFinanceContract::configure_fees(
        env.clone(),
        admin,
        program_id.clone(),
        FeeBasis::Fixed(20000), // fee > amount
        1000,
        Symbol::new(&env, "XLM"),
    ).unwrap();

    assert_eq!(
        ScholarshipFinanceContract::calculate_fees(env, program_id, 10000),
        Err(ContractError::InvalidFeeConfig)
    );
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipFinanceContract::version(env), 1);
}