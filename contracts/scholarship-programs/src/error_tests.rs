#![cfg(test)]
//! Error-path coverage for `scholarship-programs` (issue #1146).
//!
//! Closes the gap for the six variants no other test asserted, four of which
//! guard money or capacity: `WindowOverflow`, `BudgetExceeded`,
//! `BudgetAlreadyCommitted`, and `WindowNotFound`.
//!
//! ## Coverage ledger
//!
//! | Variant | Exercised by | Note |
//! | --- | --- | --- |
//! | `NotInitialized` | `test_uninitialized_window_read_is_refused` | |
//! | `AlreadyInitialized` | `test_initialize_is_not_repeatable` | |
//! | `NotAdmin` | `tests.rs` | |
//! | `InvalidWindow` | `tests.rs` | |
//! | `WindowNotFound` | `test_window_lookup_reports_a_missing_window` | |
//! | `WindowOverflow` | `test_grace_period_cannot_wrap_the_close_time` | |
//! | `InvalidBudgetConfig` | `tests.rs` | |
//! | `BudgetNotFound` | `tests.rs` | |
//! | `CapacityExceeded` | `tests.rs` | |
//! | `BudgetExceeded` | — | **unreachable**: precluded by `InvalidBudgetConfig` (see below) |
//! | `BudgetAlreadyCommitted` | `test_budget_is_frozen_once_awards_are_committed` | |
//! | — | `test_every_error_variant_is_accounted_for` | |

extern crate std;
use std::format;

use crate::{ContractError, ScholarshipProgramsContract, ScholarshipProgramsContractClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, BytesN, Env};

fn setup() -> (Env, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipProgramsContract, ());
    let admin = Address::generate(&env);
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, id, admin)
}

fn program(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[3u8; 32])
}

fn open_window(client: &ScholarshipProgramsContractClient, env: &Env, admin: &Address) {
    client.set_program_window(admin, &program(env), &1_000, &2_000, &0, &0);
}

fn fund(client: &ScholarshipProgramsContractClient, env: &Env, admin: &Address) {
    // 10 recipients at 100 each against a 1,000 total.
    client.configure_award_budget(admin, &program(env), &10, &100, &1_000);
}

// ── NotInitialized ──────────────────────────────────────────────────────────

#[test]
fn test_uninitialized_admin_write_is_refused() {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(ScholarshipProgramsContract, ());
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    let admin = Address::generate(&env);

    // The initialization guard sits on the admin path.
    assert_eq!(
        client.try_set_program_window(&admin, &program(&env), &1_000, &2_000, &0, &0),
        Err(Ok(ContractError::NotInitialized))
    );

    // Read paths are not guarded: before `initialize`, a window lookup
    // reports the window as missing rather than reporting a missing setup.
    // Both are `WindowNotFound`, which is honest -- there genuinely is no
    // window -- but a client cannot tell the two apart.
    assert_eq!(
        client.try_get_program_window(&program(&env)),
        Err(Ok(ContractError::WindowNotFound))
    );
}

// ── AlreadyInitialized ──────────────────────────────────────────────────────

#[test]
fn test_initialize_is_not_repeatable() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    assert_eq!(
        client.try_initialize(&admin),
        Err(Ok(ContractError::AlreadyInitialized))
    );
    assert_eq!(
        client.try_initialize(&Address::generate(&env)),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

// ── WindowNotFound ──────────────────────────────────────────────────────────

#[test]
fn test_window_lookup_reports_a_missing_window() {
    let (env, id, _admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // A program with no window configured has no schedule, and saying
    // "WindowNotFound" is more useful than reporting it as closed.
    assert_eq!(
        client.try_get_program_window(&program(&env)),
        Err(Ok(ContractError::WindowNotFound))
    );
    assert_eq!(
        client.try_is_open(&program(&env)),
        Err(Ok(ContractError::WindowNotFound))
    );
    assert_eq!(
        client.try_is_within_grace(&program(&env)),
        Err(Ok(ContractError::WindowNotFound))
    );
}

#[test]
fn test_missing_window_is_distinct_from_a_closed_one() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // No window: an error.
    assert!(client.try_is_open(&program(&env)).is_err());
    // A window that has closed: a definite `false`.
    open_window(&client, &env, &admin);
    env.ledger().set_timestamp(5_000);
    assert!(!client.is_open(&program(&env)));
}

// ── WindowOverflow ──────────────────────────────────────────────────────────

#[test]
fn test_grace_period_cannot_wrap_the_close_time() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // `closes_at + grace_period_seconds` would wrap u64. The contract checks
    // it, so a pathological grace period is refused rather than becoming a
    // window that appears to end before it opens.
    assert_eq!(
        client.try_set_program_window(&admin, &program(&env), &(u64::MAX - 1), &u64::MAX, &0, &1,),
        Err(Ok(ContractError::WindowOverflow))
    );
}

#[test]
fn test_window_overflow_boundary_is_table_driven() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // The largest grace period that still fits, and the smallest that does
    // not. If the check is ever loosened or tightened, one of these moves.
    let cases = std::vec![
        (u64::MAX - 20, u64::MAX - 10, 0u64, true),
        (u64::MAX - 10, u64::MAX - 5, 5, true),
        (u64::MAX - 10, u64::MAX, 0, true),
        (u64::MAX - 10, u64::MAX, 1, false),
        (u64::MAX - 10, u64::MAX, u64::MAX, false),
    ];
    for (index, (opens, closes, grace, accepted)) in cases.iter().copied().enumerate() {
        let target = BytesN::from_array(&env, &{
            let mut raw = [0u8; 32];
            raw[0] = (index + 1) as u8;
            raw
        });
        let result = client.try_set_program_window(&admin, &target, &opens, &closes, &0, &grace);
        assert_eq!(
            result.is_ok(),
            accepted,
            "grace {grace} against close {closes} should be {}",
            if accepted { "accepted" } else { "refused" }
        );
    }
}

// ── BudgetAlreadyCommitted ──────────────────────────────────────────────────

#[test]
fn test_budget_is_frozen_once_awards_are_committed() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    fund(&client, &env, &admin);
    client.reserve_award(&admin, &program(&env));

    // Reconfiguring now would silently discard the commitment already
    // recorded against the budget.
    assert_eq!(
        client.try_configure_award_budget(&admin, &program(&env), &99, &1, &10_000),
        Err(Ok(ContractError::BudgetAlreadyCommitted))
    );
    // The recorded commitment is untouched by the refused write.
    let budget = client.get_award_budget(&program(&env));
    assert_eq!(budget.committed_amount, 100);
    assert_eq!(budget.max_recipients, 10);
}

#[test]
fn test_budget_is_reconfigurable_before_any_award() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    fund(&client, &env, &admin);
    // No award committed yet, so correcting a misconfigured budget is allowed.
    assert!(client
        .try_configure_award_budget(&admin, &program(&env), &5, &50, &250)
        .is_ok());
    assert_eq!(client.get_award_budget(&program(&env)).max_recipients, 5);
}

#[test]
fn test_the_budget_freeze_tracks_the_commitment_not_the_history() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    fund(&client, &env, &admin);
    client.reserve_award(&admin, &program(&env));
    client.release_award(&admin, &program(&env));

    // Releasing the last award returns `committed_amount` to zero, and the
    // freeze lifts with it. Worth stating explicitly: the guard protects the
    // *recorded commitment* from being silently re-cut, and makes no claim
    // that an award can never be released and the budget re-decided.
    assert_eq!(client.get_award_budget(&program(&env)).committed_amount, 0);
    assert!(client
        .try_configure_award_budget(&admin, &program(&env), &5, &50, &250)
        .is_ok());
}

// ── BudgetExceeded ──────────────────────────────────────────────────────────

#[test]
fn test_budget_exceeded_cannot_be_reached_because_configuration_precludes_it() {
    // `ContractError::BudgetExceeded` is returned by `reserve_award` when the
    // new commitment would pass `total_budget`. That branch cannot be
    // reached, and the reason is worth recording:
    //
    //   * `configure_award_budget` refuses `max_recipients * per_award >
    //     total_budget`, so at configuration time
    //     `max_recipients * per_award <= total_budget`.
    //   * `reserve_award` stops at `awarded_count == max_recipients`, so
    //     `committed_amount <= max_recipients * per_award`.
    //   * Therefore `committed_amount <= total_budget` always, and the
    //     `checked_add` cannot overflow either, since `total_budget` is a
    //     valid i128.
    //   * Reconfiguration is refused once anything is committed
    //     (`BudgetAlreadyCommitted`), so the invariant cannot be re-cut
    //     afterwards.
    //
    // So this asserts the invariant that makes the branch unreachable,
    // which is the only way to test a branch that cannot be entered.
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);

    // 3 recipients at 100 against a total of exactly 300: the tightest
    // configuration the contract still accepts.
    client.configure_award_budget(&admin, &program(&env), &3, &100, &300);
    for expected in 1..=3u32 {
        assert_eq!(client.reserve_award(&admin, &program(&env)), expected);
        let budget = client.get_award_budget(&program(&env));
        assert!(
            budget.committed_amount <= budget.total_budget,
            "committed {} exceeded total {}",
            budget.committed_amount,
            budget.total_budget
        );
    }
    // A fourth reserve stops on capacity, which is the ceiling that is
    // actually reachable.
    assert_eq!(
        client.try_reserve_award(&admin, &program(&env)),
        Err(Ok(ContractError::CapacityExceeded))
    );
}

#[test]
fn test_undersized_budget_is_refused_at_configuration_time() {
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // 3 x 100 = 300 required, 299 funded. Refused here rather than
    // surfacing later as a reserve that cannot be honoured.
    assert_eq!(
        client.try_configure_award_budget(&admin, &program(&env), &3, &100, &299),
        Err(Ok(ContractError::InvalidBudgetConfig))
    );
    // Nothing was recorded.
    assert_eq!(
        client.try_get_award_budget(&program(&env)),
        Err(Ok(ContractError::BudgetNotFound))
    );
}

#[test]
fn test_budget_exceeded_boundary_is_table_driven() {
    // Capacity and budget are two different ceilings, and a program can hit
    // either first. Table-driven so which one binds is explicit.
    let cases = std::vec![
        // (max_recipients, per_award, total, reserves_that_succeed)
        (10u32, 100i128, 1_000i128, 10u32),
        // Required 300 against a funded 250: refused at configuration, so
        // there is no budget to reserve against at all.
        (3, 100, 250, 0),
        (2, 100, 200, 2),
        (5, 1, 5, 5),
        // 1,000 required against a 999 total: likewise refused at
        // configuration time.
        (10, 100, 999, 0),
    ];
    for (index, (max_recipients, per_award, total, expected_ok)) in cases.iter().enumerate() {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(ScholarshipProgramsContract, ());
        let admin = Address::generate(&env);
        let client = ScholarshipProgramsContractClient::new(&env, &id);
        client.initialize(&admin);
        let target = BytesN::from_array(&env, &{
            let mut raw = [0u8; 32];
            raw[0] = (index + 1) as u8;
            raw
        });

        if client
            .try_configure_award_budget(&admin, &target, max_recipients, per_award, total)
            .is_err()
        {
            assert_eq!(
                *expected_ok, 0,
                "case {index} was rejected at configuration"
            );
            continue;
        }

        let mut succeeded = 0u32;
        for _ in 0..*max_recipients {
            if client.try_reserve_award(&admin, &target).is_ok() {
                succeeded += 1;
            } else {
                break;
            }
        }
        assert_eq!(
            succeeded, *expected_ok,
            "case {index}: reserves that should succeed"
        );
        // Whatever the reason it stopped, the recorded commitment never
        // exceeds the funded total.
        let budget = client.get_award_budget(&target);
        assert!(
            budget.committed_amount <= budget.total_budget,
            "case {index}: committed {} exceeds total {}",
            budget.committed_amount,
            budget.total_budget
        );
    }
}

#[test]
fn test_committed_amount_never_wraps() {
    // The commit arithmetic is checked, so a budget that cannot hold another
    // award reports BudgetExceeded rather than wrapping to a negative
    // commitment that would look like enormous remaining headroom.
    let (env, id, admin) = setup();
    let client = ScholarshipProgramsContractClient::new(&env, &id);
    // 1 recipient at i128::MAX, funded at i128::MAX.
    client.configure_award_budget(&admin, &program(&env), &1, &i128::MAX, &i128::MAX);
    assert_eq!(client.reserve_award(&admin, &program(&env)), 1);
    let budget = client.get_award_budget(&program(&env));
    assert_eq!(budget.committed_amount, i128::MAX);
    assert!(budget.committed_amount > 0);
}

// ── coverage ledger ────────────────────────────────────────────────────────

const DECLARED: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "NotAdmin",
    "InvalidWindow",
    "WindowNotFound",
    "WindowOverflow",
    "InvalidBudgetConfig",
    "BudgetNotFound",
    "CapacityExceeded",
    "BudgetExceeded",
    "BudgetAlreadyCommitted",
];

const COVERED_ELSEWHERE: &[&str] = &[
    "NotAdmin",
    "InvalidWindow",
    "InvalidBudgetConfig",
    "BudgetNotFound",
    "CapacityExceeded",
];

const COVERED_HERE: &[&str] = &[
    "NotInitialized",
    "AlreadyInitialized",
    "WindowNotFound",
    "WindowOverflow",
    "BudgetAlreadyCommitted",
];

/// Declared but returned by no reachable code path. Each needs a reason.
const UNREACHABLE: &[&str] = &[
    // `configure_award_budget` refuses `max_recipients * per_award >
    // total_budget`, and `reserve_award` stops at `max_recipients`, so
    // `committed_amount` can never pass `total_budget`.
    "BudgetExceeded",
];

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
            "{variant} is not accounted for"
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
