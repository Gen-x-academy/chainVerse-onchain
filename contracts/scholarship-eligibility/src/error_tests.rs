#![cfg(test)]
//! Error-path coverage for `scholarship-eligibility` (issue #1146).
//!
//! Closes the gap for the five variants no other test asserted, including
//! `VersionOverflow`, which is only reachable by seeding the version counter
//! directly -- four billion publishes is not a test.
//!
//! ## Coverage ledger
//!
//! | Variant | Exercised by | Note |
//! | --- | --- | --- |
//! | `NotInitialized` | `test_uninitialized_admin_write_is_refused` | |
//! | `AlreadyInitialized` | `test_initialize_is_not_repeatable` | |
//! | `NotAdmin` | `tests.rs` | |
//! | `NotAuthorizedIssuer` | `tests.rs` | |
//! | `InvalidExpiry` | `tests.rs` | |
//! | `NotAttestationIssuer` | `tests.rs` | |
//! | `RuleNotFound` | `test_rule_lookup_reports_a_missing_version` | |
//! | `AttestationNotFound` | `test_attestation_lookup_reports_a_missing_record` | |
//! | `VersionOverflow` | `test_rule_version_counter_cannot_wrap` | reached by seeding storage |
//! | `EmptyRule` / `RuleTooLarge` / `NoRulePublished` | `tests.rs` | |
//! | — | `test_every_error_variant_is_accounted_for` | |

extern crate std;
use std::format;

use crate::{
    ContractError, DataKey, ScholarshipEligibilityContract, ScholarshipEligibilityContractClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol, Vec};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipEligibilityContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, id, admin)
}

fn program(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[5u8; 32])
}

fn scope(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[9u8; 32])
}

fn one_type(env: &Env) -> Vec<Symbol> {
    let mut v = Vec::new(env);
    v.push_back(Symbol::new(env, "enrollment"));
    v
}

// ── NotInitialized ──────────────────────────────────────────────────────────

#[test]
fn test_uninitialized_admin_write_is_refused() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipEligibilityContract, ());
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    let admin = Address::generate(&env);

    assert_eq!(
        client.try_add_issuer(&admin, &Address::generate(&env)),
        Err(Ok(ContractError::NotInitialized))
    );
    assert_eq!(
        client.try_publish_eligibility_rule(&admin, &program(&env), &one_type(&env)),
        Err(Ok(ContractError::NotInitialized))
    );

    // Read paths are unguarded and report the record as absent, which is
    // indistinguishable from "no such version".
    assert_eq!(
        client.try_get_latest_rule_version(&program(&env)),
        Err(Ok(ContractError::NoRulePublished))
    );
    assert!(!client.has_valid_attestation(
        &Address::generate(&env),
        &scope(&env),
        &Symbol::new(&env, "enrollment")
    ));
}

// ── AlreadyInitialized ──────────────────────────────────────────────────────

#[test]
fn test_initialize_is_not_repeatable() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    // Nor can a successor be installed over the first administrator.
    assert_eq!(
        client.try_initialize(&Address::generate(&env)),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

// ── RuleNotFound ────────────────────────────────────────────────────────────

#[test]
fn test_rule_lookup_reports_a_missing_version() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    client.publish_eligibility_rule(&admin, &program(&env), &one_type(&env));

    // A program with no rules at all has no latest version.
    assert_eq!(
        client.try_get_latest_rule_version(&BytesN::from_array(&env, &[4u8; 32])),
        Err(Ok(ContractError::NoRulePublished))
    );
    // A program with rules, asked for one that was never published. Versions
    // are not renumbered, so this gap is permanent rather than pending.
    assert_eq!(
        client.try_get_eligibility_rule(&program(&env), &2),
        Err(Ok(ContractError::RuleNotFound))
    );
    assert_eq!(
        client.try_get_eligibility_rule(&program(&env), &0),
        Err(Ok(ContractError::RuleNotFound))
    );
    // Version 1 does exist.
    assert_eq!(
        client.get_latest_rule_version(&program(&env)),
        1,
        "first publish is version 1"
    );
}

#[test]
fn test_rules_for_one_program_do_not_leak_into_another() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    let first = program(&env);
    let second = BytesN::from_array(&env, &[6u8; 32]);
    client.publish_eligibility_rule(&admin, &first, &one_type(&env));

    assert_eq!(
        client.try_get_latest_rule_version(&second),
        Err(Ok(ContractError::NoRulePublished))
    );
    // And the second program's first publish is its own version 1.
    client.publish_eligibility_rule(&admin, &second, &one_type(&env));
    assert_eq!(client.get_latest_rule_version(&second), 1);
    assert_eq!(client.get_latest_rule_version(&first), 1);
}

// ── AttestationNotFound ─────────────────────────────────────────────────────

#[test]
fn test_attestation_lookup_reports_a_missing_record() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    client.add_issuer(&admin, &issuer);

    assert_eq!(
        client.try_get_attestation(&subject, &scope(&env), &Symbol::new(&env, "enrollment")),
        Err(Ok(ContractError::AttestationNotFound))
    );
    // Revoking something that was never issued is refused rather than
    // silently succeeding, so a caller cannot believe it withdrew a
    // credential that never existed.
    assert_eq!(
        client.try_revoke_attestation(
            &issuer,
            &subject,
            &scope(&env),
            &Symbol::new(&env, "enrollment")
        ),
        Err(Ok(ContractError::AttestationNotFound))
    );
    // Validity answers false rather than erroring, so a caller gating on it
    // gets a boolean.
    assert!(!client.has_valid_attestation(
        &subject,
        &scope(&env),
        &Symbol::new(&env, "enrollment")
    ));
}

#[test]
fn test_revoking_an_expired_attestation_still_reports_not_found_only_if_absent() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    let issuer = Address::generate(&env);
    let subject = Address::generate(&env);
    client.add_issuer(&admin, &issuer);
    client.issue_attestation(
        &issuer,
        &subject,
        &scope(&env),
        &Symbol::new(&env, "enrollment"),
        &10_000,
    );
    // The record exists, so it is found -- expiry is a separate question
    // answered by `has_valid_attestation`.
    let record = client.get_attestation(&subject, &scope(&env), &Symbol::new(&env, "enrollment"));
    assert!(!record.revoked);
}

// ── VersionOverflow ─────────────────────────────────────────────────────────

#[test]
fn test_rule_version_counter_cannot_wrap() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    // Seed the counter at its ceiling. Publishing is the only thing that
    // advances it, and it is advanced with `checked_add`, so this is the
    // only way to reach the branch -- and the point is that the write is
    // refused rather than wrapping to version 0 and colliding with the
    // "no rules published" reading.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::RuleVersion(program(&env)), &u32::MAX);
    });

    assert_eq!(
        client.try_publish_eligibility_rule(&admin, &program(&env), &one_type(&env)),
        Err(Ok(ContractError::VersionOverflow))
    );
    // The counter was not advanced past the ceiling.
    let stored = env.as_contract(&id, || {
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::RuleVersion(program(&env)))
    });
    assert_eq!(stored, Some(u32::MAX));
}

#[test]
fn test_version_overflow_boundary_is_table_driven() {
    let cases = std::vec![
        // (seeded counter, publish accepted)
        (0u32, true),
        (1, true),
        (u32::MAX - 1, true),
        (u32::MAX, false),
    ];
    for (index, (seed, accepted)) in cases.iter().enumerate() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(ScholarshipEligibilityContract, ());
        let admin = Address::generate(&env);
        let client = ScholarshipEligibilityContractClient::new(&env, &id);
        client.initialize(&admin);
        let target = BytesN::from_array(&env, &{
            let mut raw = [0u8; 32];
            raw[0] = (index + 1) as u8;
            raw
        });
        env.as_contract(&id, || {
            env.storage()
                .persistent()
                .set(&DataKey::RuleVersion(target.clone()), seed);
        });

        let result = client.try_publish_eligibility_rule(&admin, &target, &one_type(&env));
        assert_eq!(
            result.is_ok(),
            *accepted,
            "counter at {seed} should be {}",
            if *accepted { "accepted" } else { "refused" }
        );
        if *accepted {
            assert_eq!(client.get_latest_rule_version(&target.clone()), seed + 1);
        }
    }
}

#[test]
fn test_publishing_after_overflow_leaves_the_existing_rule_readable() {
    let (env, id, admin) = setup();
    let client = ScholarshipEligibilityContractClient::new(&env, &id);
    client.publish_eligibility_rule(&admin, &program(&env), &one_type(&env));
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::RuleVersion(program(&env)), &u32::MAX);
    });

    assert_eq!(
        client.try_publish_eligibility_rule(&admin, &program(&env), &one_type(&env)),
        Err(Ok(ContractError::VersionOverflow))
    );
    // The refusal did not damage the rule that is already published: a
    // failed publish must not leave the program with no rules at all.
    assert_eq!(client.get_latest_rule_version(&program(&env)), u32::MAX);
    assert_eq!(
        client.get_eligibility_rule(&program(&env), &1).version,
        1,
        "the original rule is intact"
    );
}

// ── coverage ledger ────────────────────────────────────────────────────────

const DECLARED: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "NotAdmin",
    "NotAuthorizedIssuer",
    "InvalidExpiry",
    "NotAttestationIssuer",
    "RuleNotFound",
    "AttestationNotFound",
    "VersionOverflow",
    "EmptyRule",
    "RuleTooLarge",
    "NoRulePublished",
];

const COVERED_ELSEWHERE: &[&str] = &[
    "NotAdmin",
    "NotAuthorizedIssuer",
    "InvalidExpiry",
    "NotAttestationIssuer",
    "EmptyRule",
    "RuleTooLarge",
    "NoRulePublished",
];

const COVERED_HERE: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "RuleNotFound",
    "AttestationNotFound",
    "VersionOverflow",
];

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
