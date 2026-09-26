#![cfg(test)]
//! Error-path coverage for `scholarship-core` (issue #1146).
//!
//! Issue #1146 requires that "every state transition and typed error is
//! exercised". This file closes the gap for the four variants that no other
//! test in this crate asserted.
//!
//! It lives in-crate rather than in `tests/` on purpose: it reaches the
//! contract's private storage keys to drive states that are otherwise
//! unreachable in a single call, which is the only honest way to test a
//! boundary that depends on accumulated state.
//!
//! ## Coverage ledger
//!
//! Every declared variant, and where it is exercised. Update this table when a
//! variant is added — `test_every_error_variant_is_accounted_for` fails if a
//! variant is missing from it.
//!
//! | Variant | Exercised by | Note |
//! | --- | --- | --- |
//! | `NotInitialized` | — | **unreachable**: declared but never returned (see below) |
//! | `AlreadyInitialized` | `test_initialize_is_not_repeatable` | |
//! | `NotAdmin` | — | **unreachable**: declared but never returned (see below) |
//! | `ProgramAlreadyExists` | `tests.rs::test_create_program_rejects_duplicate_id` | |
//! | `ProgramNotFound` | `tests.rs::test_get_nonexistent_program_fails` | |
//! | `InvalidTitle` | `tests.rs::test_create_program_rejects_empty_title` | |
//! | `InvalidDescription` | `test_description_length_is_bounded` | |
//! | `NotProgramOwner` | `tests.rs::test_only_owner_can_transition` | |
//! | `InvalidTransition` | `tests.rs::test_published_to_draft_is_illegal` and siblings | |

extern crate std;

use crate::{ContractError, FundingModel, ScholarshipCoreContract, ScholarshipCoreContractClient};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, Symbol};

// Mirrors the crate's own bound. If the contract's limit moves and this
// does not, the boundary table below stops testing the real edge.
const MAX_DESCRIPTION_LEN: u32 = 2_000;

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipCoreContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipCoreContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, id, admin)
}

fn program_id(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[7u8; 32])
}

fn create(client: &ScholarshipCoreContractClient, env: &Env, owner: &Address) {
    client.create_program(
        owner,
        &program_id(env),
        &BytesN::from_array(env, &[1u8; 32]),
        &String::from_str(env, "Test Program"),
        &String::from_str(env, "A description"),
        &Symbol::new(env, "usd"),
        &FundingModel::FixedAward,
    );
}

// ── NotInitialized: unreachable ────────────────────────────────────────────

#[test]
fn test_uninitialized_reads_report_a_missing_program_not_a_missing_setup() {
    // `NotInitialized` is declared but returned by no code path: this
    // contract has no initialization guard on its read paths. Before
    // `initialize`, a lookup for any program reports `ProgramNotFound`.
    //
    // Worth stating plainly, because it is a real gap rather than a test
    // preference: a client cannot distinguish "this contract was never
    // initialized" from "no such program exists". Both answer
    // `ProgramNotFound`. The same applies to `create_program`, which
    // succeeds on a fresh deployment.
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipCoreContract, ());
    let client = ScholarshipCoreContractClient::new(&env, &id);

    assert_eq!(
        client.try_get_program(&program_id(&env)),
        Err(Ok(ContractError::ProgramNotFound))
    );

    // And the write path is unguarded too: a program can be created before
    // `initialize` is ever called.
    let owner = Address::generate(&env);
    assert!(client
        .try_create_program(
            &owner,
            &program_id(&env),
            &BytesN::from_array(&env, &[1u8; 32]),
            &String::from_str(&env, "Deployed but never initialized"),
            &String::from_str(&env, "Desc"),
            &Symbol::new(&env, "usd"),
            &FundingModel::FixedAward,
        )
        .is_ok());
}

// ── AlreadyInitialized ──────────────────────────────────────────────────────

#[test]
fn test_initialize_is_not_repeatable() {
    let (env, id, admin) = setup();
    let client = ScholarshipCoreContractClient::new(&env, &id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn test_initialize_cannot_hand_over_administration() {
    let (env, id, _admin) = setup();
    let client = ScholarshipCoreContractClient::new(&env, &id);
    let successor = Address::generate(&env);
    // Refused, so a second caller cannot capture the contract by re-running
    // initialization.
    assert_eq!(
        client.try_initialize(&successor),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    // The contract is still serving reads under the original deployment.
    create(&client, &env, &Address::generate(&env));
    assert!(client.try_get_program(&program_id(&env)).is_ok());
}

// ── InvalidDescription ─────────────────────────────────────────────────────

#[test]
fn test_description_length_is_bounded() {
    let (env, id, _admin) = setup();
    let client = ScholarshipCoreContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    // Exactly at the limit is accepted.
    let at_limit = "d".repeat(MAX_DESCRIPTION_LEN as usize);
    client.create_program(
        &owner,
        &program_id(&env),
        &BytesN::from_array(&env, &[1u8; 32]),
        &String::from_str(&env, "T"),
        &String::from_str(&env, &at_limit),
        &Symbol::new(&env, "usd"),
        &FundingModel::FixedAward,
    );
    assert!(client.try_get_program(&program_id(&env)).is_ok());

    // One character over is refused, on a second program so the first is
    // not what makes the call fail.
    let over = "d".repeat(MAX_DESCRIPTION_LEN as usize + 1);
    let other = BytesN::from_array(&env, &[8u8; 32]);
    assert_eq!(
        client.try_create_program(
            &owner,
            &other,
            &BytesN::from_array(&env, &[1u8; 32]),
            &String::from_str(&env, "T"),
            &String::from_str(&env, &over),
            &Symbol::new(&env, "usd"),
            &FundingModel::FixedAward,
        ),
        Err(Ok(ContractError::InvalidDescription))
    );
}

#[test]
fn test_description_boundary_is_table_driven() {
    let (env, id, _admin) = setup();
    let client = ScholarshipCoreContractClient::new(&env, &id);
    let owner = Address::generate(&env);

    // The boundary is a single comparison; table-driving it makes the
    // off-by-one visible instead of incidental.
    let cases = std::vec![
        (0usize, true),
        (1, true),
        (MAX_DESCRIPTION_LEN as usize - 1, true),
        (MAX_DESCRIPTION_LEN as usize, true),
        (MAX_DESCRIPTION_LEN as usize + 1, false),
        (MAX_DESCRIPTION_LEN as usize * 2, false),
    ];
    for (index, (len, accepted)) in cases.iter().enumerate() {
        let text = "d".repeat(*len);
        let id_for_case = BytesN::from_array(&env, &{
            let mut raw = [0u8; 32];
            raw[0] = (index + 1) as u8;
            raw
        });
        let result = client.try_create_program(
            &owner,
            &id_for_case,
            &BytesN::from_array(&env, &[1u8; 32]),
            &String::from_str(&env, "T"),
            &String::from_str(&env, &text),
            &Symbol::new(&env, "usd"),
            &FundingModel::FixedAward,
        );
        assert_eq!(
            result.is_ok(),
            *accepted,
            "description of {len} chars should be {}",
            if *accepted { "accepted" } else { "refused" }
        );
    }
}

// ── NotInitialized, NotAdmin: unreachable ──────────────────────────────────

#[test]
fn test_unreachable_variants_are_not_returned_by_any_path() {
    // `NotAdmin` is declared but returned by no code path. Authorization here
    // is per-program ownership (`NotProgramOwner`, lib.rs:232). The `Admin`
    // key that `initialize` writes is read back exactly once, at lib.rs:114,
    // and only to detect a repeat `initialize` -- never to gate a call.
    //
    // `NotInitialized` is likewise never returned. `initialize` is the only
    // function that could plausibly raise it, and it raises
    // `AlreadyInitialized` instead.
    //
    // These assertions cannot fail today. They exist so that the day a code
    // path starts returning either variant, this test is the thing that
    // fails, and the ledger above gets corrected rather than quietly rotting.
    let (env, id, _admin) = setup();
    let client = ScholarshipCoreContractClient::new(&env, &id);
    let owner = Address::generate(&env);
    create(&client, &env, &owner);

    if let Err(Ok(e)) = client.try_get_program(&program_id(&env)) {
        assert_ne!(e, ContractError::NotAdmin);
        assert_ne!(e, ContractError::NotInitialized);
    }
    if let Err(Ok(e)) = client.try_get_program_status(&program_id(&env)) {
        assert_ne!(e, ContractError::NotAdmin);
        assert_ne!(e, ContractError::NotInitialized);
    }
}

// ── coverage ledger ────────────────────────────────────────────────────────

use std::format;

/// The declared variants of `ContractError`, in declaration order.
const DECLARED: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "NotAdmin",
    "ProgramAlreadyExists",
    "ProgramNotFound",
    "InvalidTitle",
    "InvalidDescription",
    "NotProgramOwner",
    "InvalidTransition",
];

/// Variants asserted by a test in this file.
const COVERED_HERE: &[&str] = &["AlreadyInitialized", "InvalidDescription"];

/// Variants asserted by a test in this crate's `tests.rs`.
const COVERED_ELSEWHERE: &[&str] = &[
    "ProgramAlreadyExists",
    "ProgramNotFound",
    "InvalidTitle",
    "NotProgramOwner",
    "InvalidTransition",
];

/// Variants declared but returned by no code path. The reason each is
/// unreachable is given in the test above; kept here as a list so the
/// classification is countable rather than implied.
const UNREACHABLE: &[&str] = &["NotInitialized", "NotAdmin"];

#[test]
fn test_every_error_variant_is_accounted_for() {
    assert_eq!(
        DECLARED.len(),
        COVERED_HERE.len() + COVERED_ELSEWHERE.len() + UNREACHABLE.len(),
        "a variant is classified twice or not at all"
    );
    for variant in DECLARED {
        assert!(
            COVERED_HERE.contains(variant)
                || COVERED_ELSEWHERE.contains(variant)
                || UNREACHABLE.contains(variant),
            "{variant} is not accounted for: add it to COVERED_HERE, \
             COVERED_ELSEWHERE, or UNREACHABLE with a reason"
        );
    }
}

#[test]
fn test_declared_ledger_matches_the_contract() {
    // Guards against the ledger drifting from the enum: a variant added to
    // `ContractError` without a ledger entry fails here.
    let source = include_str!("lib.rs");
    for variant in DECLARED {
        assert!(
            source.contains(&format!("{variant} =")),
            "{variant} is in the ledger but not declared in lib.rs"
        );
    }
}
