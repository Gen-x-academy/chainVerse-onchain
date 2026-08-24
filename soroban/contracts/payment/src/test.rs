//! Unit tests for the ChainVerse payment configuration contract.
//!
//! Coverage:
//! - Positive paths: all public methods succeed under valid inputs.
//! - Negative paths: every error variant is exercised at least once.
//!
//! Uses Soroban's native test environment (`Env::default()`).
#![cfg(test)]

extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

use crate::{ContractError, PaymentContract, PaymentContractClient};

// ─── Helper: extract inner error from try_* call ─────────────────────────────

/// Unwrap the inner `ContractError` from a `try_*` result.
///
/// In soroban-sdk 21.x, `try_method()` returns
/// `Result<Result<T, ConvErr>, Result<ContractError, InvokeError>>`.
/// For a contract-level error the outer `Result` is `Err(Ok(ContractError))`.
macro_rules! contract_err {
    ($r:expr) => {
        match $r {
            Err(Ok(e)) => e,
            other => panic!("expected contract error, got {:?}", other),
        }
    };
}

// ─── Fixture ─────────────────────────────────────────────────────────────────

#[allow(dead_code)]
struct Fixture {
    env: Env,
    contract: Address,
    admin: Address,
    treasury: Address,
    asset: Address,
    asset2: Address,
}

impl Fixture {
    /// Create a fresh Env, register the contract, and initialise it.
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let asset = Address::generate(&env);
        let asset2 = Address::generate(&env);

        let contract = env.register_contract(None, PaymentContract {});
        let client = PaymentContractClient::new(&env, &contract);

        client.initialize(&admin, &treasury, &500u32, &86_400u64);

        Fixture {
            env,
            contract,
            admin,
            treasury,
            asset,
            asset2,
        }
    }

    fn client(&self) -> PaymentContractClient<'_> {
        PaymentContractClient::new(&self.env, &self.contract)
    }

    fn course_id(&self) -> Symbol {
        Symbol::new(&self.env, "RUST101")
    }

    fn instructor(&self) -> Address {
        Address::generate(&self.env)
    }
}

// ─── Initialisation tests ────────────────────────────────────────────────────

#[test]
fn test_initialize_success() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    client.initialize(&admin, &treasury, &500u32, &86_400u64);

    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_fee(), 500u32);
}

#[test]
fn test_initialize_already_initialized_fails() {
    let f = Fixture::new();
    let new_admin = Address::generate(&f.env);
    let new_treasury = Address::generate(&f.env);

    let err = contract_err!(f
        .client()
        .try_initialize(&new_admin, &new_treasury, &100u32, &0u64));
    assert_eq!(err, ContractError::AlreadyInitialized);
}

#[test]
fn test_initialize_fee_too_high_fails() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    let err = contract_err!(client.try_initialize(&admin, &treasury, &2_001u32, &0u64));
    assert_eq!(err, ContractError::InvalidFee);
}

#[test]
fn test_initialize_max_fee_allowed() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    // 2000 bps (20 %) is the boundary – must succeed.
    client.initialize(&admin, &treasury, &2_000u32, &0u64);
    assert_eq!(client.get_fee(), 2_000u32);
}

// ─── Admin setter tests ───────────────────────────────────────────────────────

#[test]
fn test_set_admin_success() {
    let f = Fixture::new();
    let new_admin = Address::generate(&f.env);

    f.client().set_admin(&f.admin, &new_admin);

    assert_eq!(f.client().get_admin(), new_admin);
}

#[test]
fn test_set_admin_not_admin_fails() {
    let f = Fixture::new();
    let impostor = Address::generate(&f.env);
    let new_admin = Address::generate(&f.env);

    let err = contract_err!(f.client().try_set_admin(&impostor, &new_admin));
    assert_eq!(err, ContractError::NotAdmin);
    // Storage must be unchanged.
    assert_eq!(f.client().get_admin(), f.admin);
}

// ─── Treasury setter tests ────────────────────────────────────────────────────

#[test]
fn test_set_treasury_success() {
    let f = Fixture::new();
    let new_treasury = Address::generate(&f.env);

    f.client().set_treasury(&f.admin, &new_treasury);

    assert_eq!(f.client().get_treasury(), new_treasury);
}

#[test]
fn test_set_treasury_not_admin_fails() {
    let f = Fixture::new();
    let impostor = Address::generate(&f.env);
    let new_treasury = Address::generate(&f.env);

    let err = contract_err!(f.client().try_set_treasury(&impostor, &new_treasury));
    assert_eq!(err, ContractError::NotAdmin);
}

// ─── Fee setter tests ─────────────────────────────────────────────────────────

#[test]
fn test_set_fee_success() {
    let f = Fixture::new();

    f.client().set_fee(&f.admin, &1_000u32);

    assert_eq!(f.client().get_fee(), 1_000u32);
}

#[test]
fn test_set_fee_too_high_fails() {
    let f = Fixture::new();

    let err = contract_err!(f.client().try_set_fee(&f.admin, &2_001u32));
    assert_eq!(err, ContractError::InvalidFee);
    // Fee must be unchanged.
    assert_eq!(f.client().get_fee(), 500u32);
}

#[test]
fn test_set_fee_not_admin_fails() {
    let f = Fixture::new();
    let impostor = Address::generate(&f.env);

    let err = contract_err!(f.client().try_set_fee(&impostor, &100u32));
    assert_eq!(err, ContractError::NotAdmin);
}

// ─── Asset CRUD tests ─────────────────────────────────────────────────────────

#[test]
fn test_add_asset_enabled_success() {
    let f = Fixture::new();

    f.client().add_asset(&f.admin, &f.asset, &true);

    assert!(f.client().is_asset_enabled(&f.asset));
    let config = f.client().get_asset_config(&f.asset).unwrap();
    assert!(config.enabled);
    assert_eq!(config.asset, f.asset);
}

#[test]
fn test_add_asset_disabled_success() {
    let f = Fixture::new();

    f.client().add_asset(&f.admin, &f.asset, &false);

    assert!(!f.client().is_asset_enabled(&f.asset));
}

#[test]
fn test_add_asset_not_admin_fails() {
    let f = Fixture::new();
    let impostor = Address::generate(&f.env);

    let err = contract_err!(f.client().try_add_asset(&impostor, &f.asset, &true));
    assert_eq!(err, ContractError::NotAdmin);
}

#[test]
fn test_enable_asset_success() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &false);

    f.client().enable_asset(&f.admin, &f.asset);

    assert!(f.client().is_asset_enabled(&f.asset));
}

#[test]
fn test_enable_asset_not_found_fails() {
    let f = Fixture::new();
    let unknown = Address::generate(&f.env);

    let err = contract_err!(f.client().try_enable_asset(&f.admin, &unknown));
    assert_eq!(err, ContractError::AssetNotFound);
}

#[test]
fn test_disable_asset_success() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);

    f.client().disable_asset(&f.admin, &f.asset);

    assert!(!f.client().is_asset_enabled(&f.asset));
}

#[test]
fn test_disable_asset_not_found_fails() {
    let f = Fixture::new();
    let unknown = Address::generate(&f.env);

    let err = contract_err!(f.client().try_disable_asset(&f.admin, &unknown));
    assert_eq!(err, ContractError::AssetNotFound);
}

#[test]
fn test_disable_asset_not_admin_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let impostor = Address::generate(&f.env);

    let err = contract_err!(f.client().try_disable_asset(&impostor, &f.asset));
    assert_eq!(err, ContractError::NotAdmin);
}

#[test]
fn test_get_asset_config_returns_none_for_unknown() {
    let f = Fixture::new();
    let unknown = Address::generate(&f.env);

    assert!(f.client().get_asset_config(&unknown).is_none());
}

// ─── Course CRUD tests ────────────────────────────────────────────────────────

fn setup_course(f: &Fixture) -> (Symbol, Address) {
    f.client().add_asset(&f.admin, &f.asset, &true);
    let instructor = f.instructor();
    let course_id = f.course_id();
    f.client().add_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &100u32,
        &true,
    );
    (course_id, instructor)
}

#[test]
fn test_add_course_success() {
    let f = Fixture::new();
    let (course_id, instructor) = setup_course(&f);

    let config = f.client().get_course_config(&course_id).unwrap();
    assert_eq!(config.course_id, course_id);
    assert_eq!(config.price, 1_000_000i128);
    assert_eq!(config.asset, f.asset);
    assert_eq!(config.instructor, instructor);
    assert_eq!(config.fee_bps, 100u32);
    assert!(config.active);
}

#[test]
fn test_add_course_not_admin_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let impostor = Address::generate(&f.env);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &impostor,
        &course_id,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::NotAdmin);
}

#[test]
fn test_add_course_zero_price_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &f.admin,
        &course_id,
        &0i128,
        &f.asset,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::InvalidAmount);
}

#[test]
fn test_add_course_negative_price_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &f.admin,
        &course_id,
        &(-1i128),
        &f.asset,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::InvalidAmount);
}

#[test]
fn test_add_course_disabled_asset_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &false);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::AssetNotEnabled);
}

#[test]
fn test_add_course_unregistered_asset_fails() {
    let f = Fixture::new();
    let unknown = Address::generate(&f.env);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &unknown,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::AssetNotEnabled);
}

#[test]
fn test_add_course_excessive_fee_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let course_id = f.course_id();
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_add_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &2_001u32,
        &true,
    ));
    assert_eq!(err, ContractError::InvalidFee);
}

#[test]
fn test_update_course_success() {
    let f = Fixture::new();
    let (course_id, _) = setup_course(&f);
    f.client().add_asset(&f.admin, &f.asset2, &true);
    let new_instructor = f.instructor();

    f.client().update_course(
        &f.admin,
        &course_id,
        &2_000_000i128,
        &f.asset2,
        &new_instructor,
        &200u32,
        &false,
    );

    let config = f.client().get_course_config(&course_id).unwrap();
    assert_eq!(config.price, 2_000_000i128);
    assert_eq!(config.asset, f.asset2);
    assert_eq!(config.instructor, new_instructor);
    assert_eq!(config.fee_bps, 200u32);
    assert!(!config.active);
}

#[test]
fn test_update_course_not_found_fails() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let missing = Symbol::new(&f.env, "MISSING");
    let instructor = f.instructor();

    let err = contract_err!(f.client().try_update_course(
        &f.admin,
        &missing,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::CourseNotFound);
}

#[test]
fn test_update_course_asset_change_to_disabled_fails() {
    let f = Fixture::new();
    let (course_id, instructor) = setup_course(&f);
    // asset2 is NOT added, so it's not enabled.

    let err = contract_err!(f.client().try_update_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &f.asset2,
        &instructor,
        &100u32,
        &true,
    ));
    assert_eq!(err, ContractError::AssetNotEnabled);
    // Original config must remain unchanged.
    let config = f.client().get_course_config(&course_id).unwrap();
    assert_eq!(config.asset, f.asset);
}

#[test]
fn test_activate_course_success() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    let course_id = f.course_id();
    let instructor = f.instructor();
    // Add the course as inactive.
    f.client().add_course(
        &f.admin,
        &course_id,
        &1_000_000i128,
        &f.asset,
        &instructor,
        &0u32,
        &false,
    );

    f.client().activate_course(&f.admin, &course_id);

    assert!(f.client().is_course_active(&course_id));
}

#[test]
fn test_deactivate_course_success() {
    let f = Fixture::new();
    let (course_id, _) = setup_course(&f);

    f.client().deactivate_course(&f.admin, &course_id);

    assert!(!f.client().is_course_active(&course_id));
}

#[test]
fn test_activate_course_not_found_fails() {
    let f = Fixture::new();
    let missing = Symbol::new(&f.env, "MISSING");

    let err = contract_err!(f.client().try_activate_course(&f.admin, &missing));
    assert_eq!(err, ContractError::CourseNotFound);
}

#[test]
fn test_deactivate_course_not_found_fails() {
    let f = Fixture::new();
    let missing = Symbol::new(&f.env, "MISSING");

    let err = contract_err!(f.client().try_deactivate_course(&f.admin, &missing));
    assert_eq!(err, ContractError::CourseNotFound);
}

#[test]
fn test_activate_course_not_admin_fails() {
    let f = Fixture::new();
    let (course_id, _) = setup_course(&f);
    let impostor = Address::generate(&f.env);

    let err = contract_err!(f.client().try_activate_course(&impostor, &course_id));
    assert_eq!(err, ContractError::NotAdmin);
}

// ─── Query tests ──────────────────────────────────────────────────────────────

#[test]
fn test_get_course_config_none_for_unknown() {
    let f = Fixture::new();
    let unknown = Symbol::new(&f.env, "UNKNOWN");

    assert!(f.client().get_course_config(&unknown).is_none());
}

#[test]
fn test_is_course_active_false_for_unknown() {
    let f = Fixture::new();
    let unknown = Symbol::new(&f.env, "UNKNOWN");

    assert!(!f.client().is_course_active(&unknown));
}

#[test]
fn test_version() {
    use std::string::ToString;
    let f = Fixture::new();
    // version() returns a soroban String; compare via alloc String conversion.
    let ver = f.client().version();
    let ver_str = ver.to_string();
    assert_eq!(ver_str, "1.0.0");
}

// ─── Not-initialized guard ────────────────────────────────────────────────────

#[test]
fn test_get_admin_not_initialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    let err = contract_err!(client.try_get_admin());
    assert_eq!(err, ContractError::NotInitialized);
}

#[test]
fn test_get_treasury_not_initialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    let err = contract_err!(client.try_get_treasury());
    assert_eq!(err, ContractError::NotInitialized);
}

#[test]
fn test_get_fee_not_initialized_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register_contract(None, PaymentContract {});
    let client = PaymentContractClient::new(&env, &contract);

    let err = contract_err!(client.try_get_fee());
    assert_eq!(err, ContractError::NotInitialized);
}

// ─── Round-trip tests ─────────────────────────────────────────────────────────

#[test]
fn test_asset_config_round_trip() {
    let f = Fixture::new();

    f.client().add_asset(&f.admin, &f.asset, &true);
    f.client().disable_asset(&f.admin, &f.asset);
    f.client().enable_asset(&f.admin, &f.asset);

    assert!(f.client().is_asset_enabled(&f.asset));
    let config = f.client().get_asset_config(&f.asset).unwrap();
    assert_eq!(config.asset, f.asset);
    assert!(config.enabled);
}

#[test]
fn test_course_config_round_trip() {
    let f = Fixture::new();
    f.client().add_asset(&f.admin, &f.asset, &true);
    f.client().add_asset(&f.admin, &f.asset2, &true);
    let course_id = f.course_id();
    let instructor = f.instructor();
    let new_instructor = f.instructor();

    // Add → update → deactivate → activate
    f.client().add_course(
        &f.admin,
        &course_id,
        &500_000i128,
        &f.asset,
        &instructor,
        &50u32,
        &false,
    );

    f.client().update_course(
        &f.admin,
        &course_id,
        &999_999i128,
        &f.asset2,
        &new_instructor,
        &200u32,
        &false,
    );

    f.client().activate_course(&f.admin, &course_id);
    assert!(f.client().is_course_active(&course_id));

    f.client().deactivate_course(&f.admin, &course_id);
    assert!(!f.client().is_course_active(&course_id));

    let config = f.client().get_course_config(&course_id).unwrap();
    assert_eq!(config.price, 999_999i128);
    assert_eq!(config.asset, f.asset2);
    assert_eq!(config.instructor, new_instructor);
    assert_eq!(config.fee_bps, 200u32);
    assert!(!config.active);
}

#[test]
fn test_admin_transfer_round_trip() {
    let f = Fixture::new();
    let new_admin = Address::generate(&f.env);

    f.client().set_admin(&f.admin, &new_admin);

    // Original admin can no longer perform admin actions.
    let err = contract_err!(f.client().try_set_fee(&f.admin, &100u32));
    assert_eq!(err, ContractError::NotAdmin);

    // New admin can.
    f.client().set_fee(&new_admin, &100u32);
    assert_eq!(f.client().get_fee(), 100u32);
}
