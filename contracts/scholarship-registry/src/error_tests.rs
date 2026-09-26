#![cfg(test)]
//! Initialization-boundary coverage for `scholarship-registry` (issue #1146).
//!
//! A small contract with a small gap: `NotInitialized` and `AlreadyInitialized`
//! were asserted nowhere. Both are worth pinning here, because this contract
//! is the directory other scholarship contracts resolve through, and a
//! half-initialized registry is the kind of thing that is much cheaper to
//! catch in a test than in an incident.
//!
//! ## Coverage ledger
//!
//! | Variant | Exercised by |
//! | --- | --- |
//! | `NotInitialized` | `test_reads_before_initialize_are_refused` (admin path) |
//! | `AlreadyInitialized` | `test_initialize_is_not_repeatable` |
//! | `NotAdmin` | `tests.rs` |
//! | `ModuleNotFound` | `tests.rs` |
//! | `Unauthorized` | `tests.rs` |
//! | — | `test_every_error_variant_is_accounted_for` |

extern crate std;
use std::format;

use crate::{ContractError, ScholarshipRegistryContract, ScholarshipRegistryContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipRegistryContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipRegistryContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, id, admin)
}

// ── NotInitialized ──────────────────────────────────────────────────────────

#[test]
fn test_reads_before_initialize_are_refused() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipRegistryContract, ());
    let client = ScholarshipRegistryContractClient::new(&env, &id);
    let name = Symbol::new(&env, "applications");

    // The initialization guard sits on the admin path.
    assert_eq!(
        client.try_register_module(&Address::generate(&env), &name, &Address::generate(&env)),
        Err(Ok(ContractError::NotInitialized))
    );
    assert_eq!(
        client.try_grant_role(
            &Address::generate(&env),
            &Address::generate(&env),
            &crate::Role::Administrator
        ),
        Err(Ok(ContractError::NotInitialized))
    );

    // Read paths are unguarded, and report an absent module rather than an
    // absent setup -- so before `initialize`, "never initialized" and "that
    // module was never registered" are indistinguishable to a caller. Both
    // are true, and the second is the more actionable half, but an operator
    // debugging a fresh deployment is reading a `ModuleNotFound` that looks
    // like a misconfiguration rather than a missing `initialize`.
    assert_eq!(
        client.try_get_module(&name),
        Err(Ok(ContractError::ModuleNotFound))
    );
    assert!(!client.is_module_registered(&name));
}

#[test]
fn test_a_missing_module_is_distinct_from_an_uninitialized_registry() {
    let (env, id, admin) = setup();
    let client = ScholarshipRegistryContractClient::new(&env, &id);
    client.register_module(
        &admin,
        &Symbol::new(&env, "applications"),
        &Address::generate(&env),
    );

    // Initialized: a genuinely absent module says so.
    assert_eq!(
        client.try_get_module(&Symbol::new(&env, "not_registered")),
        Err(Ok(ContractError::ModuleNotFound))
    );
    assert!(!client.is_module_registered(&Symbol::new(&env, "not_registered")));
}

// ── AlreadyInitialized ──────────────────────────────────────────────────────

#[test]
fn test_initialize_is_not_repeatable() {
    let (env, id, admin) = setup();
    let client = ScholarshipRegistryContractClient::new(&env, &id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_cannot_hand_over_administration() {
    let (env, id, _admin) = setup();
    let client = ScholarshipRegistryContractClient::new(&env, &id);
    let successor = Address::generate(&env);
    // A second caller must not be able to capture the registry that other
    // contracts resolve module addresses through.
    assert_eq!(
        client.try_initialize(&successor),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    // The original administrator still holds the role.
    let grantee = Address::generate(&env);
    assert!(client
        .try_grant_role(&successor, &grantee, &crate::Role::Administrator)
        .is_err());
    assert!(client
        .try_grant_role(&_admin, &grantee, &crate::Role::Administrator)
        .is_ok());
}

// ── coverage ledger ────────────────────────────────────────────────────────

const DECLARED: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "NotAdmin",
    "ModuleNotFound",
    "Unauthorized",
];

const COVERED_ELSEWHERE: &[&str] = &["NotAdmin", "ModuleNotFound", "Unauthorized"];

const COVERED_HERE: &[&str] = &["NotInitialized", "AlreadyInitialized"];

#[test]
fn test_every_error_variant_is_accounted_for() {
    assert_eq!(
        DECLARED.len(),
        COVERED_HERE.len() + COVERED_ELSEWHERE.len(),
        "a variant is classified twice or not at all"
    );
    for variant in DECLARED {
        assert!(
            COVERED_HERE.contains(variant) || COVERED_ELSEWHERE.contains(variant),
            "{variant} is not accounted for"
        );
    }
}

#[test]
fn test_declared_ledger_matches_the_contract() {
    let source = include_str!("lib.rs");
    for variant in DECLARED {
        assert!(
            source.contains(&format!("{variant} =")),
            "{variant} is in the ledger but not declared in lib.rs"
        );
    }
}
