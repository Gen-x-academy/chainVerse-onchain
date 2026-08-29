#![cfg(test)]

//! Acceptance tests for buyer-bound `fund_escrow` authorization.

use escrow::{EscrowContract, EscrowContractClient, EscrowError, EscrowStatus};
use soroban_sdk::{
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal,
};

fn deploy(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    EscrowContractClient<'static>,
) {
    env.ledger().with_mut(|li| li.timestamp = 1_000);

    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(env, &contract_id);

    let token_admin = Address::generate(env);
    let token = env.register_stellar_asset_contract(token_admin.clone());

    let admin = Address::generate(env);
    let buyer = Address::generate(env);
    let seller = Address::generate(env);

    // Blanket auth mocks must be active before minting (token admin auth)
    // and the admin setup below.
    env.mock_all_auths();
    StellarAssetClient::new(env, &token).mint(&buyer, &1_000);

    client.set_admin(&admin);
    client.whitelist_token(&admin, &token);
    env.set_auths(&[]);

    (admin, buyer, seller, token, client)
}

/// Calling `fund_escrow` without the buyer's authorization fails auth.
#[test]
fn fund_escrow_from_non_buyer_fails_auth() {
    let env = Env::default();
    let (_admin, buyer, seller, token, client) = deploy(&env);

    // Create with buyer auth only.
    env.mock_auths(&[MockAuth {
        address: &buyer,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "create_escrow",
            args: (&buyer, &seller, &token, 500_i128, 9_000_u64).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let id = client.create_escrow(&buyer, &seller, &token, &500, &9_000);

    let stranger = Address::generate(&env);

    // Only stranger is authorized — the caller must equal the escrow buyer,
    // so this must be rejected even with the stranger's auth present.
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "fund_escrow",
            args: (&stranger, id).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_fund_escrow(&stranger, &id);
    assert!(result.is_err(), "non-buyer auth must not fund escrow");

    let escrow = client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Created);
    assert_eq!(TokenClient::new(&env, &token).balance(&buyer), 1_000);
}

/// Calling `fund_escrow` with buyer authorization succeeds and moves funds.
#[test]
fn fund_escrow_from_buyer_succeeds() {
    let env = Env::default();
    let (_admin, buyer, seller, token, client) = deploy(&env);

    env.mock_all_auths();
    let id = client.create_escrow(&buyer, &seller, &token, &500, &9_000);
    client.fund_escrow(&buyer, &id);

    let escrow = client.get_escrow(&id).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(TokenClient::new(&env, &token).balance(&buyer), 500);
    assert_eq!(TokenClient::new(&env, &token).balance(&client.address), 500);
}

/// Funding an already-funded escrow returns InvalidEscrowState.
#[test]
fn fund_escrow_already_funded_returns_invalid_state() {
    let env = Env::default();
    let (_admin, buyer, seller, token, client) = deploy(&env);

    env.mock_all_auths();
    let id = client.create_escrow(&buyer, &seller, &token, &500, &9_000);
    client.fund_escrow(&buyer, &id);

    let result = client.try_fund_escrow(&buyer, &id);
    assert_eq!(result, Err(Ok(EscrowError::InvalidEscrowState)));
}
