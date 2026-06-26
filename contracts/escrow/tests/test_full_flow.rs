#![cfg(test)]

use escrow::{EscrowContract, EscrowContractClient, EscrowError};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Address, Env, String,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sets up a minimal environment: registers the escrow contract, creates a
/// whitelisted token, mints `1_000` units to `buyer`, and advances the ledger
/// timestamp to `now`.
fn setup(
    now: u64,
) -> (
    Env,
    Address, // buyer
    Address, // seller
    Address, // token
    EscrowContractClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().with_mut(|li| {
        li.timestamp = now;
    });

    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_addr = env.register_stellar_asset_contract(token_admin.clone());
    let stellar = StellarAssetClient::new(&env, &token_addr);

    let admin = Address::generate(&env);
    let buyer = Address::generate(&env);
    let seller = Address::generate(&env);

    stellar.mint(&buyer, &1_000);

    client.set_admin(&admin);
    client.whitelist_token(&token_addr);

    (env, buyer, seller, token_addr, client)
}

// ---------------------------------------------------------------------------
// Baseline / version smoke-tests (preserved from original)
// ---------------------------------------------------------------------------

#[test]
fn e2e_escrow_version_and_count_baseline() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    assert_eq!(client.version(), String::from_str(&env, "1.0.0"));
    assert_eq!(client.get_escrow_count(), 0);
    assert_eq!(client.get_total_volume(), 0);
}

#[test]
fn e2e_escrow_version_and_dispute_baseline() {
    let env = Env::default();
    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    assert_eq!(client.version(), String::from_str(&env, "1.0.0"));
    assert_eq!(
        client.try_resolve_dispute(&1, &true),
        Err(Ok(EscrowError::DisputeResolutionNotImplemented))
    );
}

// ---------------------------------------------------------------------------
// get_escrow_count — entry-point contract tests
// ---------------------------------------------------------------------------

/// Before any escrow has been created the public entry point must return 0.
/// This ensures dashboards and pagination start from a correct baseline.
#[test]
fn get_escrow_count_returns_zero_before_any_escrow_is_created() {
    let (_env, _buyer, _seller, _token, client) = setup(1_000);

    assert_eq!(
        client.get_escrow_count(),
        0,
        "count must be 0 on a freshly deployed contract"
    );
}

/// Each successful `create_escrow` call must increment the count by exactly 1.
/// After N creations the count must equal N, providing reliable data for
/// dashboard totals and offset-based pagination.
#[test]
fn get_escrow_count_increments_by_one_for_each_created_escrow() {
    let (_env, buyer, seller, token, client) = setup(1_000);

    assert_eq!(client.get_escrow_count(), 0);

    client.create_escrow(&buyer, &seller, &token, &100, &9_000);
    assert_eq!(client.get_escrow_count(), 1, "count must be 1 after first escrow");

    client.create_escrow(&buyer, &seller, &token, &100, &9_000);
    assert_eq!(client.get_escrow_count(), 2, "count must be 2 after second escrow");

    client.create_escrow(&buyer, &seller, &token, &100, &9_000);
    assert_eq!(client.get_escrow_count(), 3, "count must be 3 after third escrow");
}

/// The count must NOT decrement when an escrow is released.  Dashboards show
/// the total number of escrows ever created, not only those that are still
/// pending — so releasing funds must leave the counter unchanged.
#[test]
fn get_escrow_count_does_not_decrement_after_release() {
    let (_env, buyer, seller, token, client) = setup(1_000);

    let id = client.create_escrow(&buyer, &seller, &token, &200, &9_000);
    assert_eq!(client.get_escrow_count(), 1);

    client.release_funds(&id);

    assert_eq!(
        client.get_escrow_count(),
        1,
        "releasing funds must not change the escrow count"
    );
}

/// The count must NOT decrement when an escrow is refunded after expiry.
/// Cancelled/refunded escrows are still part of the historical total that
/// dashboards and pagination rely on.
#[test]
fn get_escrow_count_does_not_decrement_after_refund() {
    let (env, buyer, seller, token, client) = setup(1_000);

    let id = client.create_escrow(&buyer, &seller, &token, &200, &5_000);
    assert_eq!(client.get_escrow_count(), 1);

    // Advance past expiration so refund is valid.
    env.ledger().with_mut(|li| li.timestamp = 6_000);

    client.refund_buyer(&id);

    assert_eq!(
        client.get_escrow_count(),
        1,
        "refunding an escrow must not change the escrow count"
    );
}

/// The returned ID of each new escrow must equal the current count,
/// confirming that IDs are assigned sequentially starting from 1 and that
/// `get_escrow_count` faithfully tracks the next available slot.
#[test]
fn get_escrow_count_matches_sequential_escrow_ids() {
    let (_env, buyer, seller, token, client) = setup(1_000);

    for expected_id in 1_u64..=5 {
        let id = client.create_escrow(&buyer, &seller, &token, &50, &9_000);
        assert_eq!(
            id, expected_id,
            "escrow id {expected_id} must be assigned sequentially"
        );
        assert_eq!(
            client.get_escrow_count(),
            expected_id,
            "count must equal the last assigned id after {expected_id} creations"
        );
    }
}
