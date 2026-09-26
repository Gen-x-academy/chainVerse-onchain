//! #1150 — load tests for scholarship deadlines and award batches.
//!
//! These are not throughput benchmarks. They are *correctness under load*
//! tests: the interesting question is not "how fast", it is "does the
//! invariant still hold after a realistic burst", because every one of
//! these paths is a read-check-write that the contracts must keep atomic
//! (ADR 0002, Concurrency).
//!
//! The scenario each file reproduces is the one that actually happens in
//! production: applications land in a burst either side of a deadline, and
//! a sponsor works through an award batch in an arbitrary order, including
//! releasing slots when recipients decline.

use scholarship_applications::ContractError as AppError;
use scholarship_e2e::fixture::World;
use scholarship_programs::ContractError as ProgError;
use soroban_sdk::testutils::Ledger as _;

const START: u64 = 1_000;
const WINDOW: u64 = 100_000;

/// A larger cohort than a journey test needs; the fixture pre-generates
/// eight students and four reviewers, which is enough to drive a burst
/// without generating thousands of keys in the test itself.
const BURST: u32 = 8;

fn h(w: &World, byte: u8) -> soroban_sdk::BytesN<32> {
    soroban_sdk::BytesN::from_array(&w.env, &[byte; 32])
}

/// Every applicant in a burst gets exactly one application, and the
/// contract's duplicate guard is what keeps it that way — including for
/// callers that retry, which is exactly what a flaky mobile client does.
#[test]
fn a_burst_of_applications_yields_exactly_one_record_each() {
    let w = World::new();
    let pid = w.publish_program(1, "Deadline Burst");

    for i in 0..BURST {
        let student = &w.actors.students.get(i).unwrap();
        w.prepare_student(student, &pid, WINDOW);
        // Each student tries twice; the retry is the interesting part.
        w.applications
            .submit_application(student, &pid, &h(&w, 1), &1);
        assert_eq!(
            w.applications
                .try_submit_application(student, &pid, &h(&w, 2), &1),
            Err(Ok(AppError::DuplicateApplication))
        );
    }

    // 8 applicants, 8 records, no more.
    assert_eq!(w.programs.remaining_capacity(&pid), 4);
    for i in 0..BURST {
        let student = &w.actors.students.get(i).unwrap();
        assert!(w.applications.has_applied(student, &pid));
    }
}

/// The deadline is a hard boundary on ledger time, and nothing about a
/// burst changes where it sits: the last applicant inside the window is
/// accepted, the next one is not, and the gap between them is one second.
#[test]
fn the_deadline_boundary_splits_a_burst_cleanly() {
    let w = World::new();
    let pid = w.publish_program(2, "Boundary");
    let deadline = START + WINDOW;

    // First half of the cohort lands just inside the window.
    w.env.ledger().set_timestamp(deadline);
    for i in 0..BURST / 2 {
        let student = &w.actors.students.get(i).unwrap();
        w.prepare_student(student, &pid, WINDOW);
        w.applications
            .submit_application(student, &pid, &h(&w, 1), &1);
    }

    // Second half lands one second later and is refused — but the refusals
    // leave no partial state behind, which is what makes a retry safe.
    w.env.ledger().set_timestamp(deadline + 1);
    for i in (BURST / 2)..BURST {
        let student = &w.actors.students.get(i).unwrap();
        w.prepare_student(student, &pid, WINDOW);
        w.applications.record_consent(student, &pid);
        assert_eq!(
            w.applications
                .try_submit_application(student, &pid, &h(&w, 1), &1),
            Err(Ok(AppError::DeadlinePassed))
        );
        assert!(!w.applications.has_applied(student, &pid));
    }

    assert_eq!(w.programs.remaining_capacity(&pid), 4);
}

/// A sponsor working an award batch in an arbitrary order: reserve, pay,
/// release on decline, reserve again. The budget must be exact at every
/// step, not merely at the end.
#[test]
fn an_award_batch_survives_an_arbitrary_reserve_release_order() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(3, "Award Batch");

    let mut expected_awarded: u32 = 0;
    // 4 slots: fill it, decline two, refill, then a fifth is refused.
    for _ in 0..4 {
        w.programs.reserve_award(&admin, &pid);
        expected_awarded += 1;
        assert_eq!(
            w.programs.remaining_capacity(&pid),
            4 - expected_awarded,
            "capacity after reserve #{expected_awarded}"
        );
    }
    assert_eq!(w.programs.get_award_budget(&pid).committed_amount, 4_000);

    for _ in 0..2 {
        w.programs.release_award(&admin, &pid);
        expected_awarded -= 1;
        assert_eq!(w.programs.remaining_capacity(&pid), 4 - expected_awarded);
    }
    assert_eq!(w.programs.get_award_budget(&pid).committed_amount, 2_000);

    for _ in 0..2 {
        w.programs.reserve_award(&admin, &pid);
        expected_awarded += 1;
    }
    assert_eq!(expected_awarded, 4);
    assert_eq!(w.programs.remaining_capacity(&pid), 0);
    assert_eq!(
        w.programs.try_reserve_award(&admin, &pid),
        Err(Ok(ProgError::CapacityExceeded))
    );
    assert_eq!(w.programs.get_award_budget(&pid).committed_amount, 4_000);
}

/// Capacity is per program: a sponsor working several batches at once
/// cannot spend one program's inventory on another's.
#[test]
fn award_batches_across_programs_do_not_interfere() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let a = w.publish_program(4, "Batch A");
    let b = w.publish_program(5, "Batch B");

    w.programs.reserve_award(&admin, &a);
    w.programs.reserve_award(&admin, &a);
    assert_eq!(w.programs.remaining_capacity(&a), 2);
    assert_eq!(w.programs.remaining_capacity(&b), 4);

    // Exhausting A leaves B untouched.
    w.programs.reserve_award(&admin, &a);
    w.programs.reserve_award(&admin, &a);
    assert_eq!(w.programs.remaining_capacity(&a), 0);
    assert_eq!(w.programs.remaining_capacity(&b), 4);
    assert_eq!(w.programs.get_award_budget(&a).awarded_count, 4);
    assert_eq!(w.programs.get_award_budget(&b).awarded_count, 0);
}

/// A window that has closed still keeps its budget readable and its
/// capacity intact, so a sponsor reconciling a closed program at 2am is
/// reading committed amounts rather than guessing.
#[test]
fn a_closed_program_keeps_its_budget_readable() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let sponsor = w.actors.sponsor.clone();
    let pid = w.publish_program(6, "Reconciliation");

    w.programs.reserve_award(&admin, &pid);
    w.programs.reserve_award(&admin, &pid);
    w.core
        .transition_program(&sponsor, &pid, &scholarship_core::ProgramStatus::Closed);

    w.env.ledger().set_timestamp(START + WINDOW * 10);
    assert!(!w.programs.is_open(&pid));
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 2);
    assert_eq!(budget.committed_amount, 2_000);
    assert_eq!(w.programs.remaining_capacity(&pid), 2);
    // And a late release is still safe, because a decline can be processed
    // long after the window closed.
    w.programs.release_award(&admin, &pid);
    assert_eq!(w.programs.get_award_budget(&pid).awarded_count, 1);
}

/// Grant/deadline arithmetic must be checked, not assumed: a budget whose
/// ceiling cannot cover its own per-award amount is rejected up front
/// rather than after the first award.
#[test]
fn an_unfundable_budget_is_rejected_at_configuration_time() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let sponsor = w.actors.sponsor.clone();
    let pid = soroban_sdk::BytesN::from_array(&w.env, &[9u8; 32]);
    w.core.create_program(
        &sponsor,
        &pid,
        &h(&w, 1),
        &soroban_sdk::String::from_str(&w.env, "Unfundable"),
        &soroban_sdk::String::from_str(&w.env, "Ceiling below the per-award amount."),
        &soroban_sdk::Symbol::new(&w.env, "USDC"),
        &scholarship_core::FundingModel::FixedAward,
    );
    w.programs
        .set_program_window(&admin, &pid, &START, &(START + WINDOW), &0, &0);

    assert_eq!(
        w.programs
            .try_configure_award_budget(&admin, &pid, &10, &1_000, &5_000),
        Err(Ok(ProgError::InvalidBudgetConfig))
    );
    assert_eq!(
        w.programs
            .try_configure_award_budget(&admin, &pid, &10, &0, &5_000),
        Err(Ok(ProgError::InvalidBudgetConfig))
    );
    // The rejected attempts left no budget behind at all.
    assert_eq!(
        w.programs.try_remaining_capacity(&pid),
        Err(Ok(ProgError::BudgetNotFound))
    );
}

/// The full award-batch lifecycle, sized like a real program's, driven
/// against a closing deadline to prove the two clocks do not interfere.
#[test]
fn a_full_cohort_and_award_batch_against_a_closing_deadline() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(8, "Full Cohort");

    // Whole cohort applies, well inside the window.
    for i in 0..BURST {
        let student = &w.actors.students.get(i).unwrap();
        w.prepare_student(student, &pid, WINDOW);
        w.applications
            .submit_application(student, &pid, &h(&w, 1), &1);
    }

    // Four awards are funded, four applicants are not — the surplus is
    // simply a program with fewer funds than demand, which is the normal
    // case and must not be an error state on chain.
    for _ in 0..4 {
        w.programs.reserve_award(&admin, &pid);
    }
    assert_eq!(w.programs.remaining_capacity(&pid), 0);

    // Then the deadline passes. Nothing about the already-funded awards
    // changes, and the unfunded applicants are not penalised or mutated.
    w.env.ledger().set_timestamp(START + WINDOW + 1);
    assert!(!w.programs.is_open(&pid));
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 4);
    assert_eq!(budget.committed_amount, 4_000);
    for i in 0..BURST {
        let student = &w.actors.students.get(i).unwrap();
        assert!(w.applications.has_applied(student, &pid));
    }
}
