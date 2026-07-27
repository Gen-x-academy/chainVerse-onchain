#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Vec};
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
fn test_threshold_zero_fails() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);
    let approver = Address::generate(&env);
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver);
    let result = client.try_create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &0u32);
    assert_eq!(result, Err(Ok(VaultError::InvalidThreshold)));
}

#[test]
fn test_threshold_exceeds_approvers_fails() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = Address::generate(&env);
    let approver = Address::generate(&env);
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver);
    // Only 1 approver but threshold asks for 5 — can never be met.
    let result = client.try_create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &5u32);
    assert_eq!(result, Err(Ok(VaultError::ThresholdExceedsApprovers)));
}

#[test]
fn test_duplicate_approvers_deduplicated() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(alice.clone());
    approvers.push_back(alice.clone());
    approvers.push_back(bob.clone());
    // [Alice, Alice, Bob] with threshold 2 -> deduplicated to [Alice, Bob],
    // so the vault is created successfully rather than rejected.
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &2u32);
    // Alice approves once; a second approval from Alice must be rejected as
    // AlreadyVoted (not counted twice), proving her single approval is
    // insufficient to reach the threshold of 2 on its own.
    client.approve_vault(&vault_id, &alice);
    let result = client.try_approve_vault(&vault_id, &alice);
    assert_eq!(result, Err(Ok(VaultError::AlreadyVoted)));
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
fn test_approve_and_release_at_threshold() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    client.approve_vault(&vault_id, &approver);
    // Vault should now be Released, not Pending — a further approve call
    // hits the status guard (NotPending) rather than AlreadyVoted.
    let result = client.try_approve_vault(&vault_id, &approver);
    assert_eq!(result, Err(Ok(VaultError::NotPending)));
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
fn test_self_approve_by_beneficiary_fails() {
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

#[test]
fn test_revoke_approval_before_threshold() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    // threshold 2 so a single approval never auto-releases
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &2u32);
    client.approve_vault(&vault_id, &approver);
    client.revoke_approval(&vault_id, &approver);
    // Approving again succeeds (not AlreadyVoted) since the prior vote was revoked.
    client.approve_vault(&vault_id, &approver);
}

#[test]
fn test_revoke_after_release_fails() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    client.approve_vault(&vault_id, &approver);
    let result = client.try_revoke_approval(&vault_id, &approver);
    assert_eq!(result, Err(Ok(VaultError::NotPending)));
}

#[test]
fn test_emergency_cancel_by_admin() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.set_admin(&admin);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let token_client = soroban_sdk::token::Client::new(&env, &token_id);
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token_id, &1000_i128, &approvers, &1u32);
    let balance_before = token_client.balance(&depositor);
    let reason = String::from_str(&env, "legal hold");
    client.emergency_cancel(&vault_id, &reason);
    let balance_after = token_client.balance(&depositor);
    assert_eq!(balance_after, balance_before + 1000_i128);
    // Vault is Cancelled, not Pending — approve now fails with NotPending.
    let result = client.try_approve_vault(&vault_id, &approver);
    assert_eq!(result, Err(Ok(VaultError::NotPending)));
}

#[test]
fn test_emergency_cancel_by_non_admin_fails() {
    // No admin has ever been configured for this contract instance. Even
    // with mock_all_auths() approving any signature, emergency_cancel must
    // still fail closed rather than let an arbitrary caller through when
    // there's no legitimate admin to authorize the action.
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    let reason = String::from_str(&env, "unauthorized attempt");
    let result = client.try_emergency_cancel(&vault_id, &reason);
    assert_eq!(result, Err(Ok(VaultError::NotInitialized)));
}
