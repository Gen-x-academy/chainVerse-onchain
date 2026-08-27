#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    Address, BytesN, Env,
};
use crate::{CHVToken, CHVTokenClient, TokenError};

/// Hard cap constant mirrored from lib.rs for use in tests.
const MAX_SUPPLY: i128 = 1_000_000_000 * 10_i128.pow(7);

/// Treasury receives this as initial supply on initialize().
const INITIAL_SUPPLY: i128 = 100_000_000 * 10_i128.pow(7);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register the contract, mock all auths, initialise, and return
/// (env, contract_id, admin, treasury) — matching the pattern in test.rs.
fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, CHVToken);
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let client = CHVTokenClient::new(&env, &contract_id);
    client.initialize(&admin, &treasury);
    (env, contract_id, admin, treasury)
}

fn set_sequence(env: &Env, sequence: u32) {
    env.ledger().set(LedgerInfo {
        timestamp: 0,
        protocol_version: 22,
        sequence_number: sequence,
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 4096,
        max_entry_ttl: 6_220_800,
    });
}

#[test]
fn test_allowance_read_extends_ttl() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);

    client.approve(&owner, &spender, &100_i128);
    set_sequence(&env, 150_000);
    assert_eq!(client.allowance(&owner, &spender), 100_i128);
    set_sequence(&env, 250_000);
    assert_eq!(client.allowance(&owner, &spender), 100_i128);
}

#[test]
fn test_transfer_from_decrement_extends_ttl() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.mint(&admin, &owner, &100_i128);
    client.approve(&owner, &spender, &100_i128);
    set_sequence(&env, 150_000);
    client.transfer_from(&spender, &owner, &recipient, &40_i128);
    set_sequence(&env, 250_000);
    assert_eq!(client.allowance(&owner, &spender), 60_i128);
}

#[test]
fn test_expired_allowance_reads_as_zero_and_cannot_be_spent() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    client.mint(&admin, &owner, &100_i128);
    client.approve(&owner, &spender, &100_i128);
    set_sequence(&env, 200_001);

    assert_eq!(client.allowance(&owner, &spender), 0);
    assert_eq!(
        client.try_transfer_from(&spender, &owner, &recipient, &1_i128),
        Err(Ok(TokenError::InsufficientAllowance))
    );
}

#[test]
fn test_mint_and_burn_separate_cumulative_and_circulating_supply() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let initial_minted = client.total_minted();
    let initial_circulating = client.circulating_supply();

    client.mint(&admin, &user, &1_000_i128);
    client.burn(&user, &400_i128);

    assert_eq!(client.total_minted(), initial_minted + 1_000_i128);
    assert_eq!(client.circulating_supply(), initial_circulating + 600_i128);
}

#[test]
fn test_failed_burn_does_not_change_supply_counters() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.mint(&admin, &user, &100_i128);
    let minted = client.total_minted();
    let circulating = client.circulating_supply();

    assert_eq!(client.try_burn(&user, &101_i128), Err(Ok(TokenError::InsufficientBalance)));
    assert_eq!(client.total_minted(), minted);
    assert_eq!(client.circulating_supply(), circulating);
}

#[test]
fn test_adversarial_mint_cannot_overflow_supply_cap() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let minted = client.total_minted();
    let circulating = client.circulating_supply();

    assert_eq!(
        client.try_mint(&admin, &treasury, &i128::MAX),
        Err(Ok(TokenError::SupplyCapExceeded))
    );
    assert_eq!(client.total_minted(), minted);
    assert_eq!(client.circulating_supply(), circulating);
}

#[test]
fn test_storage_version_is_initialized_and_migration_is_idempotent() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let circulating = client.circulating_supply();

    assert_eq!(client.storage_version(), 1);
    client.migrate(&admin, &1_u32, &circulating);
    client.migrate(&admin, &1_u32, &circulating);
    assert_eq!(client.storage_version(), 1);
    assert_eq!(client.circulating_supply(), circulating);
}

#[test]
fn test_legacy_storage_migration_sets_version_once() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let circulating = client.circulating_supply();

    env.as_contract(&contract_id, || {
        env.storage().instance().remove(&crate::DataKey::StorageVersion);
    });
    assert_eq!(client.storage_version(), 0);
    client.migrate(&admin, &0_u32, &circulating);
    assert_eq!(client.storage_version(), 1);
    assert_eq!(client.circulating_supply(), circulating);
}

#[test]
fn test_migration_rejects_unsupported_source_and_invalid_snapshot() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let circulating = client.circulating_supply();

    assert_eq!(
        client.try_migrate(&admin, &99_u32, &circulating),
        Err(Ok(TokenError::UnsupportedStorageVersion))
    );
    assert_eq!(
        client.try_migrate(&admin, &1_u32, &(circulating + 1)),
        Err(Ok(TokenError::InvalidMigration))
    );
    assert_eq!(client.storage_version(), 1);
    assert_eq!(client.circulating_supply(), circulating);
}

#[test]
fn test_upgrade_rejects_unsupported_stored_version() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let unsupported_version = 99_u32;

    env.as_contract(&contract_id, || {
        env.storage().instance().set(&crate::DataKey::StorageVersion, &unsupported_version);
    });
    assert_eq!(
        client.try_upgrade(&admin, &BytesN::from_array(&env, &[0_u8; 32])),
        Err(Ok(TokenError::UnsupportedStorageVersion))
    );
}

fn next_random(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    *state
}

#[test]
fn property_generated_operations_conserve_supply() {
    for seed in [1_u64, 17, 1_003, u64::MAX] {
        let (env, contract_id, initial_admin, treasury) = setup();
        let client = CHVTokenClient::new(&env, &contract_id);
        let accounts = [
            treasury,
            Address::generate(&env),
            Address::generate(&env),
            Address::generate(&env),
        ];
        let mut admin = initial_admin;
        let mut state = seed;

        for step in 0..96 {
            let operation = next_random(&mut state) % 9;
            let from_index = (next_random(&mut state) % accounts.len() as u64) as usize;
            let to_index = (next_random(&mut state) % accounts.len() as u64) as usize;
            let amount = if step == 95 {
                i128::MAX
            } else {
                (next_random(&mut state) % 500 + 1) as i128
            };
            let from = &accounts[from_index];
            let to = &accounts[to_index];

            match operation {
                0 => {
                    let _ = client.try_mint(&admin, to, &amount);
                }
                1 => {
                    let _ = client.try_transfer(from, to, &amount);
                }
                2 => {
                    let _ = client.try_approve(from, to, &amount);
                }
                3 => {
                    let allowance_before = client.allowance(from, to);
                    let result = client.try_transfer_from(
                        to,
                        from,
                        &accounts[(to_index + 1) % accounts.len()],
                        &amount,
                    );
                    let allowance_after = client.allowance(from, to);
                    assert!(allowance_after <= allowance_before);
                    if result.is_ok() {
                        assert_eq!(allowance_after, allowance_before - amount);
                    }
                }
                4 => {
                    let _ = client.try_burn(from, &amount);
                }
                5 => {
                    let _ = client.try_freeze(from);
                }
                6 => {
                    let _ = client.try_unfreeze(from);
                }
                7 => {
                    let proposed = accounts[(from_index + 1) % accounts.len()].clone();
                    if client.try_propose_admin(&admin, &proposed).is_ok()
                        && client.try_accept_admin(&proposed).is_ok()
                    {
                        admin = proposed;
                    }
                }
                _ => {
                    let _ = client.allowance(from, to);
                }
            }

            let balance_sum: i128 = accounts.iter().map(|account| client.balance(account)).sum();
            assert!(accounts.iter().all(|account| client.balance(account) >= 0));
            assert_eq!(balance_sum, client.circulating_supply());
            assert!(client.circulating_supply() >= 0);
            assert!(client.total_minted() >= client.circulating_supply());
            assert!(client.total_minted() <= MAX_SUPPLY);
            for owner in &accounts {
                for spender in &accounts {
                    assert!(client.allowance(owner, spender) >= 0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mint-cap tests (issue #650 — supply cap enforcement)
// ---------------------------------------------------------------------------

/// Minting exactly the remaining supply up to MAX_SUPPLY should succeed.
#[test]
fn test_mint_up_to_cap_succeeds() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let remaining = MAX_SUPPLY - INITIAL_SUPPLY;
    let recipient = Address::generate(&env);
    let result = client.try_mint(&admin, &recipient, &remaining);
    assert!(
        result.is_ok(),
        "minting exactly the remaining supply should succeed"
    );
    assert_eq!(client.total_minted(), MAX_SUPPLY);
}

/// Minting a single token beyond MAX_SUPPLY must return SupplyCapExceeded.
#[test]
fn test_mint_cap_enforced() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    // Try to push total_minted one token over the cap.
    let over_cap = MAX_SUPPLY - INITIAL_SUPPLY + 1;
    let result = client.try_mint(&admin, &treasury, &over_cap);
    assert_eq!(
        result,
        Err(Ok(TokenError::SupplyCapExceeded)),
        "minting above MAX_SUPPLY must return SupplyCapExceeded"
    );
}

/// Minting zero tokens is always invalid regardless of the cap state.
#[test]
fn test_mint_zero_amount_rejected() {
    let (env, contract_id, admin, treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let result = client.try_mint(&admin, &treasury, &0_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InvalidAmount)),
        "minting zero must return InvalidAmount"
    );
}

// ---------------------------------------------------------------------------
// Burn underflow tests (issue #650 — underflow protection)
// ---------------------------------------------------------------------------

/// Burning more than an account's balance must return InsufficientBalance.
#[test]
fn test_burn_underflow_returns_error() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    // Give the user a small balance, then try to burn one more than they have.
    client.mint(&admin, &user, &500_i128);
    let result = client.try_burn(&user, &501_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InsufficientBalance)),
        "burning more than balance must return InsufficientBalance"
    );
}

/// Burning from an account with zero balance must return InsufficientBalance.
#[test]
fn test_burn_insufficient_balance_fails() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    // user has never received tokens — any positive burn must fail.
    let result = client.try_burn(&user, &1_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InsufficientBalance)),
        "burning from a zero-balance account must return InsufficientBalance"
    );
}

/// Burning zero tokens is always invalid (InvalidAmount guard runs before balance check).
#[test]
fn test_burn_zero_amount_rejected() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let result = client.try_burn(&user, &0_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::InvalidAmount)),
        "burning zero must return InvalidAmount"
    );
}

/// A successful burn reduces the account balance by exactly the burned amount.
#[test]
fn test_burn_reduces_balance() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.mint(&admin, &user, &1_000_i128);
    client.burn(&user, &400_i128);
    assert_eq!(
        client.balance(&user),
        600_i128,
        "balance after burn must equal initial minus burned amount"
    );
}

// ---------------------------------------------------------------------------
// Admin transfer tests (issue #650 — two-step admin transfer safety)
// ---------------------------------------------------------------------------

/// Full happy path: propose then accept hands control to the new admin.
#[test]
fn test_admin_transfer_succeeds_with_auth() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    // Step 1 — current admin proposes.
    client.propose_admin(&admin, &new_admin);
    // Step 2 — new admin accepts.
    client.accept_admin(&new_admin);

    // Verify: new admin can mint (fails with Unauthorized if transfer was wrong).
    let recipient = Address::generate(&env);
    let result = client.try_mint(&new_admin, &recipient, &1_000_i128);
    assert!(
        result.is_ok(),
        "new admin must be able to mint after successful two-step transfer"
    );
}

/// The original admin must lose minting rights after the transfer completes.
#[test]
fn test_old_admin_cannot_mint_after_transfer() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    let recipient = Address::generate(&env);
    let result = client.try_mint(&admin, &recipient, &1_000_i128);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "old admin must be rejected after admin transfer"
    );
}

/// An address that was not proposed cannot accept the admin slot.
#[test]
fn test_wrong_address_cannot_accept_admin() {
    let (env, contract_id, admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);

    let result = client.try_accept_admin(&impostor);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "an address that was not proposed must not be able to accept admin"
    );
}

/// Calling accept_admin when no proposal is pending returns NoPendingAdmin.
#[test]
fn test_accept_admin_without_proposal_fails() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let random = Address::generate(&env);

    let result = client.try_accept_admin(&random);
    assert_eq!(
        result,
        Err(Ok(TokenError::NoPendingAdmin)),
        "accepting admin with no pending proposal must return NoPendingAdmin"
    );
}

/// A non-admin cannot propose a new admin.
#[test]
fn test_non_admin_cannot_propose_admin() {
    let (env, contract_id, _admin, _treasury) = setup();
    let client = CHVTokenClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    let result = client.try_propose_admin(&attacker, &target);
    assert_eq!(
        result,
        Err(Ok(TokenError::Unauthorized)),
        "a non-admin address must not be able to propose a new admin"
    );
}
