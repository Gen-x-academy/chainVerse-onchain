#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{CHVToken, CHVTokenClient, TokenError};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CHVToken);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    (env, contract_id, admin, treasury)
}

#[test]
fn test_initialize_sets_admin() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let new_admin = Address::generate(&env);
    assert!(client.try_propose_admin(&admin, &new_admin, &(env.ledger().timestamp() + 86_400)).is_ok());
}

#[test]
fn test_standard_token_metadata() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);

    assert_eq!(client.name(), soroban_sdk::String::from_str(&env, "ChainVerse"));
    assert_eq!(client.symbol(), soroban_sdk::String::from_str(&env, "CHV"));
    assert_eq!(client.decimals(), 7_u32);
}

#[test]
fn test_mint_increases_supply() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let initial_supply = client.total_minted();
    let recipient = Address::generate(&env);
    let mint_amount = 5_000_000_i128;

    client.mint(&admin, &recipient, &mint_amount);

    assert_eq!(client.total_minted(), initial_supply + mint_amount);
    assert_eq!(client.balance(&recipient), mint_amount);
}

#[test]
fn test_mint_above_cap_fails() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let max_supply: i128 = 1_000_000_000 * 10_i128.pow(7);
    let over_cap_amount = max_supply + 1;
    let recipient = Address::generate(&env);

    let result = client.try_mint(&admin, &recipient, &over_cap_amount);
    assert_eq!(result, Err(Ok(TokenError::SupplyCapExceeded)));
}

#[test]
fn test_transfer_reduces_sender_balance() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let sender = Address::generate(&env);
    let receiver = Address::generate(&env);

    client.mint(&admin, &sender, &1_000_i128);
    let sender_initial_balance = client.balance(&sender);
    let transfer_amount = 400_i128;

    client.transfer(&sender, &receiver, &transfer_amount);

    assert_eq!(client.balance(&sender), sender_initial_balance - transfer_amount);
    assert_eq!(client.balance(&receiver), transfer_amount);
}

#[test]
fn test_transfer_insufficient_balance_fails() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let sender = Address::generate(&env);
    let receiver = Address::generate(&env);

    client.mint(&admin, &sender, &100_i128);

    let result = client.try_transfer(&sender, &receiver, &500_i128);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn test_approve_and_transfer_from() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.mint(&admin, &owner, &1_000_i128);

    client.transfer(&owner, &recipient, &500_i128);
    assert_eq!(client.balance(&recipient), 500_i128);
    assert_eq!(client.balance(&owner), 500_i128);
}

#[test]
fn test_transfer_from_exceeds_allowance_fails() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.mint(&admin, &owner, &100_i128);

    let result = client.try_transfer(&owner, &recipient, &200_i128);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn test_allowance_state_changes_emit_events() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(&admin, &owner, &1_000_i128);

    let before_approval = env.events().all().len();
    client.approve(&owner, &spender, &600_i128, &100_u32);
    assert_eq!(client.allowance(&owner, &spender), 600_i128);
    assert_eq!(env.events().all().len(), before_approval + 1);

    let before_decrement = env.events().all().len();
    client.transfer_from(&spender, &owner, &recipient, &250_i128);
    assert_eq!(client.allowance(&owner, &spender), 350_i128);
    assert_eq!(env.events().all().len(), before_decrement + 2);

    let before_revocation = env.events().all().len();
    client.revoke_allowance(&owner, &spender);
    assert_eq!(client.allowance(&owner, &spender), 0_i128);
    assert_eq!(env.events().all().len(), before_revocation + 1);
}

#[test]
fn test_failed_allowance_changes_emit_no_events() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(&admin, &owner, &100_i128);
    client.approve(&owner, &spender, &50_i128, &100_u32);

    let before_invalid_approval = env.events().all().len();
    assert_eq!(
        client.try_approve(&owner, &spender, &-1_i128, &100_u32),
        Err(Ok(TokenError::InvalidAmount))
    );
    assert_eq!(env.events().all().len(), before_invalid_approval);

    let before_excessive_transfer = env.events().all().len();
    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &51_i128),
        Err(Ok(TokenError::InsufficientAllowance))
    );
    assert_eq!(env.events().all().len(), before_excessive_transfer);
    assert_eq!(client.allowance(&owner, &spender), 50_i128);
}

#[test]
fn test_burn_reduces_supply() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let user = Address::generate(&env);
    client.mint(&admin, &user, &1_000_i128);
    let initial_balance = client.balance(&user);

    client.burn(&user, &400_i128);

    assert_eq!(client.balance(&user), initial_balance - 400_i128);
}

#[test]
fn test_burn_underflow_fails() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let user = Address::generate(&env);
    client.mint(&admin, &user, &100_i128);

    let result = client.try_burn(&user, &200_i128);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn test_freeze_blocks_transfer() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let sender = Address::generate(&env);
    let receiver = Address::generate(&env);

    let result = client.try_transfer(&sender, &receiver, &0_i128);
    assert_eq!(result, Err(Ok(TokenError::InvalidAmount)));
}

#[test]
fn test_frozen_sender_and_recipient_cannot_transfer() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(&admin, &sender, &100_i128);

    client.freeze_account(&sender);
    assert_eq!(
        client.try_transfer(&sender, &recipient, &1_i128),
        Err(Ok(TokenError::AccountFrozen))
    );

    client.unfreeze_account(&sender);
    client.freeze_account(&recipient);
    assert_eq!(
        client.try_transfer(&sender, &recipient, &1_i128),
        Err(Ok(TokenError::AccountFrozen))
    );
}

#[test]
fn test_frozen_owner_and_spender_cannot_approve() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);

    client.freeze_account(&owner);
    assert_eq!(
        client.try_approve(&owner, &spender, &10_i128, &100_u32),
        Err(Ok(TokenError::AccountFrozen))
    );

    client.unfreeze_account(&owner);
    client.freeze_account(&spender);
    assert_eq!(
        client.try_approve(&owner, &spender, &10_i128, &100_u32),
        Err(Ok(TokenError::AccountFrozen))
    );
}

#[test]
fn test_frozen_transfer_from_roles_are_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(&admin, &owner, &100_i128);
    client.approve(&owner, &spender, &10_i128, &100_u32);

    client.freeze_account(&owner);
    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &1_i128),
        Err(Ok(TokenError::AccountFrozen))
    );

    client.unfreeze_account(&owner);
    client.freeze_account(&spender);
    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &1_i128),
        Err(Ok(TokenError::AccountFrozen))
    );

    client.unfreeze_account(&spender);
    client.freeze_account(&recipient);
    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &1_i128),
        Err(Ok(TokenError::AccountFrozen))
    );
}

#[test]
fn test_frozen_spender_leaves_allowance_and_balances_unchanged() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);
    client.mint(&admin, &owner, &100_i128);
    client.approve(&owner, &spender, &40_i128, &100_u32);
    client.freeze_account(&spender);

    let owner_balance = client.balance(&owner);
    let recipient_balance = client.balance(&recipient);
    let allowance = client.allowance(&owner, &spender);

    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &10_i128),
        Err(Ok(TokenError::AccountFrozen))
    );
    assert_eq!(client.allowance(&owner, &spender), allowance);
    assert_eq!(client.balance(&owner), owner_balance);
    assert_eq!(client.balance(&recipient), recipient_balance);
}

#[test]
fn test_two_step_admin_transfer() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin, &(env.ledger().timestamp() + 86_400));
    client.accept_admin(&new_admin);

    let recipient = Address::generate(&env);
    assert!(client.try_mint(&new_admin, &recipient, &1_000_i128).is_ok());

    assert_eq!(
        client.try_mint(&admin, &recipient, &1_000_i128),
        Err(Ok(TokenError::Unauthorized))
    );
}

#[test]
fn test_unauthorized_mint_fails() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);

    let non_admin = Address::generate(&env);
    let recipient = Address::generate(&env);

    let result = client.try_mint(&non_admin, &recipient, &1_000_i128);
    assert_eq!(result, Err(Ok(TokenError::Unauthorized)));
}

#[test]
fn test_initialize_sets_treasury_balance() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    assert!(client.balance(&treasury) > 0);
}

#[test]
fn test_transfer_moves_tokens() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let recipient = Address::generate(&env);
    client.transfer(&treasury, &recipient, &1000_i128);
    assert_eq!(client.balance(&recipient), 1000);
}

#[test]
fn test_self_transfer_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let result = client.try_transfer(&treasury, &treasury, &100_i128);
    assert_eq!(result, Err(Ok(TokenError::SelfTransfer)));
}

#[test]
fn test_transfer_insufficient_balance_rejected() {
    let (env, contract_id, admin, _) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let treasury = Address::generate(&env);
    client.initialize(&admin, &treasury);
    let user = Address::generate(&env);
    let result = client.try_transfer(&user, &treasury, &100_i128);
    assert_eq!(result, Err(Ok(TokenError::InsufficientBalance)));
}

#[test]
fn test_negative_transfer_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    let recipient = Address::generate(&env);
    let result = client.try_transfer(&treasury, &recipient, &(-1_i128));
    assert_eq!(result, Err(Ok(TokenError::InvalidAmount)));
}
