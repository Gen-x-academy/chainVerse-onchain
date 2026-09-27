#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, Symbol, String,
};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

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