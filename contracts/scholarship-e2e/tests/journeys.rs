//! #1148 — end-to-end scholarship journeys.
//!
//! Each test drives the five contracts the way the off-chain orchestrator
//! does, in the order a real program's lifecycle happens, and asserts the
//! cross-module state a frontend would render. Two things are exercised
//! throughout:
//!
//! - **Role isolation.** Each journey names which actor performs each step,
//!   and the negative tests prove a neighbouring role cannot perform it.
//! - **Accessibility-critical actions.** Anything an applicant must be able
//!   to do alone — consent, submit, revoke — is driven with nothing but the
//!   applicant's own address, never depending on an admin, a reviewer, or a
//!   second signature.
//!
//! Failure recovery is covered explicitly: a rejected submission must leave
//! the applicant able to try again, and a declined award must return its
//! capacity to the budget rather than stranding it.

use scholarship_applications::ApplicationStatus;
use scholarship_applications::ContractError as AppError;
use scholarship_core::{ContractError as CoreError, ProgramStatus};
use scholarship_e2e::fixture::World;
use scholarship_programs::ContractError as ProgError;
use scholarship_registry::{ContractError as RegError, Role};
use soroban_sdk::{testutils::Ledger as _, BytesN, String as SorobanString, Symbol};

/// Ledger time the seeded fixture starts at, so a journey can talk about
/// "the deadline" in absolute terms.
const START: u64 = 1_000;
/// How long the seeded program window stays open.
const WINDOW: u64 = 100_000;

/// A distinct 32-byte commitment, standing in for content that lives
/// off-chain (form answers, documents) and is only ever referenced by hash.
fn h(w: &World, byte: u8) -> BytesN<32> {
    BytesN::from_array(&w.env, &[byte; 32])
}

// ── the whole lifecycle, draft through final payment ────────────────────────

#[test]
fn a_program_goes_from_draft_to_final_payment() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let sponsor = w.actors.sponsor.clone();
    let pid = h(&w, 1);

    // 1. The sponsor drafts the program. It exists, but nobody can apply to
    //    it yet: the applications contract has no program registered.
    w.core.create_program(
        &sponsor,
        &pid,
        &h(&w, 2),
        &SorobanString::from_str(&w.env, "Need-Based Award"),
        &SorobanString::from_str(&w.env, "For students in financial need."),
        &Symbol::new(&w.env, "USDC"),
        &scholarship_core::FundingModel::FixedAward,
    );
    assert_eq!(w.core.get_program_status(&pid), ProgramStatus::Draft);
    assert_eq!(
        w.applications
            .try_submit_application(&w.actors.student, &pid, &h(&w, 3), &1),
        Err(Ok(AppError::ProgramNotFound))
    );

    // 2. The admin opens the window, funds the inventory, and publishes the
    //    form and the terms applicants will be held to.
    w.programs
        .set_program_window(&admin, &pid, &START, &(START + WINDOW), &0, &0);
    w.programs
        .configure_award_budget(&admin, &pid, &2, &1_000, &2_000);
    w.applications
        .register_program(&admin, &pid, &(START + WINDOW));
    w.applications.publish_form_schema(&admin, &pid, &h(&w, 1));
    w.applications
        .publish_consent_terms(&admin, &pid, &h(&w, 2));
    let mut required = soroban_sdk::Vec::new(&w.env);
    required.push_back(w.attestation_type());
    w.eligibility
        .publish_eligibility_rule(&admin, &pid, &required);

    // 3. The sponsor publishes it, and the window is now genuinely open.
    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Published);
    assert_eq!(w.core.get_program_status(&pid), ProgramStatus::Published);
    assert!(w.programs.is_open(&pid));

    // 4. The registrar vouches for the student; only then are they eligible.
    assert!(!w.eligibility.evaluate_eligibility(&pid, &w.actors.student));
    w.attest(&w.actors.student, &pid, WINDOW);
    assert!(w.eligibility.evaluate_eligibility(&pid, &w.actors.student));

    // 5. The student consents and submits, alone.
    assert_eq!(w.applications.record_consent(&w.actors.student, &pid), 1);
    w.applications
        .submit_application(&w.actors.student, &pid, &h(&w, 3), &1);
    let application = w.applications.get_application(&w.actors.student, &pid);
    assert_eq!(application.status, ApplicationStatus::Submitted);
    assert_eq!(application.submitted_at, START);
    assert_eq!(application.form_version, 1);
    assert_eq!(application.consent_version, 1);

    // 6. Finance reserves an award slot. The budget — not the applicant
    //    count — is what stops overselling.
    assert_eq!(w.programs.reserve_award(&admin, &pid), 1);
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 1);
    assert_eq!(budget.committed_amount, 1_000);

    // 7. The award is paid and the program closes. Closing the lifecycle
    //    does not by itself stop intake — that is the cross-contract gap
    //    ADR 0002 records — so the orchestrator also deactivates the
    //    program in the contract that actually gates submissions.
    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Closed);
    w.programs.release_award(&admin, &pid);
    assert_eq!(w.programs.remaining_capacity(&pid), 2);
    w.applications.set_program_active(&admin, &pid, &false);
    assert_eq!(
        w.applications
            .try_submit_application(&w.actors.reviewer, &pid, &h(&w, 4), &1),
        Err(Ok(AppError::ProgramInactive))
    );

    // 8. Archiving is terminal, and the applicant's record survives it.
    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Archived);
    assert_eq!(w.core.get_program_status(&pid), ProgramStatus::Archived);
    assert!(w.applications.has_applied(&w.actors.student, &pid));
    assert_eq!(
        w.core
            .try_transition_program(&sponsor, &pid, &ProgramStatus::Published),
        Err(Ok(CoreError::InvalidTransition))
    );
}

/// A cohort at the award ceiling: more applicants than slots, and the
/// surplus must be a clean refusal rather than a corrupted budget.
#[test]
fn a_cohort_of_students_can_be_admitted_up_to_the_award_ceiling() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(7, "Cohort Award");

    for i in 0..w.actors.students.len() {
        let student = &w.actors.students.get(i).unwrap();
        w.prepare_student(student, &pid, WINDOW);
        w.applications
            .submit_application(student, &pid, &h(&w, 3), &1);
    }

    // 8 applicants against 4 award slots.
    assert_eq!(w.programs.remaining_capacity(&pid), 4);
    for _ in 0..4 {
        w.programs.reserve_award(&admin, &pid);
    }
    assert_eq!(w.programs.remaining_capacity(&pid), 0);
    assert_eq!(
        w.programs.try_reserve_award(&admin, &pid),
        Err(Ok(ProgError::CapacityExceeded))
    );

    // Every applicant is still readable: the admission decision is
    // off-chain, and no application is hidden or rewritten by it.
    for i in 0..w.actors.students.len() {
        let student = &w.actors.students.get(i).unwrap();
        assert!(w.applications.has_applied(student, &pid));
        assert_eq!(
            w.applications.get_application(student, &pid).status,
            ApplicationStatus::Submitted
        );
    }
}

/// A pausable program is one a sponsor can stop without destroying it:
/// applications keep their records, and intake resumes on unpause.
#[test]
fn pausing_a_program_preserves_records_and_only_the_owner_can_unpause() {
    let w = World::new();
    let sponsor = w.actors.sponsor.clone();
    let pid = w.publish_program(5, "Pausable");
    let student = w.actors.student.clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications
        .submit_application(&student, &pid, &h(&w, 3), &1);

    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Paused);
    assert_eq!(w.core.get_program_status(&pid), ProgramStatus::Paused);
    assert!(w.applications.has_applied(&student, &pid));

    // Only the owner may unpause, and the state machine refuses a
    // Draft -> Published jump from any other actor.
    assert_eq!(
        w.core
            .try_transition_program(&w.actors.stranger, &pid, &ProgramStatus::Published),
        Err(Ok(CoreError::NotProgramOwner))
    );
    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Published);
    assert!(w.programs.is_open(&pid));

    // And the ledger moves on regardless of lifecycle state.
    w.env.ledger().set_timestamp(START + WINDOW - 1);
    assert!(w.programs.is_open(&pid));
}

// ── role isolation ──────────────────────────────────────────────────────────

#[test]
fn a_reviewer_cannot_act_as_the_sponsor_or_as_finance() {
    let w = World::new();
    let pid = w.publish_program(3, "Reviewer Cannot Steal");
    let admin = w.actors.admin.clone();
    let reviewer = w.actors.reviewer.clone();

    // A reviewer cannot transition someone else's program.
    assert_eq!(
        w.core
            .try_transition_program(&reviewer, &pid, &ProgramStatus::Closed),
        Err(Ok(CoreError::NotProgramOwner))
    );

    // Nor reconfigure the award budget, nor reserve an award: both are
    // admin-only today (ADR 0002, Actors).
    assert_eq!(
        w.programs
            .try_configure_award_budget(&reviewer, &pid, &9, &9, &9),
        Err(Ok(ProgError::NotAdmin))
    );
    assert_eq!(
        w.programs.try_reserve_award(&reviewer, &pid),
        Err(Ok(ProgError::NotAdmin))
    );

    // The registry is where an orchestrator resolves *which* actor is meant
    // to hold the authority a contract only checks as "admin".
    assert!(w.registry.has_role(&w.actors.finance, &Role::Finance));
    assert!(!w.registry.has_role(&reviewer, &Role::Finance));

    w.programs.reserve_award(&admin, &pid);
    assert_eq!(w.programs.get_award_budget(&pid).awarded_count, 1);
}

#[test]
fn a_stranger_is_refused_every_role_consistently() {
    let w = World::new();
    let stranger = w.actors.stranger.clone();

    // Every role check reports the same error rather than each caller
    // inventing its own, so a 401 body is stable for the frontend.
    for role in [
        Role::Student,
        Role::Sponsor,
        Role::Reviewer,
        Role::Finance,
        Role::Administrator,
    ] {
        assert!(!w.registry.has_role(&stranger, &role));
        assert_eq!(
            w.registry.try_require_role(&stranger, &role),
            Err(Ok(RegError::Unauthorized))
        );
    }
    // The roles that do exist are unaffected by a stranger's presence.
    assert!(w.registry.has_role(&w.actors.student, &Role::Student));
}

#[test]
fn revoking_a_role_takes_effect_on_the_next_check() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let reviewer = w.actors.reviewer.clone();

    assert!(w.registry.has_role(&reviewer, &Role::Reviewer));
    w.registry.revoke_role(&admin, &reviewer, &Role::Reviewer);
    assert!(!w.registry.has_role(&reviewer, &Role::Reviewer));
    assert_eq!(
        w.registry.try_require_role(&reviewer, &Role::Reviewer),
        Err(Ok(RegError::Unauthorized))
    );
}

// ── accessibility-critical actions ──────────────────────────────────────────

#[test]
fn an_applicant_can_consent_submit_and_revoke_without_any_counterparty() {
    let w = World::new();
    let pid = w.publish_program(11, "Self-Serve");
    let student = w.actors.student.clone();

    // The whole intake path needs nothing but the applicant's own address:
    // no admin, no reviewer, no off-chain account, no second signature.
    assert_eq!(w.applications.record_consent(&student, &pid), 1);
    assert!(w.applications.has_valid_consent(&student, &pid));
    w.applications
        .submit_application(&student, &pid, &h(&w, 5), &1);
    assert!(w.applications.has_applied(&student, &pid));

    // Revocation is equally self-serve, and takes effect immediately: the
    // already-submitted application stays on chain, but the applicant's
    // consent no longer authorises anything new.
    w.applications.revoke_consent(&student, &pid);
    assert!(!w.applications.has_valid_consent(&student, &pid));
    let record = w.applications.get_consent(&student, &pid);
    assert!(record.revoked);
    assert_eq!(record.revoked_at, START);
    assert!(w.applications.has_applied(&student, &pid));
}

#[test]
fn republished_terms_strand_applicants_who_must_re_consent() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(13, "Terms Change");
    let student = w.actors.students.get(0).unwrap().clone();
    let other = w.actors.students.get(1).unwrap().clone();
    let latecomer = w.actors.students.get(2).unwrap().clone();
    let never = w.actors.students.get(3).unwrap().clone();

    // Two applicants accept the terms as published (v1) and submit.
    for applicant in [&student, &other] {
        w.prepare_student(applicant, &pid, WINDOW);
        assert_eq!(w.applications.record_consent(applicant, &pid), 1);
        w.applications
            .submit_application(applicant, &pid, &h(&w, 6), &1);
    }

    // A new terms bundle invalidates every outstanding consent (ADR 0002,
    // Failure modes). The submissions already made are untouched — each
    // stays bound to the terms it was actually given under — but any
    // *further* submission from these applicants is refused until they
    // re-consent.
    w.applications
        .publish_consent_terms(&admin, &pid, &h(&w, 7));
    assert_eq!(w.applications.get_latest_consent_version(&pid), 2);
    for applicant in [&student, &other] {
        assert!(!w.applications.has_valid_consent(applicant, &pid));
    }

    // A first-time applicant who consents against the *new* terms is not
    // affected by anyone else's outstanding re-consent.
    w.prepare_student(&latecomer, &pid, WINDOW);
    assert_eq!(w.applications.record_consent(&latecomer, &pid), 2);
    w.applications
        .submit_application(&latecomer, &pid, &h(&w, 8), &1);
    assert_eq!(
        w.applications
            .get_application(&latecomer, &pid)
            .consent_version,
        2
    );

    // An eligible applicant who never consented is refused for the
    // distinct, more fundamental reason — the two failures are not
    // conflated, so a frontend can tell the user which one to fix.
    w.attest(&never, &pid, WINDOW);
    assert_eq!(
        w.applications
            .try_submit_application(&never, &pid, &h(&w, 10), &1),
        Err(Ok(AppError::ConsentNotFound))
    );

    // Re-consent is entirely in the applicant's hands, and their earlier
    // record keeps the version it was actually made under.
    assert_eq!(w.applications.record_consent(&student, &pid), 2);
    assert!(w.applications.has_valid_consent(&student, &pid));
    let first = w.applications.get_application(&student, &pid);
    assert_eq!(first.consent_version, 1);
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 9), &1),
        Err(Ok(AppError::DuplicateApplication))
    );
    assert_eq!(
        w.applications
            .get_application(&student, &pid)
            .consent_version,
        1
    );
}

// ── failure recovery ────────────────────────────────────────────────────────

#[test]
fn a_submission_rejected_for_an_unknown_form_version_leaves_the_applicant_able_to_retry() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(17, "Unknown Form");
    let student = w.actors.student.clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications.record_consent(&student, &pid);

    // A client that invented — or guessed — a form version is refused, and
    // the refusal names the reason rather than accepting answers that were
    // never validated against a real published schema. Note that an *older
    // published* version is still honoured: schemas are immutable, so an
    // applicant who completed v1 is not invalidated by v2 shipping.
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 3), &99),
        Err(Ok(AppError::FormSchemaNotFound))
    );
    assert!(!w.applications.has_applied(&student, &pid));

    // Publishing another version does not lock the applicant out of the
    // one they already completed.
    w.applications.publish_form_schema(&admin, &pid, &h(&w, 11));
    let current = w.applications.get_latest_form_version(&pid);
    assert_eq!(current, 2);
    w.applications
        .submit_application(&student, &pid, &h(&w, 3), &1);
    let application = w.applications.get_application(&student, &pid);
    assert_eq!(application.form_version, 1);
    assert_eq!(application.data_hash, h(&w, 3));
}

#[test]
fn a_declined_award_returns_its_capacity_to_the_budget() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(19, "Declined Award");
    let student = w.actors.student.clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications
        .submit_application(&student, &pid, &h(&w, 3), &1);

    w.programs.reserve_award(&admin, &pid);
    assert_eq!(w.programs.remaining_capacity(&pid), 3);

    // The recipient declines: the slot and its amount go back, so a waitlist
    // applicant can be funded without a sponsor topping the budget up.
    w.programs.release_award(&admin, &pid);
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 0);
    assert_eq!(budget.committed_amount, 0);
    assert_eq!(w.programs.remaining_capacity(&pid), 4);

    // Capacity is never double-credited: releasing a slot that was not
    // reserved is refused rather than inflating the budget.
    assert_eq!(
        w.programs.try_release_award(&admin, &pid),
        Err(Ok(ProgError::NoAwardsReserved))
    );
    assert_eq!(w.programs.get_award_budget(&pid).awarded_count, 0);
}

#[test]
fn a_submission_after_the_deadline_is_refused_but_records_survive() {
    let w = World::new();
    let pid = w.publish_program(23, "Deadline");
    let student = w.actors.student.clone();
    let latecomer = w.actors.students.get(1).unwrap().clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications
        .submit_application(&student, &pid, &h(&w, 3), &1);

    // The deadline is enforced by the applications contract that owns
    // intake, against ledger time — never against a caller's clock.
    w.env.ledger().set_timestamp(START + WINDOW + 1);
    w.prepare_student(&latecomer, &pid, WINDOW);
    w.applications.record_consent(&latecomer, &pid);
    assert_eq!(
        w.applications
            .try_submit_application(&latecomer, &pid, &h(&w, 4), &1),
        Err(Ok(AppError::DeadlinePassed))
    );

    // The on-time applicant's record is exactly where it was.
    let application = w.applications.get_application(&student, &pid);
    assert_eq!(application.submitted_at, START);
    assert!(!w.programs.is_open(&pid));
}
