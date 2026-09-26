#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Vec};

fn create_test_env() -> Env {
    let env = Env::default();
    env.mock_all_auths();
    env
}

fn setup_contract(env: &Env) -> Address {
    let admin = Address::generate(env);
    ScholarshipDisbursementEnhancementsContract::initialize(env.clone(), admin.clone()).unwrap();
    admin
}

fn create_program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

#[test]
fn test_initialize() {
    let env = create_test_env();
    let admin = Address::generate(&env);

    assert!(ScholarshipDisbursementEnhancementsContract::initialize(env.clone(), admin.clone()).is_ok());
    assert_eq!(
        ScholarshipDisbursementEnhancementsContract::initialize(env, admin),
        Err(ContractError::AlreadyInitialized)
    );
}

#[test]
fn test_configure_native_asset() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    let config = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    assert_eq!(config.asset_type, AssetType::Native);
    assert_eq!(config.asset_code, String::from_str(&env, "XLM"));
    assert_eq!(config.decimals, 7);
    assert!(!config.trustline_required);
    assert!(!config.issuer.is_some());
}

#[test]
fn test_configure_issued_asset() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let issuer = Address::generate(&env);

    let config = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Issued,
        String::from_str(&env, "USDC"),
        Some(issuer.clone()),
        2,
        true,
    ).unwrap();

    assert_eq!(config.asset_type, AssetType::Issued);
    assert_eq!(config.asset_code, String::from_str(&env, "USDC"));
    assert_eq!(config.issuer, Some(issuer));
    assert_eq!(config.decimals, 2);
    assert!(config.trustline_required);
}

#[test]
fn test_native_asset_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    // Native asset must be XLM
    let result = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "USDC"),
        None,
        7,
        false,
    );
    assert_eq!(result, Err(ContractError::InvalidAssetConfig));

    // Native asset cannot have issuer
    let result = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        Some(Address::generate(&env)),
        7,
        false,
    );
    assert_eq!(result, Err(ContractError::InvalidAssetConfig));

    // Native asset must have 7 decimals
    let result = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        2,
        false,
    );
    assert_eq!(result, Err(ContractError::InvalidAssetConfig));
}

#[test]
fn test_issued_asset_validation() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    // Issued asset must have issuer
    let result = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Issued,
        String::from_str(&env, "USDC"),
        None,
        2,
        true,
    );
    assert_eq!(result, Err(ContractError::InvalidAssetConfig));

    // Issued asset decimals must be > 0
    let result = ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Issued,
        String::from_str(&env, "USDC"),
        Some(Address::generate(&env)),
        0,
        true,
    );
    assert_eq!(result, Err(ContractError::InvalidAssetConfig));
}

#[test]
fn test_get_asset_config() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    let config = ScholarshipDisbursementEnhancementsContract::get_asset_config(env, program_id).unwrap();
    assert_eq!(config.asset_code, String::from_str(&env, "XLM"));
    assert!(config.is_active);
}

#[test]
fn test_create_transaction_record() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    assert_eq!(record.intent_id, intent_id);
    assert_eq!(record.program_id, program_id);
    assert_eq!(record.state, TransactionState::Submitted);
    assert_eq!(record.confirmations, 0);
    assert_eq!(record.required_confirmations, 3);
}

#[test]
fn test_update_transaction_state_valid_transitions() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    // Valid transitions
    let transitions = vec![
        (TransactionState::Submitted, TransactionState::Pending),
        (TransactionState::Pending, TransactionState::Confirming),
        (TransactionState::Confirming, TransactionState::Confirmed),
        (TransactionState::Confirmed, TransactionState::Finalized),
    ];

    for (from, to) in transitions {
        let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
            env.clone(),
            admin.clone(),
            intent_id.clone(),
            None,
            None,
            3,
        ).unwrap();

        // Set to 'from' state
        let mut record_mut = record.clone();
        record_mut.state = from;
        env.storage().persistent().set(&DataKey::Transaction(record.id.clone()), &record_mut);

        // Update to 'to' state
        let result = ScholarshipDisbursementEnhancementsContract::update_transaction_state(
            env.clone(),
            admin.clone(),
            record.id.clone(),
            to,
            None,
            1,
            None,
        );
        assert!(result.is_ok(), "Transition {:?} -> {:?} should be valid", from, to);
    }
}

#[test]
fn test_update_transaction_state_invalid_transitions() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    // Submitted -> Confirmed (skips Pending, Confirming)
    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    let result = ScholarshipDisbursementEnhancementsContract::update_transaction_state(
        env.clone(),
        admin.clone(),
        record.id.clone(),
        TransactionState::Confirmed,
        None,
        1,
        None,
    );
    assert_eq!(result, Err(ContractError::InvalidTransactionState));
}

#[test]
fn test_cannot_finalize_twice() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    // Move to Finalized
    let mut record_mut = record.clone();
    record_mut.state = TransactionState::Finalized;
    env.storage().persistent().set(&DataKey::Transaction(record.id.clone()), &record_mut);

    // Try to finalize again
    let result = ScholarshipDisbursementEnhancementsContract::update_transaction_state(
        env.clone(),
        admin.clone(),
        record.id.clone(),
        TransactionState::Finalized,
        None,
        1,
        None,
    );
    assert_eq!(result, Err(ContractError::AlreadyExecuted));
}

#[test]
fn test_initiate_recovery() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    // Create failed transaction
    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    let mut failed_record = record.clone();
    failed_record.state = TransactionState::Failed;
    failed_record.failure_reason = Some(String::from_str(&env, "Trustline missing"));
    env.storage().persistent().set(&DataKey::Transaction(record.id.clone()), &failed_record);

    // Initiate recovery
    let recovery = ScholarshipDisbursementEnhancementsContract::initiate_recovery(
        env.clone(),
        admin.clone(),
        record.id.clone(),
        RecoveryType::TrustlineMissing,
        None,
    ).unwrap();

    assert_eq!(recovery.original_intent_id, intent_id);
    assert_eq!(recovery.transaction_id, record.id);
    assert_eq!(recovery.recovery_type, RecoveryType::TrustlineMissing);
    assert_eq!(recovery.status, RecoveryStatus::Pending);
}

#[test]
fn test_complete_recovery() {
    let env = create_test_env();
    let admin = setup_contract(&env);
    let program_id = create_program_id(&env);
    let intent_id = BytesN::from_array(&env, &[3u8; 32]);

    ScholarshipDisbursementEnhancementsContract::configure_asset(
        env.clone(),
        admin.clone(),
        program_id.clone(),
        AssetType::Native,
        String::from_str(&env, "XLM"),
        None,
        7,
        false,
    ).unwrap();

    let record = ScholarshipDisbursementEnhancementsContract::create_transaction_record(
        env.clone(),
        admin.clone(),
        intent_id.clone(),
        None,
        None,
        3,
    ).unwrap();

    let mut failed_record = record.clone();
    failed_record.state = TransactionState::Failed;
    failed_record.failure_reason = Some(String::from_str(&env, "Trustline missing"));
    env.storage().persistent().set(&DataKey::Transaction(record.id.clone()), &failed_record);

    let recovery = ScholarshipDisbursementEnhancementsContract::initiate_recovery(
        env.clone(),
        admin.clone(),
        record.id.clone(),
        RecoveryType::TrustlineMissing,
        None,
    ).unwrap();

    // Complete recovery
    ScholarshipDisbursementEnhancementsContract::complete_recovery(
        env.clone(),
        admin.clone(),
        recovery.id.clone(),
        RecoveryStatus::Completed,
        Some(BytesN::from_array(&env, &[4u8; 32])),
    ).unwrap();

    // Check recovery status
    let completed = ScholarshipDisbursementEnhancementsContract::get_recovery_attempt(env.clone(), recovery.id).unwrap();
    assert_eq!(completed.status, RecoveryStatus::Completed);

    // Check transaction was reset to Submitted
    let tx = ScholarshipDisbursementEnhancementsContract::get_transaction(env, record.id).unwrap();
    assert_eq!(tx.state, TransactionState::Submitted);
    assert_eq!(tx.retry_count, 1);
}

#[test]
fn test_version() {
    let env = create_test_env();
    assert_eq!(ScholarshipDisbursementEnhancementsContract::version(env), 1);
}