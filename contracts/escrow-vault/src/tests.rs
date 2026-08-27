#![cfg(test)]
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Vec};
use crate::{DataKey, EscrowVault, Vault, VaultError, VaultStatus};

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

// #866 — two vaults born in the same ledger must get distinct ids. The test
// env keeps the ledger timestamp fixed, so only the monotonic nonce separates
// the two vault ids.
#[test]
fn test_same_ledger_timestamp_vaults_get_unique_ids() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);
    let approver1 = Address::generate(&env);
    let approver2 = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers_a = Vec::new(&env);
    approvers_a.push_back(approver1.clone());
    let mut approvers_b = Vec::new(&env);
    approvers_b.push_back(approver2.clone());
    let id1 = client.create_vault(&depositor, &recipient1, &token, &1000_i128, &approvers_a, &1u32);
    let id2 = client.create_vault(&depositor, &recipient2, &token, &2000_i128, &approvers_b, &1u32);
    assert_ne!(id1, id2);
}

// #865 — with two unique approvers, a threshold equal to that count is valid;
// it must not be rejected as ThresholdExceedsApprovers.
#[test]
fn test_threshold_equal_to_unique_count_succeeds() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(alice.clone());
    approvers.push_back(bob.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &2u32);
    // The vault is stored: a stranger is Unauthorized (not NotFound), and the
    // single approval below leaves it Pending until bob also approves.
    let stranger = Address::generate(&env);
    assert_eq!(client.try_approve_vault(&vault_id, &stranger), Err(Ok(VaultError::Unauthorized)));
    client.approve_vault(&vault_id, &alice);
    let res = client.try_approve_vault(&vault_id, &alice);
    assert_eq!(res, Err(Ok(VaultError::AlreadyVoted)));
}

// #867 positive — with the recipient/depositor excluded from the approver set,
// a legitimate external approver releases the payout to the recipient.
#[test]
fn test_external_approver_releases_funds_to_recipient() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let approver = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let mut approvers = Vec::new(&env);
    approvers.push_back(approver.clone());
    let vault_id = client.create_vault(&depositor, &recipient, &token, &1000_i128, &approvers, &1u32);
    let bal_before = soroban_sdk::token::Client::new(&env, &token).balance(&recipient);
    client.approve_vault(&vault_id, &approver);
    let bal_after = soroban_sdk::token::Client::new(&env, &token).balance(&recipient);
    assert_eq!(bal_after, bal_before + 1000_i128);
}

// #867 adversarial — create_vault already rejects a recipient-as-approver with
// ConflictOfInterest, so the attacker can never reach threshold release that
// way. To prove the approve-time guard is defense-in-depth, inject the hostile
// vault state directly (recipient IS an approver, threshold 1) and verify the
// recipient approving its own payout still fails with ConflictOfInterest and
// no funds move to the recipient.
#[test]
fn test_recipient_in_approvers_threshold_one_cannot_self_approve() {
    let (env, contract_id) = setup();
    let client = crate::EscrowVaultClient::new(&env, &contract_id);
    let depositor = Address::generate(&env);
    let recipient = Address::generate(&env);
    let token = env.register_stellar_asset_contract_v2(depositor.clone()).address();
    let vault_id = BytesN::from_array(&env, &[7u8; 32]);
    let mut approvers = Vec::new(&env);
    approvers.push_back(recipient.clone());
    let vault = Vault {
        depositor: depositor.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        amount: 1000_i128,
        approvers,
        approvals: 0,
        threshold: 1,
        status: VaultStatus::Pending,
    };
    env.as_contract(&contract_id, || {
        env.storage().persistent().set(&DataKey::Vault(vault_id.clone()), &vault);
        env.storage().persistent().extend_ttl(&DataKey::Vault(vault_id.clone()), 100_000, 500_000);
    });
    // Fund the contract so a release would be observable as a recipient balance move.
    soroban_sdk::token::Client::new(&env, &token).transfer(&depositor, &contract_id, &1000_i128);
    let bal_before = soroban_sdk::token::Client::new(&env, &token).balance(&recipient);
    // The recipient's single vote would meet threshold 1, but role-overlap must
    // block it with ConflictOfInterest before any funds can be released.
    let result = client.try_approve_vault(&vault_id, &recipient);
    assert_eq!(result, Err(Ok(VaultError::ConflictOfInterest)));
    let bal_after = soroban_sdk::token::Client::new(&env, &token).balance(&recipient);
    assert_eq!(bal_after, bal_before);
}
