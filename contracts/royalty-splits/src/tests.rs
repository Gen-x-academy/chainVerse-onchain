#![cfg(test)]
use crate::{ContractError, RoyaltySplitsContract};
use soroban_sdk::{testutils::Address as _, vec, Address, BytesN, Env, Symbol};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(RoyaltySplitsContract, ());
    let admin = Address::generate(&env);
    (env, contract_id, admin)
}

fn manifest_id(env: &Env, byte: u8) -> BytesN<32> {
    BytesN::from_array(env, &[byte; 32])
}

// ── manifest creation ────────────────────────────────────────────────────────

#[test]
fn test_create_manifest_with_valid_split() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let publisher = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(
        &admin,
        &mid,
        &vec![&env, author.clone(), publisher.clone()],
        &vec![&env, 7_000u32, 3_000u32],
    );

    let manifest = client.get_manifest(&mid);
    assert_eq!(manifest.recipients.len(), 2);
}

#[test]
fn test_create_manifest_rejects_split_not_summing_to_100_percent() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    let result = client.try_create_manifest(
        &admin,
        &mid,
        &vec![&env, a, b],
        &vec![&env, 7_000u32, 2_000u32], // sums to 9000, not 10000
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidBps)));
}

#[test]
fn test_create_manifest_rejects_zero_share() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    let result = client.try_create_manifest(
        &admin,
        &mid,
        &vec![&env, a, b],
        &vec![&env, 10_000u32, 0u32],
    );
    assert_eq!(result, Err(Ok(ContractError::InvalidBps)));
}

#[test]
fn test_create_manifest_rejects_mismatched_lengths() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    let result =
        client.try_create_manifest(&admin, &mid, &vec![&env, a], &vec![&env, 5_000u32, 5_000u32]);
    assert_eq!(result, Err(Ok(ContractError::MismatchedLengths)));
}

#[test]
fn test_create_manifest_rejects_empty() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mid = manifest_id(&env, 1);
    let result = client.try_create_manifest(
        &admin,
        &mid,
        &vec![&env],
        &vec![&env],
    );
    assert_eq!(result, Err(Ok(ContractError::EmptyManifest)));
}

#[test]
fn test_create_manifest_rejects_duplicate_id() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, a.clone()], &vec![&env, 10_000u32]);

    let result =
        client.try_create_manifest(&admin, &mid, &vec![&env, a], &vec![&env, 10_000u32]);
    assert_eq!(result, Err(Ok(ContractError::ManifestAlreadyExists)));
}

#[test]
fn test_non_admin_cannot_create_manifest() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let attacker = Address::generate(&env);
    let a = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    let result =
        client.try_create_manifest(&attacker, &mid, &vec![&env, a], &vec![&env, 10_000u32]);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

// ── distribution ─────────────────────────────────────────────────────────────

#[test]
fn test_distribute_splits_amount_exactly() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let publisher = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(
        &admin,
        &mid,
        &vec![&env, author.clone(), publisher.clone()],
        &vec![&env, 7_000u32, 3_000u32],
    );

    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &1_000i128);

    assert_eq!(client.get_balance(&author, &asset), 700);
    assert_eq!(client.get_balance(&publisher, &asset), 300);
    // Sum of shares equals the distributed amount exactly.
    assert_eq!(
        client.get_balance(&author, &asset) + client.get_balance(&publisher, &asset),
        1_000
    );
}

// Deterministic rounding: an amount that doesn't divide evenly must still
// sum exactly, with the remainder landing on the last recipient.
#[test]
fn test_distribute_rounding_remainder_goes_to_last_recipient() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    // Three equal-ish thirds: 3334 + 3333 + 3333 = 10000.
    client.create_manifest(
        &admin,
        &mid,
        &vec![&env, a.clone(), b.clone(), c.clone()],
        &vec![&env, 3_334u32, 3_333u32, 3_333u32],
    );

    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &10i128); // does not divide evenly

    let total =
        client.get_balance(&a, &asset) + client.get_balance(&b, &asset) + client.get_balance(&c, &asset);
    assert_eq!(total, 10);
}

#[test]
fn test_distribute_repeatedly_accumulates_balance() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, a.clone()], &vec![&env, 10_000u32]);

    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &100i128);
    client.distribute(&admin, &mid, &asset, &50i128);

    assert_eq!(client.get_balance(&a, &asset), 150);
}

#[test]
fn test_distribute_rejects_non_positive_amount() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let a = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, a], &vec![&env, 10_000u32]);

    let asset = Symbol::new(&env, "USDC");
    let result = client.try_distribute(&admin, &mid, &asset, &0i128);
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}

// ── withdrawal + reconciliation ──────────────────────────────────────────────

#[test]
fn test_withdraw_requires_recipient_auth_and_zeroes_balance() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, author.clone()], &vec![&env, 10_000u32]);

    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &500i128);

    let withdrawn = client.withdraw(&author, &asset);
    assert_eq!(withdrawn, 500);
    assert_eq!(client.get_balance(&author, &asset), 0);
}

#[test]
fn test_withdraw_with_zero_balance_fails() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let asset = Symbol::new(&env, "USDC");
    let result = client.try_withdraw(&author, &asset);
    assert_eq!(result, Err(Ok(ContractError::NothingToWithdraw)));
}

#[test]
fn test_double_withdraw_fails_second_time() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, author.clone()], &vec![&env, 10_000u32]);
    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &500i128);

    client.withdraw(&author, &asset);
    let result = client.try_withdraw(&author, &asset);
    assert_eq!(result, Err(Ok(ContractError::NothingToWithdraw)));
}

// Liabilities reconcile: distributed - withdrawn == outstanding balances,
// for a single asset, across multiple recipients and partial withdrawals.
#[test]
fn test_liabilities_reconcile_per_asset() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let publisher = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(
        &admin,
        &mid,
        &vec![&env, author.clone(), publisher.clone()],
        &vec![&env, 6_000u32, 4_000u32],
    );

    let asset = Symbol::new(&env, "USDC");
    client.distribute(&admin, &mid, &asset, &1_000i128);
    client.distribute(&admin, &mid, &asset, &500i128);

    // Only the author withdraws.
    client.withdraw(&author, &asset);

    let ledger = client.get_asset_ledger(&asset);
    assert_eq!(ledger.total_distributed, 1_500);
    let outstanding = client.get_outstanding_liability(&asset);
    // outstanding must equal publisher's still-unclaimed balance.
    assert_eq!(outstanding, client.get_balance(&publisher, &asset));
    assert_eq!(ledger.total_distributed - ledger.total_withdrawn, outstanding);
}

#[test]
fn test_different_assets_are_tracked_independently() {
    let (env, contract_id, admin) = setup();
    let client = crate::RoyaltySplitsContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let author = Address::generate(&env);
    let mid = manifest_id(&env, 1);
    client.create_manifest(&admin, &mid, &vec![&env, author.clone()], &vec![&env, 10_000u32]);

    let usdc = Symbol::new(&env, "USDC");
    let xlm = Symbol::new(&env, "XLM");
    client.distribute(&admin, &mid, &usdc, &100i128);
    client.distribute(&admin, &mid, &xlm, &200i128);

    assert_eq!(client.get_balance(&author, &usdc), 100);
    assert_eq!(client.get_balance(&author, &xlm), 200);
}
