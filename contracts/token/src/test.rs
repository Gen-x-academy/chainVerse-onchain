#![cfg(test)]

use super::*;
use soroban_sdk::{testutils::{Address as _}, Address, BytesN, Env};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

fn setup_contract(env: &Env) -> (Address, Address, Address) {
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let user1 = Address::generate(env);
    let user2 = Address::generate(env);

    client.initialize(&admin, &1000);

    (admin, user1, user2)
}

#[test]
fn test_total_supply_integrity() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    client.initialize(&admin, &1000);

    assert_eq!(client.total_supply(), 1000);
    assert_eq!(client.balance(&admin), 1000);
}

#[test]
fn test_transfer_success() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);

    client.transfer(&admin, &user, &300);

    assert_eq!(client.balance(&admin), 700);
    assert_eq!(client.balance(&user), 300);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_transfer_failure_insufficient_balance() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);

    client.transfer(&admin, &user, &2000);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_mint_attempt_after_deployment_fails() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    client.initialize(&admin, &1000);

    // Attempt to re-initialize (simulate mint attempt)
    client.initialize(&admin, &5000);
}

#[test]
fn test_transfer_applies_configured_royalty() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.set_royalty(&admin, &recipient, &500);
    client.transfer(&admin, &buyer, &200);

    assert_eq!(client.balance(&buyer), 190);
    assert_eq!(client.balance(&recipient), 10);
}

#[test]
fn test_transfer_from_applies_configured_royalty() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let buyer = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.set_royalty(&admin, &recipient, &1000);
    client.approve(&admin, &spender, &200, &None);
    client.transfer_from(&spender, &admin, &buyer, &200);

    assert_eq!(client.balance(&buyer), 180);
    assert_eq!(client.balance(&recipient), 20);
    assert_eq!(client.allowance(&admin, &spender), 0);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_non_admin_cannot_set_royalty() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let caller = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.set_royalty(&caller, &recipient, &500);
}

#[test]
#[should_panic(expected = "royalty bps too high")]
fn test_royalty_cannot_exceed_full_transfer() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.set_royalty(&admin, &recipient, &10001);
}

#[test]
#[should_panic(expected = "invalid amount")]
fn test_transfer_rejects_non_positive_amount() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.transfer(&admin, &user, &0);
}

#[test]
fn test_two_step_admin_transfer() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    client.set_royalty(&new_admin, &recipient, &100);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_wrong_address_cannot_accept_admin() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&impostor);
}

#[test]
fn test_pause_blocks_balance_mutations_until_unpaused() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.pause(&admin);
    assert!(client.is_paused());
    assert!(client.try_transfer(&admin, &user, &100).is_err());
    client.unpause(&admin);
    client.transfer(&admin, &user, &100);
    assert_eq!(client.balance(&user), 100);
}

#[test]
#[should_panic(expected = "unauthorized")]
fn test_non_admin_cannot_pause() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let caller = Address::generate(&env);

    client.initialize(&admin, &1000);
    client.pause(&caller);
}

#[test]
fn test_non_admin_cannot_upgrade() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let caller = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0; 32]);

    client.initialize(&admin, &1000);
    assert!(client.try_upgrade(&caller, &wasm_hash).is_err());
fn test_generated_client_transfer_and_allowance_flow_with_explicit_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (&admin, 1000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &1000);

    let transfer_amount = 300_i128;
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "transfer",
            args: (&admin, &recipient, transfer_amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.transfer(&admin, &recipient, &transfer_amount);
    assert_eq!(client.balance(&admin), 700);
    assert_eq!(client.balance(&recipient), 300);

    let approval_amount = 200_i128;
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "approve",
            args: (&admin, &spender, approval_amount, Option::<u64>::None).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.approve(&admin, &spender, &approval_amount, &None);
    assert_eq!(client.allowance(&admin, &spender), approval_amount);

    let spent = 125_i128;
    env.mock_auths(&[MockAuth {
        address: &spender,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "transfer_from",
            args: (&spender, &admin, &owner, spent).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.transfer_from(&spender, &admin, &owner, &spent);

    assert_eq!(client.allowance(&admin, &spender), 75);
    assert_eq!(client.balance(&admin), 575);
    assert_eq!(client.balance(&owner), 125);
}

#[test]
fn test_generated_client_approval_flow_without_blanket_auth() {
    let env = Env::default();
    let contract_id = env.register_contract(None, TokenContract);
    let client = TokenContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let transfer_to = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (&admin, 1000_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin, &1000);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "approve",
            args: (&admin, &spender, 250_i128, Option::<u64>::None).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.approve(&admin, &spender, &250, &None);

    let result = client.try_transfer_from(&spender, &admin, &transfer_to, &250);
    assert!(result.is_ok());
    assert_eq!(client.allowance(&admin, &spender), 0);
    assert_eq!(client.balance(&transfer_to), 250);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "revoke_allowance",
            args: (&admin, &spender).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.revoke_allowance(&admin, &spender);
    assert_eq!(client.allowance(&admin, &spender), 0);
}