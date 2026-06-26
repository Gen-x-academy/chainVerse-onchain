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
