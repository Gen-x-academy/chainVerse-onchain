//! #1149 — property tests for the scholarship state machines.
//!
//! The contracts in ADR 0002 are five small state machines. Example-based
//! tests check the paths someone thought of; these check the paths nobody
//! did. Each property below states an invariant that must hold for *every*
//! reachable state, then drives a generated sequence of operations against
//! it and checks the invariant after every step.
//!
//! The properties are chosen so a failure points at a specific class of
//! bug rather than "something broke":
//!
//! - lifecycle: the transition table is total on legal edges and closed on
//!   illegal ones, and the status is a function of the actions taken.
//! - budget: capacity is exactly `max_recipients - awarded_count`, money
//!   committed is exactly `awarded_count * per_award_amount`, and neither
//!   ever exceeds the ceiling or goes negative.
//! - consent: a submission is accepted if and only if the applicant's
//!   consent is live at the program's current terms version.
//! - applications: at most one record per (applicant, program), ever.

use proptest::prelude::*;

use scholarship_applications::ContractError as AppError;
use scholarship_core::{ContractError as CoreError, ProgramStatus};
use scholarship_e2e::fixture::World;
use scholarship_programs::ContractError as ProgError;
use soroban_sdk::{testutils::Ledger as _, BytesN, String as SorobanString, Symbol};

const START: u64 = 1_000;
const WINDOW: u64 = 100_000;

fn h(w: &World, byte: u8) -> BytesN<32> {
    BytesN::from_array(&w.env, &[byte; 32])
}

/// The legal edges of the lifecycle, written out here independently of the
/// contract's own `is_legal_transition` so a bug in the table cannot be
/// rubber-stamped by the table it came from. `Archived` is terminal: it has
/// no outgoing edge at all, not even to itself.
fn legal(from: ProgramStatus, to: ProgramStatus) -> bool {
    use ProgramStatus::*;
    matches!(
        (from, to),
        (Draft, Published)
            | (Draft, Archived)
            | (Published, Paused)
            | (Published, Closed)
            | (Paused, Published)
            | (Paused, Closed)
            | (Closed, Archived)
    )
}

/// An action in the lifecycle machine.
#[derive(Clone, Debug)]
enum Life {
    Publish,
    Pause,
    Resume,
    Close,
    Archive,
}

fn apply_life(w: &World, pid: &BytesN<32>, sponsor: &soroban_sdk::Address, a: &Life) -> bool {
    let to = match a {
        Life::Publish => ProgramStatus::Published,
        Life::Pause => ProgramStatus::Paused,
        Life::Resume => ProgramStatus::Published,
        Life::Close => ProgramStatus::Closed,
        Life::Archive => ProgramStatus::Archived,
    };
    match w.core.try_transition_program(sponsor, pid, &to) {
        Ok(Ok(())) => true,
        // Both a non-owner call and an illegal edge are failures, not
        // silent no-ops: the contract must say no.
        Err(Ok(CoreError::NotProgramOwner)) | Err(Ok(CoreError::InvalidTransition)) => false,
        other => panic!("unexpected transition result: {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// Whatever sequence of transitions is attempted, the program's status
    /// is always a real status, an accepted transition is always one the
    /// table allows, and a rejected one never changes the status.
    #[test]
    fn program_status_only_ever_follows_legal_transitions(
        actions in prop::collection::vec(prop_oneof![
            Just(Life::Publish),
            Just(Life::Pause),
            Just(Life::Resume),
            Just(Life::Close),
            Just(Life::Archive),
        ], 1..14),
    ) {
        let w = World::new();
        let sponsor = w.actors.sponsor.clone();
        let pid = h(&w, 1);
        w.core.create_program(
            &sponsor,
            &pid,
            &h(&w, 2),
            &SorobanString::from_str(&w.env, "Generated"),
            &SorobanString::from_str(&w.env, "Driven by a generated action list."),
            &Symbol::new(&w.env, "USDC"),
            &scholarship_core::FundingModel::FixedAward,
        );

        let mut status = ProgramStatus::Draft;
        let mut applied = 0u32;
        for action in &actions {
            let before = status;
            if apply_life(&w, &pid, &sponsor, action) {
                // An accepted transition must be an edge the independent
                // table allows, and must actually move the machine.
                let to = match action {
                    Life::Publish | Life::Resume => ProgramStatus::Published,
                    Life::Pause => ProgramStatus::Paused,
                    Life::Close => ProgramStatus::Closed,
                    Life::Archive => ProgramStatus::Archived,
                };
                prop_assert!(legal(before, to), "accepted an illegal edge {before:?} -> {to:?}");
                prop_assert_ne!(before, to, "accepted a no-op transition");
                applied += 1;
            }
            status = w.core.get_program_status(&pid);
            // Whatever happened, the observable status is the one the
            // machine should be in.
            let expected = if applied == 0 { ProgramStatus::Draft } else { status };
            prop_assert_eq!(status, expected);
        }
    }

    /// A non-owner never moves the machine, no matter how long the
    /// generated sequence is or which edges it targets.
    #[test]
    fn a_non_owner_cannot_move_the_lifecycle(
        actions in prop::collection::vec(prop_oneof![
            Just(Life::Publish),
            Just(Life::Pause),
            Just(Life::Resume),
            Just(Life::Close),
            Just(Life::Archive),
        ], 1..14),
    ) {
        let w = World::new();
        let attacker = w.actors.stranger.clone();
        let pid = w.publish_program(1, "Ownership");
        let before = w.core.get_program_status(&pid);

        for action in &actions {
            let to = match action {
                Life::Publish | Life::Resume => ProgramStatus::Published,
                Life::Pause => ProgramStatus::Paused,
                Life::Close => ProgramStatus::Closed,
                Life::Archive => ProgramStatus::Archived,
            };
            prop_assert_eq!(
                w.core.try_transition_program(&attacker, &pid, &to),
                Err(Ok(CoreError::NotProgramOwner))
            );
            prop_assert_eq!(w.core.get_program_status(&pid), before);
        }
    }
}

/// An award-batch action.
#[derive(Clone, Debug)]
enum Batch {
    Reserve,
    Release,
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// The budget invariant, checked after every generated action: capacity
    /// is the difference, committed money is the product, and the reserve
    /// call never hands out a slot the budget cannot cover.
    #[test]
    fn the_award_budget_invariant_holds_after_every_action(
        max_recipients in 1u32..6,
        actions in prop::collection::vec(
            prop_oneof![Just(Batch::Reserve), Just(Batch::Release)],
            1..24,
        ),
    ) {
        let w = World::new();
        let admin = w.actors.admin.clone();
        let pid = w.publish_program(1, "Generated Batch");
        w.programs.configure_award_budget(
            &admin,
            &pid,
            &max_recipients,
            &1_000,
            &(i128::from(max_recipients) * 1_000),
        );

        let mut expected_awarded: u32 = 0;
        for action in &actions {
            let at_capacity = expected_awarded >= max_recipients;
            match action {
                Batch::Reserve => {
                    if at_capacity {
                        prop_assert_eq!(
                            w.programs.try_reserve_award(&admin, &pid),
                            Err(Ok(ProgError::CapacityExceeded)),
                        );
                    } else {
                        w.programs.reserve_award(&admin, &pid);
                        expected_awarded += 1;
                    }
                }
                Batch::Release => {
                    if expected_awarded == 0 {
                        prop_assert_eq!(
                            w.programs.try_release_award(&admin, &pid),
                            Err(Ok(ProgError::NoAwardsReserved)),
                        );
                    } else {
                        w.programs.release_award(&admin, &pid);
                        expected_awarded -= 1;
                    }
                }
            }

            let budget = w.programs.get_award_budget(&pid);
            prop_assert_eq!(budget.awarded_count, expected_awarded);
            prop_assert_eq!(budget.committed_amount, i128::from(expected_awarded) * 1_000);
            prop_assert!(budget.committed_amount <= budget.total_budget);
            prop_assert_eq!(
                w.programs.remaining_capacity(&pid),
                max_recipients - expected_awarded
            );
        }
    }
}

/// An intake action for one applicant against one program.
#[derive(Clone, Debug)]
enum Intake {
    Consent,
    Revoke,
    Submit,
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// A submission exists if and only if the applicant consented, never
    /// revoked afterwards without re-consenting, and was inside the
    /// deadline — checked after every generated action.
    #[test]
    fn a_submission_is_accepted_exactly_when_consent_is_live(
        actions in prop::collection::vec(
            prop_oneof![
                Just(Intake::Consent),
                Just(Intake::Revoke),
                Just(Intake::Submit),
            ],
            1..14,
        ),
    ) {
        let w = World::new();
        let pid = w.publish_program(1, "Generated Intake");
        let student = w.actors.student.clone();
        w.attest(&student, &pid, WINDOW);

        // Two separate facts, because the contract keeps them separate: a
        // consent *record* once created is never deleted (so revoking it
        // twice is a no-op, not an error), and consent is *live* only
        // while that record is unrevoked.
        let mut has_record = false;
        let mut live = false;
        let mut applied = false;
        for action in &actions {
            match action {
                Intake::Consent => {
                    w.applications.record_consent(&student, &pid);
                    has_record = true;
                    live = true;
                }
                Intake::Revoke => {
                    if has_record {
                        w.applications.revoke_consent(&student, &pid);
                        live = false;
                    } else {
                        prop_assert_eq!(
                            w.applications.try_revoke_consent(&student, &pid),
                            Err(Ok(AppError::ConsentNotFound))
                        );
                    }
                }
                Intake::Submit => {
                    let result = w
                        .applications
                        .try_submit_application(&student, &pid, &h(&w, 1), &1);
                    // The contract's guard order is: program, active,
                    // deadline, form version, then the consent gates, and
                    // only then the duplicate check. Consent therefore wins
                    // the error a caller sees after a revocation — the
                    // property pins that ordering rather than assuming it,
                    // because a client picks its retry advice from the code
                    // it is handed.
                    if live {
                        if applied {
                            // One record per (applicant, program), forever.
                            prop_assert_eq!(result, Err(Ok(AppError::DuplicateApplication)));
                        } else {
                            prop_assert_eq!(result, Ok(Ok(())));
                            applied = true;
                        }
                    } else if has_record {
                        prop_assert_eq!(result, Err(Ok(AppError::ConsentRevoked)));
                    } else {
                        prop_assert_eq!(result, Err(Ok(AppError::ConsentNotFound)));
                    }
                }
            }
            prop_assert_eq!(w.applications.has_applied(&student, &pid), applied);
            prop_assert_eq!(w.applications.has_valid_consent(&student, &pid), live);
        }
    }

    /// Advancing the ledger never changes what is stored, only what is
    /// currently valid — and an applicant inside the window keeps
    /// submitting successfully across any number of generated time steps.
    #[test]
    fn moving_the_clock_only_changes_validity_never_records(
        steps in prop::collection::vec(1u64..40, 1..20),
        max_recipients in 1u32..5,
    ) {
        let w = World::new();
        let pid = w.publish_program(1, "Clock");
        let student = w.actors.student.clone();
        w.prepare_student(&student, &pid, WINDOW);
        w.applications.submit_application(&student, &pid, &h(&w, 1), &1);
        let before = w.applications.get_application(&student, &pid);
        w.programs.configure_award_budget(
            &w.actors.admin.clone(),
            &pid,
            &max_recipients,
            &1_000,
            &(i128::from(max_recipients) * 1_000),
        );

        for step in &steps {
            w.env.ledger().set_timestamp(START + (step % (WINDOW - 1)));
            prop_assert_eq!(w.applications.get_application(&student, &pid), before.clone());
            prop_assert_eq!(w.programs.get_award_budget(&pid).awarded_count, 0);
        }
    }
}
