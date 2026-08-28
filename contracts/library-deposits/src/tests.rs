use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, BytesN, Env,
};

use crate::{DepositError, DepositStatus, LibraryDeposits, LibraryDepositsClient};

struct Ctx {
    env: Env,
    contract: Address,
    admin: Address,
    treasury: Address,
    token: Address,
    patron: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let patron = Address::generate(&env);

    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();
    StellarAssetClient::new(&env, &token).mint(&patron, &100_000);

    let contract = env.register(LibraryDeposits, ());
    LibraryDepositsClient::new(&env, &contract).initialize(&admin, &treasury);

    Ctx { env, contract, admin, treasury, token, patron }
}

fn client(ctx: &Ctx) -> LibraryDepositsClient {
    LibraryDepositsClient::new(&ctx.env, &ctx.contract)
}

fn balance(ctx: &Ctx, account: &Address) -> i128 {
    TokenClient::new(&ctx.env, &ctx.token).balance(account)
}

fn loan_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[1u8; 32])
}

// ===== Positive paths =====

#[test]
fn test_lock_deposit_transfers_tokens_in() {
    let ctx = setup();
    let amount = 5_000_i128;
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &amount);
    assert_eq!(balance(&ctx, &ctx.patron), 95_000);
    assert_eq!(balance(&ctx, &ctx.contract), amount);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.original_amount, amount);
    assert_eq!(d.remaining_amount, amount);
    assert_eq!(d.status, DepositStatus::Locked);
    assert_eq!(d.loan_id, loan_id(&ctx.env));
}

#[test]
fn test_release_deposit_refunds_patron_fully() {
    let ctx = setup();
    let amount = 3_000_i128;
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &amount);
    client(&ctx).release_deposit(&ctx.patron, &id);
    assert_eq!(balance(&ctx, &ctx.patron), 100_000);
    assert_eq!(balance(&ctx, &ctx.contract), 0);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.status, DepositStatus::Released);
    assert_eq!(d.remaining_amount, 0);
}

#[test]
fn test_admin_can_release_deposit() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &2_000);
    client(&ctx).release_deposit(&ctx.admin, &id);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.status, DepositStatus::Released);
}

#[test]
fn test_partial_charge_moves_tokens_to_treasury() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &10_000);
    client(&ctx).partial_charge(&ctx.admin, &id, &3_000);
    assert_eq!(balance(&ctx, &ctx.treasury), 3_000);
    assert_eq!(balance(&ctx, &ctx.contract), 7_000);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.remaining_amount, 7_000);
    assert_eq!(d.status, DepositStatus::Locked);
}

#[test]
fn test_full_charge_closes_deposit_and_moves_tokens() {
    let ctx = setup();
    let amount = 4_000_i128;
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &amount);
    client(&ctx).full_charge(&ctx.admin, &id);
    assert_eq!(balance(&ctx, &ctx.treasury), amount);
    assert_eq!(balance(&ctx, &ctx.contract), 0);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.status, DepositStatus::Charged);
    assert_eq!(d.remaining_amount, 0);
}

#[test]
fn test_cumulative_partial_charges_exhaust_balance() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &9_000);
    client(&ctx).partial_charge(&ctx.admin, &id, &3_000);
    client(&ctx).partial_charge(&ctx.admin, &id, &3_000);
    client(&ctx).partial_charge(&ctx.admin, &id, &3_000);
    assert_eq!(balance(&ctx, &ctx.treasury), 9_000);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.remaining_amount, 0);
}

// ===== Authorization tests =====

#[test]
fn test_non_admin_cannot_partial_charge() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &5_000);
    let attacker = Address::generate(&ctx.env);
    assert_eq!(
        client(&ctx).try_partial_charge(&attacker, &id, &1_000),
        Err(Ok(DepositError::Unauthorized))
    );
}

#[test]
fn test_non_admin_non_patron_cannot_release() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &5_000);
    let stranger = Address::generate(&ctx.env);
    assert_eq!(
        client(&ctx).try_release_deposit(&stranger, &id),
        Err(Ok(DepositError::Unauthorized))
    );
}

#[test]
fn test_patron_cannot_full_charge() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &5_000);
    assert_eq!(
        client(&ctx).try_full_charge(&ctx.patron, &id),
        Err(Ok(DepositError::Unauthorized))
    );
}

// ===== Negative tests =====

#[test]
fn test_release_already_released_fails() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &1_000);
    client(&ctx).release_deposit(&ctx.patron, &id);
    assert_eq!(
        client(&ctx).try_release_deposit(&ctx.patron, &id),
        Err(Ok(DepositError::AlreadyClosed))
    );
}

#[test]
fn test_charge_after_release_fails() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &2_000);
    client(&ctx).release_deposit(&ctx.patron, &id);
    assert_eq!(
        client(&ctx).try_partial_charge(&ctx.admin, &id, &500),
        Err(Ok(DepositError::AlreadyClosed))
    );
}

#[test]
fn test_charge_after_full_charge_fails() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &2_000);
    client(&ctx).full_charge(&ctx.admin, &id);
    assert_eq!(
        client(&ctx).try_partial_charge(&ctx.admin, &id, &500),
        Err(Ok(DepositError::AlreadyClosed))
    );
}

#[test]
fn test_partial_charge_exceeding_balance_fails() {
    let ctx = setup();
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &1_000);
    assert_eq!(
        client(&ctx).try_partial_charge(&ctx.admin, &id, &1_001),
        Err(Ok(DepositError::InsufficientBalance))
    );
}

#[test]
fn test_zero_amount_lock_rejected() {
    let ctx = setup();
    assert_eq!(
        client(&ctx).try_lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &0),
        Err(Ok(DepositError::InvalidAmount))
    );
}

#[test]
fn test_negative_amount_lock_rejected() {
    let ctx = setup();
    assert_eq!(
        client(&ctx).try_lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &-1),
        Err(Ok(DepositError::InvalidAmount))
    );
}

#[test]
fn test_get_deposit_not_found() {
    let ctx = setup();
    let missing = BytesN::from_array(&ctx.env, &[99u8; 32]);
    assert_eq!(
        client(&ctx).try_get_deposit(&missing),
        Err(Ok(DepositError::NotFound))
    );
}

// ===== Boundary tests =====

#[test]
fn test_exact_remaining_partial_charge_succeeds() {
    let ctx = setup();
    let amount = 500_i128;
    let id = client(&ctx).lock_deposit(&ctx.patron, &loan_id(&ctx.env), &ctx.token, &amount);
    client(&ctx).partial_charge(&ctx.admin, &id, &amount);
    let d = client(&ctx).get_deposit(&id);
    assert_eq!(d.remaining_amount, 0);
    // partial_charge does not flip status to Charged — only full_charge does.
    assert_eq!(d.status, DepositStatus::Locked);
}

#[test]
fn test_two_deposits_same_loan_are_independent() {
    let ctx = setup();
    let lid = loan_id(&ctx.env);
    let id1 = client(&ctx).lock_deposit(&ctx.patron, &lid, &ctx.token, &1_000);
    let id2 = client(&ctx).lock_deposit(&ctx.patron, &lid, &ctx.token, &2_000);
    assert_ne!(id1, id2);
    client(&ctx).release_deposit(&ctx.patron, &id1);
    let d2 = client(&ctx).get_deposit(&id2);
    assert_eq!(d2.status, DepositStatus::Locked);
    assert_eq!(d2.remaining_amount, 2_000);
}

#[test]
fn test_initialize_twice_rejected() {
    let ctx = setup();
    assert_eq!(
        client(&ctx).try_initialize(&ctx.admin, &ctx.treasury),
        Err(Ok(DepositError::AlreadyInitialized))
    );
}
