#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Vec};
use crate::{EscrowVault, VaultError};

fn setup() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, EscrowVault);
    (env, contract_id)
}

#[test]
fn test_create_vault_empty_approvers_rejected() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);
    let approvers: Vec<Address> = Vec::new(&env);
    let result = client.try_create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    assert_eq!(result, Err(Ok(VaultError::EmptyApprovers)));
}

#[test]
fn test_create_vault_zero_amount_rejected() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);
    let approver = Address::generate(&env);
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver);
    let result = client.try_create_vault(&depositor, &recipient, &token, &0_i128, &approvers, &1u32);
    assert_eq!(result, Err(Ok(VaultError::InvalidAmount)));
}

#[test]
fn test_duplicate_approver_rejected() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);
    let approver = Address::generate(&env);
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    approvers.push_back(approver.clone());
    let result = client.try_create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    assert_eq!(result, Err(Ok(VaultError::DuplicateApprover)));
}

#[test]
fn test_create_vault_success() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    // Register a mock token contract so transfer doesn't panic
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    // Verify the vault is stored by retrieving it via a second approve attempt that returns NotPending or similar
    // We confirm storage by checking that approve_vault finds the vault (returns Unauthorized for non-approver)
    let stranger = Address::generate(&env);
    let result = client.try_approve_vault(&vault_id, &stranger);
    // stranger is not an approver → Unauthorized (not NotFound), proving vault was stored
    assert_eq!(result, Err(Ok(VaultError::Unauthorized)));
}

#[test]
fn test_approve_unauthorized() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    let stranger = Address::generate(&env);
    let result = client.try_approve_vault(&vault_id, &stranger);
    assert_eq!(result, Err(Ok(VaultError::Unauthorized)));
}

#[test]
fn test_approve_self_approve_blocked() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    // depositor is also listed as approver
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(depositor.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    let result = client.try_approve_vault(&vault_id, &depositor);
    assert_eq!(result, Err(Ok(VaultError::Unauthorized)));
}

#[test]
fn test_double_vote_blocked() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    // threshold 2 so vault stays Pending after first vote
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &2u32);
    client.approve_vault(&vault_id, &approver);
    let result = client.try_approve_vault(&vault_id, &approver);
    assert_eq!(result, Err(Ok(VaultError::AlreadyVoted)));
}
