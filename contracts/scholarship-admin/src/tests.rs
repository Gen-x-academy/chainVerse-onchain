#![cfg(test)]
// The crate is `no_std`; the test module needs `vec!` and `format!`.
extern crate std;
use std::format;

use crate::{
    ConfigKind, ContractError, Environment, Flag, FlagState, ScholarshipAdminContract,
    ScholarshipAdminContractClient,
};
use soroban_sdk::{
    testutils::Address as _, testutils::Events as _, Address, BytesN, Env, Symbol, Vec,
};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAdminContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin)
}

fn subject(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

fn ref_hash(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &{
        let mut a = [0u8; 32];
        a[0] = seed;
        a
    })
}

fn cohorts(env: &Env, names: &[&str]) -> Vec<Symbol> {
    let mut v = Vec::new(env);
    for n in names {
        v.push_back(Symbol::new(env, n));
    }
    v
}

// ═══════════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_initialize_is_not_repeatable() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_requires_auth() {
    let env = Env::default();
    let contract_id = env.register(ScholarshipAdminContract, ());
    let admin = Address::generate(&env);
    // No `mock_all_auths`, so `require_auth` is unsatisfied.
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert!(client.try_initialize(&admin).is_err());
}

#[test]
fn test_reads_on_uninitialized_contract_error() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(ScholarshipAdminContract, ());
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    // Rather than inventing an administrator to attribute a read to.
    assert_eq!(
        client.try_get_flag(&Flag::Discovery, &Environment::Production),
        Err(Ok(ContractError::NotInitialized))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Flags fail closed (#1144)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_unconfigured_flag_is_disabled_and_reads_off() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);

    // Fail closed, and answer rather than error: the caller needs a boolean.
    assert!(!client.is_enabled(
        &Flag::Discovery,
        &Environment::Production,
        &Symbol::new(&env, "anyone")
    ));
    assert!(!client.is_enabled(
        &Flag::Payouts,
        &Environment::Testnet,
        &Symbol::new(&env, "anyone")
    ));

    let config = client.get_flag(&Flag::Discovery, &Environment::Production);
    assert_eq!(config.state, FlagState::Off);
    assert_eq!(config.version, 0);
    assert_eq!(config.effective_at, 0);
}

#[test]
fn test_every_surface_fails_closed_before_configuration() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for flag in [
        Flag::Discovery,
        Flag::Applications,
        Flag::Reviews,
        Flag::Awards,
        Flag::Payouts,
    ] {
        for environment in [
            Environment::Testnet,
            Environment::Staging,
            Environment::Production,
        ] {
            assert!(
                !client.is_enabled(&flag, &environment, &Symbol::new(&env, "cohort_a")),
                "{flag:?}/{environment:?} must fail closed"
            );
        }
    }
}

#[test]
fn test_flag_discriminants_are_fixed() {
    // A stored flag is identified by its discriminant, so reordering the
    // enum must not be able to silently reinterpret what is on-chain: a
    // stored `Applications` must keep meaning applications forever.
    assert_eq!(
        [
            Flag::Discovery as u32,
            Flag::Applications as u32,
            Flag::Reviews as u32,
            Flag::Awards as u32,
            Flag::Payouts as u32,
        ],
        [1, 2, 3, 4, 5]
    );
}

#[test]
fn test_flag_event_names_are_distinct() {
    // Two flags sharing an event name would make the audit stream ambiguous
    // about which surface changed.
    let (env, _contract_id, _admin) = setup();
    let flags = Flag::all();
    for i in 0..flags.len() {
        for j in (i + 1)..flags.len() {
            assert_ne!(
                flags[i].name(&env),
                flags[j].name(&env),
                "{:?} and {:?} share a name",
                flags[i],
                flags[j]
            );
        }
    }
}

#[test]
fn test_flag_names_are_stable_strings() {
    let (env, _contract_id, _admin) = setup();
    // Spelled out rather than derived, so the audit stream is readable
    // without a discriminant-to-name table.
    assert_eq!(Flag::Discovery.name(&env), Symbol::new(&env, "discovery"));
    assert_eq!(Flag::Payouts.name(&env), Symbol::new(&env, "payouts"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Three-state semantics (#1144)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_off_enables_nobody() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Applications,
        &Environment::Production,
        &FlagState::Off,
        &Vec::new(&env),
    );
    assert!(!client.is_enabled(
        &Flag::Applications,
        &Environment::Production,
        &Symbol::new(&env, "anyone")
    ));
}

#[test]
fn test_on_enables_any_cohort() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Reviews,
        &Environment::Staging,
        &FlagState::On,
        &Vec::new(&env),
    );
    assert!(client.is_enabled(
        &Flag::Reviews,
        &Environment::Staging,
        &Symbol::new(&env, "anyone")
    ));
    assert!(client.is_enabled(
        &Flag::Reviews,
        &Environment::Staging,
        &Symbol::new(&env, "other")
    ));
}

#[test]
fn test_cohort_allowlist_admits_only_listed_cohorts() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Discovery,
        &Environment::Production,
        &FlagState::CohortsOnly,
        &cohorts(&env, &["early_partners", "pilot_universities"]),
    );
    assert!(client.is_enabled(
        &Flag::Discovery,
        &Environment::Production,
        &Symbol::new(&env, "early_partners")
    ));
    assert!(client.is_enabled(
        &Flag::Discovery,
        &Environment::Production,
        &Symbol::new(&env, "pilot_universities")
    ));
    // The point of the third state.
    assert!(!client.is_enabled(
        &Flag::Discovery,
        &Environment::Production,
        &Symbol::new(&env, "everyone_else")
    ));
}

#[test]
fn test_environments_are_independent() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Discovery,
        &Environment::Testnet,
        &FlagState::CohortsOnly,
        &cohorts(&env, &["test_cohort"]),
    );
    // A testnet allowlist must not leak into production.
    assert!(client.is_enabled(
        &Flag::Discovery,
        &Environment::Testnet,
        &Symbol::new(&env, "test_cohort")
    ));
    assert!(!client.is_enabled(
        &Flag::Discovery,
        &Environment::Production,
        &Symbol::new(&env, "test_cohort")
    ));
    assert!(!client.is_enabled(
        &Flag::Discovery,
        &Environment::Staging,
        &Symbol::new(&env, "test_cohort")
    ));
}

#[test]
fn test_surfaces_are_independent() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Discovery,
        &Environment::Production,
        &FlagState::On,
        &Vec::new(&env),
    );
    // Discovery on must not imply applications on.
    assert!(!client.is_enabled(
        &Flag::Applications,
        &Environment::Production,
        &Symbol::new(&env, "c")
    ));
    assert!(!client.is_enabled(
        &Flag::Awards,
        &Environment::Production,
        &Symbol::new(&env, "c")
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// Cohort list validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_cohort_allowlist_cannot_be_empty_when_declared() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    // `CohortsOnly` with no cohorts would be `Off` under another name.
    assert_eq!(
        client.try_set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::CohortsOnly,
            &Vec::new(&env),
        ),
        Err(Ok(ContractError::InvalidCohorts))
    );
}

#[test]
fn test_duplicate_cohort_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::CohortsOnly,
            &cohorts(&env, &["dup", "dup"]),
        ),
        Err(Ok(ContractError::InvalidCohorts))
    );
}

#[test]
fn test_empty_cohort_name_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::CohortsOnly,
            &cohorts(&env, &[""]),
        ),
        Err(Ok(ContractError::InvalidCohorts))
    );
}

#[test]
fn test_cohort_list_is_bounded() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let mut many = Vec::new(&env);
    for i in 0..33u32 {
        many.push_back(Symbol::new(&env, &format!("c{i}")));
    }
    assert_eq!(
        client.try_set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::CohortsOnly,
            &many,
        ),
        Err(Ok(ContractError::TooManyCohorts))
    );

    // Exactly at the ceiling is accepted.
    let mut at_limit = Vec::new(&env);
    for i in 0..32u32 {
        at_limit.push_back(Symbol::new(&env, &format!("c{i}")));
    }
    assert_eq!(
        client.set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::CohortsOnly,
            &at_limit,
        ),
        1
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Authorization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_non_admin_cannot_set_a_flag() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let impostor = Address::generate(&env);
    assert_eq!(
        client.try_set_flag(
            &impostor,
            &Flag::Payouts,
            &Environment::Production,
            &FlagState::On,
            &Vec::new(&env),
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_non_admin_cannot_set_config() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let impostor = Address::generate(&env);
    assert_eq!(
        client.try_set_config(
            &impostor,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &1,
            &2,
            &ref_hash(&env, 9),
            &Symbol::new(&env, "l"),
        ),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn test_non_admin_cannot_roll_back_config() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &10,
        &20,
        &ref_hash(&env, 9),
        &Symbol::new(&env, "l"),
    );
    let impostor = Address::generate(&env);
    assert_eq!(
        client.try_rollback_config(&impostor, &ConfigKind::Limits, &subject(&env, 1), &1),
        Err(Ok(ContractError::NotAdmin))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Versioning and exposure auditability (#1144)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_flag_versions_increment_and_are_retained() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);

    assert_eq!(
        client.set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::On,
            &Vec::new(&env)
        ),
        1
    );
    assert_eq!(
        client.set_flag(
            &admin,
            &Flag::Discovery,
            &Environment::Production,
            &FlagState::Off,
            &Vec::new(&env)
        ),
        2
    );

    // History is legible, so "what was exposed when" is answerable.
    let first = client.flag_version(&Flag::Discovery, &Environment::Production, &1);
    assert_eq!(first.state, FlagState::On);
    let second = client.flag_version(&Flag::Discovery, &Environment::Production, &2);
    assert_eq!(second.state, FlagState::Off);
}

#[test]
fn test_absent_flag_version_is_reported() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_flag_version(&Flag::Discovery, &Environment::Production, &1),
        Err(Ok(ContractError::VersionNotFound))
    );
    assert_eq!(
        client.try_flag_version(&Flag::Discovery, &Environment::Production, &0),
        Err(Ok(ContractError::VersionNotFound))
    );
}

#[test]
fn test_flag_history_is_bounded_and_floor_advances() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for _ in 0..33 {
        client.set_flag(
            &admin,
            &Flag::Awards,
            &Environment::Production,
            &FlagState::On,
            &Vec::new(&env),
        );
    }
    // 32 retained; version 1 evicted, floor reported so an auditor knows
    // where the log begins.
    assert_eq!(
        client.flag_version_floor(&Flag::Awards, &Environment::Production),
        2
    );
    assert_eq!(
        client.try_flag_version(&Flag::Awards, &Environment::Production, &1),
        Err(Ok(ContractError::VersionNotFound))
    );
    // The current version is never a candidate for eviction.
    assert!(client
        .try_flag_version(&Flag::Awards, &Environment::Production, &33)
        .is_ok());
}

#[test]
fn test_exposure_surface_is_listable_in_one_pass() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_flag(
        &admin,
        &Flag::Discovery,
        &Environment::Production,
        &FlagState::On,
        &Vec::new(&env),
    );
    client.set_flag(
        &admin,
        &Flag::Payouts,
        &Environment::Production,
        &FlagState::CohortsOnly,
        &cohorts(&env, &["finance"]),
    );

    let all = client.list_flags(&Environment::Production);
    assert_eq!(all.len(), 5);
    // Discovery On, Payouts cohort-only, everything else untouched/Off.
    assert_eq!(all.get(0).unwrap().state, FlagState::On);
    assert_eq!(all.get(1).unwrap().state, FlagState::Off);
    assert_eq!(all.get(2).unwrap().state, FlagState::Off);
    assert_eq!(all.get(3).unwrap().state, FlagState::Off);
    assert_eq!(all.get(4).unwrap().state, FlagState::CohortsOnly);
}

// ═══════════════════════════════════════════════════════════════════════════
// Pins: rollback must not corrupt in-flight work (#1144)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_pin_records_the_decision_a_workflow_started_with() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let workflow = subject(&env, 7);

    client.set_flag(
        &admin,
        &Flag::Payouts,
        &Environment::Production,
        &FlagState::On,
        &Vec::new(&env),
    );
    assert_eq!(
        client.pin(&Flag::Payouts, &Environment::Production, &workflow),
        FlagState::On
    );

    // An in-flight workflow keeps its decision when the flag moves.
    client.set_flag(
        &admin,
        &Flag::Payouts,
        &Environment::Production,
        &FlagState::Off,
        &Vec::new(&env),
    );
    assert!(!client.is_enabled(
        &Flag::Payouts,
        &Environment::Production,
        &Symbol::new(&env, "c")
    ));
    assert_eq!(
        client.pinned_state(&Flag::Payouts, &Environment::Production, &workflow),
        FlagState::On
    );
}

#[test]
fn test_pin_of_an_unconfigured_flag_records_off() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.pin(
            &Flag::Discovery,
            &Environment::Production,
            &subject(&env, 3)
        ),
        FlagState::Off
    );
}

#[test]
fn test_missing_pin_is_reported() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_pinned_state(&Flag::Payouts, &Environment::Production, &subject(&env, 8)),
        Err(Ok(ContractError::PinNotFound))
    );
}

#[test]
fn test_pins_are_counted_once_per_workflow() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let workflow = subject(&env, 4);
    for _ in 0..5 {
        client.pin(&Flag::Payouts, &Environment::Production, &workflow);
    }
    // Re-pinning must not ratchet the counter toward the ceiling.
    assert_eq!(client.tracked_pins(), 1);
}

#[test]
fn test_pin_table_is_bounded() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for i in 0..5_000u32 {
        let mut raw = [0u8; 32];
        raw[..4].copy_from_slice(&i.to_be_bytes());
        client.pin(
            &Flag::Payouts,
            &Environment::Production,
            &BytesN::from_array(&env, &raw),
        );
    }
    // The table saturates rather than growing without limit.
    assert!(client.tracked_pins() <= 5_000);
    assert_eq!(client.tracked_pins(), 5_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Configuration validation (#1145)
// ═══════════════════════════════════════════════════════════════════════════

/// Every invalid configuration, and the error the write path must produce.
/// The same table drives the preview assertions below, so the two paths
/// cannot drift apart.
fn invalid_cases(env: &Env) -> std::vec::Vec<(ConfigKind, i128, i128, BytesN<32>, ContractError)> {
    std::vec![
        // Limits: zero, negative, and a maximum below its minimum.
        (
            ConfigKind::Limits,
            0,
            10,
            ref_hash(env, 1),
            ContractError::InvalidLimit
        ),
        (
            ConfigKind::Limits,
            -1,
            10,
            ref_hash(env, 1),
            ContractError::InvalidLimit
        ),
        (
            ConfigKind::Limits,
            10,
            9,
            ref_hash(env, 1),
            ContractError::InvalidLimit
        ),
        // Deadline: an end before its start, or a zero bound.
        (
            ConfigKind::Deadline,
            100,
            99,
            ref_hash(env, 1),
            ContractError::InvalidDeadline
        ),
        (
            ConfigKind::Deadline,
            0,
            100,
            ref_hash(env, 1),
            ContractError::InvalidDeadline
        ),
        (
            ConfigKind::Deadline,
            100,
            0,
            ref_hash(env, 1),
            ContractError::InvalidDeadline
        ),
        (
            ConfigKind::Deadline,
            -5,
            100,
            ref_hash(env, 1),
            ContractError::InvalidDeadline
        ),
        // Provider: an all-zero reference is a provider with no credential
        // reference, discovered only when the first payment fails.
        (
            ConfigKind::Provider,
            1,
            1,
            BytesN::from_array(env, &[0u8; 32]),
            ContractError::EmptyReference
        ),
        // Asset: a non-positive amount.
        (
            ConfigKind::Asset,
            0,
            1,
            ref_hash(env, 1),
            ContractError::InvalidLimit
        ),
        (
            ConfigKind::Asset,
            -1,
            1,
            ref_hash(env, 1),
            ContractError::InvalidLimit
        ),
        // Risk threshold: negative floor, or an inverted band.
        (
            ConfigKind::RiskThreshold,
            -1,
            10,
            ref_hash(env, 1),
            ContractError::InvalidThreshold
        ),
        (
            ConfigKind::RiskThreshold,
            10,
            9,
            ref_hash(env, 1),
            ContractError::InvalidThreshold
        ),
    ]
}

#[test]
fn test_invalid_configuration_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for (kind, a, b, reference, expected) in invalid_cases(&env) {
        assert_eq!(
            client.try_set_config(
                &admin,
                &kind,
                &subject(&env, 1),
                &a,
                &b,
                &reference,
                &Symbol::new(&env, "x")
            ),
            Err(Ok(expected)),
            "{kind:?}({a}, {b}) must be rejected"
        );
    }
}

#[test]
fn test_preview_agrees_with_the_write_path_exactly() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for (kind, a, b, reference, expected) in invalid_cases(&env) {
        assert_eq!(
            client.try_preview_config(&kind, &a, &b, &reference),
            Err(Ok(expected)),
            "preview must return what the write would return"
        );
        assert_eq!(
            client.try_set_config(
                &admin,
                &kind,
                &subject(&env, 1),
                &a,
                &b,
                &reference,
                &Symbol::new(&env, "x")
            ),
            Err(Ok(expected))
        );
    }
}

#[test]
fn test_preview_accepts_what_the_write_accepts() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let valid = std::vec![
        (ConfigKind::Limits, 1i128, 10i128, ref_hash(&env, 0)),
        (ConfigKind::Limits, 10, 10, ref_hash(&env, 0)),
        (ConfigKind::Deadline, 100, 100, ref_hash(&env, 0)),
        (ConfigKind::Provider, 1, 1, ref_hash(&env, 1)),
        (ConfigKind::Asset, 1, 0, ref_hash(&env, 0)),
        // A template carries only a label and a reference.
        (
            ConfigKind::Template,
            0,
            0,
            BytesN::from_array(&env, &[0u8; 32])
        ),
        (ConfigKind::RiskThreshold, 0, 0, ref_hash(&env, 0)),
        (ConfigKind::RiskThreshold, 5, 5, ref_hash(&env, 0)),
    ];
    for (kind, a, b, reference) in valid {
        assert_eq!(
            client.try_preview_config(&kind, &a, &b, &reference),
            Ok(Ok(()))
        );
    }
}

#[test]
fn test_preview_writes_nothing() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.preview_config(&ConfigKind::Limits, &1, &10, &ref_hash(&env, 1));
    assert!(client
        .try_preview_config(&ConfigKind::Limits, &0, &10, &ref_hash(&env, 1))
        .is_err());
    assert_eq!(client.tracked_config_keys(), 0);
    assert_eq!(
        client.try_get_config(&ConfigKind::Limits, &subject(&env, 1)),
        Err(Ok(ContractError::ConfigNotFound))
    );
}

#[test]
fn test_valid_configuration_round_trips() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.set_config(
            &admin,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &100,
            &1_000,
            &ref_hash(&env, 5),
            &Symbol::new(&env, "per_program"),
        ),
        1
    );
    let stored = client.get_config(&ConfigKind::Limits, &subject(&env, 1));
    assert_eq!(stored.a, 100);
    assert_eq!(stored.b, 1_000);
    assert_eq!(stored.label, Symbol::new(&env, "per_program"));
    assert_eq!(stored.version, 1);
    assert_eq!(stored.updated_by, admin);
}

#[test]
fn test_provider_stores_a_reference_not_a_secret() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let reference = ref_hash(&env, 42);
    client.set_config(
        &admin,
        &ConfigKind::Provider,
        &subject(&env, 1),
        &1,
        &1,
        &reference,
        &Symbol::new(&env, "payments_api"),
    );
    // What comes back is the reference that was supplied, and nothing else.
    // A credential is never an input here, so there is nothing to leak.
    let stored = client.get_config(&ConfigKind::Provider, &subject(&env, 1));
    assert_eq!(stored.ref_hash, reference);
    let listed = client.list_config(&ConfigKind::Provider);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap().ref_hash, reference);
}

// ═══════════════════════════════════════════════════════════════════════════
// Config versioning and rollback (#1145)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_config_versions_increment_and_are_retained() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.set_config(
            &admin,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &10,
            &20,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "a")
        ),
        1
    );
    assert_eq!(
        client.set_config(
            &admin,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &30,
            &40,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "b")
        ),
        2
    );

    assert_eq!(
        client
            .config_version(&ConfigKind::Limits, &subject(&env, 1), &1)
            .a,
        10
    );
    assert_eq!(
        client
            .config_version(&ConfigKind::Limits, &subject(&env, 1), &2)
            .a,
        30
    );
    assert_eq!(
        client.get_config(&ConfigKind::Limits, &subject(&env, 1)).a,
        30
    );
}

#[test]
fn test_config_history_is_bounded_and_floor_advances() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for i in 0..33i128 {
        client.set_config(
            &admin,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &(i + 1),
            &1000,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "l"),
        );
    }
    assert_eq!(
        client.config_version_floor(&ConfigKind::Limits, &subject(&env, 1)),
        2
    );
    assert_eq!(
        client.try_config_version(&ConfigKind::Limits, &subject(&env, 1), &1),
        Err(Ok(ContractError::VersionNotFound))
    );
    assert!(client
        .try_config_version(&ConfigKind::Limits, &subject(&env, 1), &33)
        .is_ok());
}

#[test]
fn test_rollback_appends_rather_than_rewinding() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &10,
        &20,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "v1"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &30,
        &40,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "v2"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &50,
        &60,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "v3"),
    );

    // Restoring version 1 appends as version 4.
    assert_eq!(
        client.rollback_config(&admin, &ConfigKind::Limits, &subject(&env, 1), &1),
        4
    );
    let current = client.get_config(&ConfigKind::Limits, &subject(&env, 1));
    assert_eq!(current.a, 10);
    assert_eq!(current.version, 4);
    // History stayed append-only: versions 2 and 3 are still legible, which
    // is the whole point of not rewinding the counter.
    assert_eq!(
        client
            .config_version(&ConfigKind::Limits, &subject(&env, 1), &2)
            .a,
        30
    );
    assert_eq!(
        client
            .config_version(&ConfigKind::Limits, &subject(&env, 1), &3)
            .a,
        50
    );
    assert_eq!(
        client
            .config_version(&ConfigKind::Limits, &subject(&env, 1), &4)
            .a,
        10
    );
}

#[test]
fn test_rollback_does_not_overwrite_an_existing_version() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for v in 1..=5i128 {
        client.set_config(
            &admin,
            &ConfigKind::Asset,
            &subject(&env, 2),
            &v,
            &1,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "a"),
        );
    }
    // Rolling back to version 2 must not write over version 3.
    assert_eq!(
        client.rollback_config(&admin, &ConfigKind::Asset, &subject(&env, 2), &2),
        6
    );
    // Version 3 still holds what was written to it.
    assert_eq!(
        client
            .config_version(&ConfigKind::Asset, &subject(&env, 2), &3)
            .a,
        3
    );
    assert_eq!(
        client
            .config_version(&ConfigKind::Asset, &subject(&env, 2), &6)
            .a,
        2
    );
}

#[test]
fn test_rollback_to_absent_version_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &10,
        &20,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "a"),
    );
    assert_eq!(
        client.try_rollback_config(&admin, &ConfigKind::Limits, &subject(&env, 1), &99),
        Err(Ok(ContractError::VersionNotFound))
    );
}

#[test]
fn test_rollback_of_absent_config_is_rejected() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(
        client.try_rollback_config(&admin, &ConfigKind::Limits, &subject(&env, 99), &1),
        Err(Ok(ContractError::VersionNotFound))
    );
}

#[test]
fn test_rollback_leaves_an_in_flight_workflow_intact() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    let workflow = subject(&env, 6);
    client.set_flag(
        &admin,
        &Flag::Awards,
        &Environment::Production,
        &FlagState::On,
        &Vec::new(&env),
    );
    client.pin(&Flag::Awards, &Environment::Production, &workflow);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &10,
        &20,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "v1"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &99,
        &199,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "v2"),
    );

    // Turn the feature off and roll the bad limit back.
    client.set_flag(
        &admin,
        &Flag::Awards,
        &Environment::Production,
        &FlagState::Off,
        &Vec::new(&env),
    );
    client.rollback_config(&admin, &ConfigKind::Limits, &subject(&env, 1), &1);

    // Future reads see the restored state; the in-flight workflow does not.
    assert_eq!(
        client.get_config(&ConfigKind::Limits, &subject(&env, 1)).a,
        10
    );
    assert_eq!(
        client.pinned_state(&Flag::Awards, &Environment::Production, &workflow),
        FlagState::On
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Listing and bounds
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_list_config_is_scoped_to_one_kind() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &1,
        &2,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "a"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 2),
        &3,
        &4,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "b"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Deadline,
        &subject(&env, 3),
        &5,
        &6,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "c"),
    );

    assert_eq!(client.list_config(&ConfigKind::Limits).len(), 2);
    assert_eq!(client.list_config(&ConfigKind::Deadline).len(), 1);
    assert_eq!(client.list_config(&ConfigKind::Provider).len(), 0);
    assert_eq!(client.tracked_config_keys(), 3);
}

#[test]
fn test_updating_a_key_does_not_duplicate_its_index_entry() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    for _ in 0..5 {
        client.set_config(
            &admin,
            &ConfigKind::Limits,
            &subject(&env, 1),
            &1,
            &2,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "a"),
        );
    }
    assert_eq!(client.list_config(&ConfigKind::Limits).len(), 1);
    assert_eq!(client.tracked_config_keys(), 1);
}

#[test]
fn test_config_key_table_is_bounded() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    // Distinct subjects, up to and past the ceiling.
    let mut last = None;
    for i in 0..2_001u32 {
        let mut raw = [0u8; 32];
        raw[..4].copy_from_slice(&i.to_be_bytes());
        let s = BytesN::from_array(&env, &raw);
        last = Some(client.try_set_config(
            &admin,
            &ConfigKind::Limits,
            &s,
            &1,
            &2,
            &ref_hash(&env, 1),
            &Symbol::new(&env, "l"),
        ));
    }
    assert_eq!(client.tracked_config_keys(), 2_000);
    assert_eq!(last, Some(Err(Ok(ContractError::TooManyConfigKeys))));
}

#[test]
fn test_version_is_reported() {
    let (env, contract_id, _admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    assert_eq!(client.version(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Events
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_flag_change_publishes_the_version_it_replaced() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    // `Events::all()` reports the most recent invocation, so each write is
    // checked on its own: one event per change, from this contract.
    client.set_flag(
        &admin,
        &Flag::Payouts,
        &Environment::Production,
        &FlagState::On,
        &Vec::new(&env),
    );
    let first = env.events().all();
    assert_eq!(first.len(), 1, "one event per write");
    assert_eq!(&first.get(0).unwrap().0, &contract_id);

    client.set_flag(
        &admin,
        &Flag::Payouts,
        &Environment::Production,
        &FlagState::Off,
        &Vec::new(&env),
    );
    assert_eq!(env.events().all().len(), 1);
}

#[test]
fn test_config_change_publishes_an_event() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    // Checked per invocation, since `Events::all()` reports only the most
    // recent one.
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &1,
        &2,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "a"),
    );
    assert_eq!(env.events().all().len(), 1);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &3,
        &4,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "b"),
    );
    assert_eq!(env.events().all().len(), 1);
}

#[test]
fn test_rollback_publishes_its_own_event() {
    let (env, contract_id, admin) = setup();
    let client = ScholarshipAdminContractClient::new(&env, &contract_id);
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &1,
        &2,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "a"),
    );
    client.set_config(
        &admin,
        &ConfigKind::Limits,
        &subject(&env, 1),
        &3,
        &4,
        &ref_hash(&env, 1),
        &Symbol::new(&env, "b"),
    );
    // The rollback is itself auditable, not a silent edit.
    client.rollback_config(&admin, &ConfigKind::Limits, &subject(&env, 1), &1);
    assert_eq!(env.events().all().len(), 1);
}
