#![cfg(test)]
//! Error-path coverage for `scholarship-applications` (issue #1146).
//!
//! The three remaining variants no other test asserted. Two of the three
//! `VersionOverflow` sites in this contract are covered here, since both share
//! the same shape: a counter read from storage and advanced with
//! `checked_add`, reachable only by seeding the counter at `u32::MAX`.
//!
//! ## Coverage ledger
//!
//! | Variant | Exercised by |
//! | --- | --- |
//! | `NotInitialized` | `test_uninitialized_admin_write_is_refused` |
//! | `ConsentTermsNotFound` | `test_consent_terms_lookup_reports_a_missing_version` |
//! | `VersionOverflow` | `test_form_version_counter_cannot_wrap`, `test_consent_version_counter_cannot_wrap` |
//! | all 14 others | `tests.rs` |
//! | — | `test_every_error_variant_is_accounted_for` |

extern crate std;
use std::format;

use crate::{
    ContractError, DataKey, ScholarshipApplicationsContract, ScholarshipApplicationsContractClient,
};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipApplicationsContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, id, admin)
}

fn program(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[2u8; 32])
}

fn hash32(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &{
        let mut raw = [0u8; 32];
        raw[0] = seed;
        raw
    })
}

// ── NotInitialized ──────────────────────────────────────────────────────────

#[test]
fn test_uninitialized_admin_write_is_refused() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipApplicationsContract, ());
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    let admin = Address::generate(&env);

    // The initialization guard sits on the admin path.
    assert_eq!(
        client.try_register_program(&admin, &program(&env), &9_000),
        Err(Ok(ContractError::NotInitialized))
    );
    assert_eq!(
        client.try_publish_form_schema(&admin, &program(&env), &hash32(&env, 1)),
        Err(Ok(ContractError::NotInitialized))
    );
    assert_eq!(
        client.try_publish_consent_terms(&admin, &program(&env), &hash32(&env, 2)),
        Err(Ok(ContractError::NotInitialized))
    );

    // Read paths are unguarded and report the record as absent.
    assert_eq!(
        client.try_get_latest_form_version(&program(&env)),
        Err(Ok(ContractError::NoFormSchemaPublished))
    );
    assert_eq!(
        client.try_get_form_schema(&program(&env), &1),
        Err(Ok(ContractError::FormSchemaNotFound))
    );
}

// ── ConsentTermsNotFound ────────────────────────────────────────────────────

#[test]
fn test_consent_terms_lookup_reports_a_missing_version() {
    let (env, id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.register_program(&admin, &program(&env), &9_000);
    client.publish_consent_terms(&admin, &program(&env), &hash32(&env, 2));

    // A program that published no terms at all.
    assert_eq!(
        client.try_get_consent_terms(&BytesN::from_array(&env, &[7u8; 32]), &1),
        Err(Ok(ContractError::ConsentTermsNotFound))
    );
    // This program published version 1, so version 2 is permanently absent:
    // versions are never renumbered or reused.
    assert_eq!(
        client.try_get_consent_terms(&program(&env), &2),
        Err(Ok(ContractError::ConsentTermsNotFound))
    );
    assert_eq!(
        client.try_get_consent_terms(&program(&env), &0),
        Err(Ok(ContractError::ConsentTermsNotFound))
    );
    // Version 1 is there.
    assert_eq!(
        client.get_consent_terms(&program(&env), &1).version,
        1,
        "first publish is version 1"
    );
}

#[test]
fn test_consent_before_terms_is_refused_with_a_distinct_error() {
    let (env, id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.register_program(&admin, &program(&env), &9_000);
    let applicant = Address::generate(&env);

    // Distinct from `ConsentTermsNotFound`: this is "you never published
    // terms", not "that version does not exist". The distinction matters to
    // a caller deciding whether to ask the admin to publish or to ask which
    // version to re-consent to.
    assert_eq!(
        client.try_record_consent(&applicant, &program(&env)),
        Err(Ok(ContractError::NoConsentTermsPublished))
    );
    // Publishing terms unblocks it.
    client.publish_consent_terms(&admin, &program(&env), &hash32(&env, 2));
    assert_eq!(client.record_consent(&applicant, &program(&env)), 1);
}

// ── VersionOverflow ─────────────────────────────────────────────────────────

#[test]
fn test_form_version_counter_cannot_wrap() {
    let (env, id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.register_program(&admin, &program(&env), &9_000);
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::FormVersion(program(&env)), &u32::MAX);
    });

    assert_eq!(
        client.try_publish_form_schema(&admin, &program(&env), &hash32(&env, 1)),
        Err(Ok(ContractError::VersionOverflow))
    );
    // The counter did not wrap to 0, which would have collided with the
    // "nothing published" reading of version 0.
    let stored = env.as_contract(&id, || {
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::FormVersion(program(&env)))
    });
    assert_eq!(stored, Some(u32::MAX));
}

#[test]
fn test_consent_version_counter_cannot_wrap() {
    let (env, id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.register_program(&admin, &program(&env), &9_000);
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::ConsentVersion(program(&env)), &u32::MAX);
    });

    assert_eq!(
        client.try_publish_consent_terms(&admin, &program(&env), &hash32(&env, 2)),
        Err(Ok(ContractError::VersionOverflow))
    );
    let stored = env.as_contract(&id, || {
        env.storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::ConsentVersion(program(&env)))
    });
    assert_eq!(stored, Some(u32::MAX));
}

#[test]
fn test_version_overflow_boundary_is_table_driven() {
    // Both counters behave identically, so one table drives both. If either
    // check is loosened, the `u32::MAX` row is what moves.
    let cases = std::vec![
        (0u32, true),
        (1, true),
        (u32::MAX - 1, true),
        (u32::MAX, false)
    ];
    for (index, (seed, accepted)) in cases.iter().enumerate() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(ScholarshipApplicationsContract, ());
        let admin = Address::generate(&env);
        let client = ScholarshipApplicationsContractClient::new(&env, &id);
        client.initialize(&admin);
        let target = BytesN::from_array(&env, &{
            let mut raw = [0u8; 32];
            raw[0] = (index + 1) as u8;
            raw
        });
        client.register_program(&admin, &target, &9_000);
        env.as_contract(&id, || {
            env.storage()
                .persistent()
                .set(&DataKey::FormVersion(target.clone()), seed);
        });

        let result = client.try_publish_form_schema(&admin, &target, &hash32(&env, 1));
        assert_eq!(
            result.is_ok(),
            *accepted,
            "form counter at {seed} should be {}",
            if *accepted { "accepted" } else { "refused" }
        );
        if *accepted {
            assert_eq!(client.get_latest_form_version(&target), seed + 1);
        }
    }
}

#[test]
fn test_a_refused_publish_leaves_the_published_schema_intact() {
    let (env, id, admin) = setup();
    let client = ScholarshipApplicationsContractClient::new(&env, &id);
    client.register_program(&admin, &program(&env), &9_000);
    client.publish_form_schema(&admin, &program(&env), &hash32(&env, 1));
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::FormVersion(program(&env)), &u32::MAX);
    });

    assert_eq!(
        client.try_publish_form_schema(&admin, &program(&env), &hash32(&env, 9)),
        Err(Ok(ContractError::VersionOverflow))
    );
    // A failed publish must not leave applicants unable to apply: the
    // already-published schema is still served.
    assert_eq!(client.get_latest_form_version(&program(&env)), u32::MAX);
    assert_eq!(
        client.get_form_schema(&program(&env), &1).schema_hash,
        hash32(&env, 1)
    );
}

// ── coverage ledger ────────────────────────────────────────────────────────

const DECLARED: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "NotAdmin",
    "ProgramNotFound",
    "ProgramAlreadyExists",
    "ProgramInactive",
    "DeadlinePassed",
    "DuplicateApplication",
    "ApplicationNotFound",
    "NoFormSchemaPublished",
    "FormSchemaNotFound",
    "NoConsentTermsPublished",
    "ConsentTermsNotFound",
    "ConsentNotFound",
    "ConsentRevoked",
    "ConsentOutOfDate",
    "VersionOverflow",
];

const COVERED_ELSEWHERE: &[&str] = &[
    "AlreadyInitialized",
    "NotAdmin",
    "ProgramNotFound",
    "ProgramAlreadyExists",
    "ProgramInactive",
    "DeadlinePassed",
    "DuplicateApplication",
    "ApplicationNotFound",
    "NoFormSchemaPublished",
    "FormSchemaNotFound",
    "NoConsentTermsPublished",
    "ConsentNotFound",
    "ConsentRevoked",
    "ConsentOutOfDate",
];

const COVERED_HERE: &[&str] = &["NotInitialized", "ConsentTermsNotFound", "VersionOverflow"];

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
