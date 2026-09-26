//! #1151 — security abuse cases for the scholarship contracts.
//!
//! Every test here is an attack, written from the adversary's side of the
//! trust boundary ADR 0002 draws: the five contracts hold commitments and
//! counters, an off-chain orchestrator holds the content and the decisions,
//! and the applicant's own address is the only key that may act for them.
//!
//! The abuse classes covered are the ones an attacker with a funded,
//! registered wallet would actually try:
//!
//! - **Wallet substitution** — acting as, or on behalf of, another address.
//! - **Privilege self-assignment** — granting oneself a role or authority.
//! - **Replay / double-spend** — re-submitting an application, re-using a
//!   revoked consent, double-drawing an award slot.
//! - **Scope confusion** — reusing one program's consent or attestation to
//!   apply to a different program.
//! - **Stored-content abuse** — unbounded writes, and PII smuggled into
//!   on-chain fields.

use scholarship_applications::ContractError as AppError;
use scholarship_core::{ContractError as CoreError, FundingModel, ProgramStatus};
use scholarship_e2e::fixture::World;
use scholarship_eligibility::ContractError as EligError;
use scholarship_programs::ContractError as ProgError;
use scholarship_registry::ContractError as RegError;
use soroban_sdk::{
    testutils::Address as _, testutils::Ledger as _, Address, BytesN, String as SorobanString,
    Symbol,
};

const START: u64 = 1_000;
const WINDOW: u64 = 100_000;

fn h(w: &World, byte: u8) -> BytesN<32> {
    BytesN::from_array(&w.env, &[byte; 32])
}

// ── wallet substitution ─────────────────────────────────────────────────────

/// Every applicant-facing call takes the applicant as its own argument and
/// requires *that* address's authorization, so an attacker with a funded
/// wallet can only ever act for themselves. The attempt to have the
/// victim's record created for them is not expressible in the interface at
/// all, and going around it by acting as themselves leaves the victim
/// untouched.
#[test]
fn a_third_party_cannot_produce_records_for_another_applicant() {
    let w = World::new();
    let pid = w.publish_program(1, "Substitution");
    let victim = w.actors.student.clone();
    let attacker = w.actors.stranger.clone();

    w.attest(&victim, &pid, WINDOW);
    w.attest(&attacker, &pid, WINDOW);

    // The attacker consents and applies — for themselves.
    w.applications.record_consent(&attacker, &pid);
    w.applications
        .submit_application(&attacker, &pid, &h(&w, 1), &1);

    // Their own record exists; the victim's does not, and the attacker's
    // submission cannot be pointed at the victim by any argument they
    // control, because `applicant` is both the key and the required signer.
    assert!(w.applications.has_applied(&attacker, &pid));
    assert!(!w.applications.has_applied(&victim, &pid));
    assert!(!w.applications.has_valid_consent(&victim, &pid));
    assert_eq!(
        w.applications.get_application(&attacker, &pid).applicant,
        attacker
    );

    // And the victim can still apply for themselves, unaffected.
    w.prepare_student(&victim, &pid, WINDOW);
    w.applications
        .submit_application(&victim, &pid, &h(&w, 2), &1);
    assert_eq!(
        w.applications.get_application(&victim, &pid).data_hash,
        h(&w, 2)
    );
}

/// With authorization mocking switched off, a call that needs a signature
/// nobody provided fails at the host, before the contract body runs. This
/// is the property that makes the interface above safe in production: the
/// default test world mocks every signature, so this test deliberately
/// does not.
#[test]
fn a_call_without_a_signature_is_refused_by_the_host() {
    let w = World::new();
    let pid = w.publish_program(20, "Enforced Auth");
    let attacker = w.actors.stranger.clone();
    w.seal();

    // `create_program` calls `owner.require_auth()` as its first statement.
    let forged = soroban_sdk::BytesN::from_array(&w.env, &[77u8; 32]);
    let result = w.core.try_create_program(
        &attacker,
        &forged,
        &h(&w, 1),
        &SorobanString::from_str(&w.env, "Forged"),
        &SorobanString::from_str(&w.env, "Created without a signature."),
        &Symbol::new(&w.env, "USDC"),
        &FundingModel::FixedAward,
    );
    assert!(
        result.is_err(),
        "an unsigned create_program must not succeed"
    );
    assert_eq!(
        w.core.try_get_program(&forged),
        Err(Ok(CoreError::ProgramNotFound))
    );

    // Same for the applicant path: consent, submission, and revocation all
    // require the applicant's own signature.
    assert!(w.applications.try_record_consent(&attacker, &pid).is_err());
    assert!(!w.applications.has_valid_consent(&attacker, &pid));
    assert!(w
        .applications
        .try_submit_application(&attacker, &pid, &h(&w, 1), &1)
        .is_err());
    assert!(!w.applications.has_applied(&attacker, &pid));
    assert!(w.applications.try_revoke_consent(&attacker, &pid).is_err());
}

/// A program owner cannot act as the *admin* of another program, and the
/// admin cannot seize ownership of a program it does not own.
#[test]
fn ownership_and_admin_authority_do_not_overlap() {
    let w = World::new();
    let pid = w.publish_program(2, "Authority");
    let sponsor = w.actors.sponsor.clone();
    let admin = w.actors.admin.clone();
    let reviewer = w.actors.reviewer.clone();

    // A registered reviewer is still not the admin of the programs
    // contract: role membership is not authority.
    assert_eq!(
        w.programs
            .try_set_program_window(&reviewer, &pid, &0, &(START + WINDOW), &0, &0),
        Err(Ok(ProgError::NotAdmin))
    );
    assert_eq!(
        w.applications
            .try_register_program(&reviewer, &pid, &(START + WINDOW)),
        Err(Ok(AppError::NotAdmin))
    );
    let mut required = soroban_sdk::Vec::new(&w.env);
    required.push_back(w.attestation_type());
    assert_eq!(
        w.eligibility
            .try_publish_eligibility_rule(&reviewer, &pid, &required),
        Err(Ok(EligError::NotAdmin))
    );

    // Nor can the admin rewrite the lifecycle: ownership is separate, and
    // the two checks are both required.
    assert_eq!(
        w.core
            .try_transition_program(&admin, &pid, &ProgramStatus::Closed),
        Err(Ok(CoreError::NotProgramOwner))
    );
    // The sponsor, as owner, still can.
    w.core
        .transition_program(&sponsor, &pid, &ProgramStatus::Closed);
    assert_eq!(w.core.get_program_status(&pid), ProgramStatus::Closed);
}

// ── privilege self-assignment ───────────────────────────────────────────────

/// The registry is the root of trust in the role model, so an
/// unauthenticated-for-admin caller must not be able to grant itself any
/// role — including Administrator.
#[test]
fn a_stranger_cannot_grant_itself_a_role() {
    let w = World::new();
    let stranger = w.actors.stranger.clone();

    for role in [
        scholarship_registry::Role::Reviewer,
        scholarship_registry::Role::Administrator,
        scholarship_registry::Role::Finance,
    ] {
        assert_eq!(
            w.registry.try_grant_role(&stranger, &stranger, &role),
            Err(Ok(RegError::NotAdmin))
        );
        assert!(!w.registry.has_role(&stranger, &role));
    }
}

/// Likewise for the issuer list: an unrecognised address must not be able
/// to vouch for students, which is the credential-forgery path.
#[test]
fn an_unauthorised_address_cannot_issue_attestations() {
    let w = World::new();
    let pid = w.publish_program(3, "Forged Attestation");
    let impostor = w.actors.stranger.clone();
    let student = w.actors.student.clone();

    assert!(!w.eligibility.is_authorized_issuer(&impostor));
    assert_eq!(
        w.eligibility.try_issue_attestation(
            &impostor,
            &student,
            &pid,
            &w.attestation_type(),
            &WINDOW
        ),
        Err(Ok(EligError::NotAuthorizedIssuer))
    );
    assert!(!w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
    assert!(!w.eligibility.evaluate_eligibility(&pid, &student));
}

/// A third party cannot revoke someone else's attestation — revocation is
/// limited to the original issuer and the admin, so a competitor cannot
/// sabotage a rival sponsor's standing attestations.
#[test]
fn only_the_issuer_or_admin_can_revoke_an_attestation() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let registrar = w.actors.registrar.clone();
    let student = w.actors.student.clone();
    let pid = w.publish_program(4, "Revocation");
    w.attest(&student, &pid, WINDOW);
    assert!(w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));

    assert_eq!(
        w.eligibility.try_revoke_attestation(
            &w.actors.stranger,
            &student,
            &pid,
            &w.attestation_type()
        ),
        Err(Ok(EligError::NotAttestationIssuer))
    );
    assert!(w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));

    // The issuer can, and the effect is immediate.
    w.eligibility
        .revoke_attestation(&registrar, &student, &pid, &w.attestation_type());
    assert!(!w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
    assert!(!w.eligibility.evaluate_eligibility(&pid, &student));

    // The admin can too, which is the documented break-glass path.
    w.attest(&student, &pid, WINDOW);
    w.eligibility
        .revoke_attestation(&admin, &student, &pid, &w.attestation_type());
    assert!(!w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
}

// ── replay and double-spend ─────────────────────────────────────────────────

/// Replay is refused at the storage-key level, before any write: a retried
/// submission, a second attempt after revocation, and a re-submission with
/// different content all fail the same way.
#[test]
fn a_submission_cannot_be_replayed_with_different_content() {
    let w = World::new();
    let pid = w.publish_program(5, "Replay");
    let student = w.actors.student.clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications
        .submit_application(&student, &pid, &h(&w, 1), &1);

    // Identical retry.
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 1), &1),
        Err(Ok(AppError::DuplicateApplication))
    );
    // Retry with different content — the applicant cannot rewrite what they
    // already submitted.
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 2), &1),
        Err(Ok(AppError::DuplicateApplication))
    );
    // And not even with a different form version.
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 1), &2),
        Err(Ok(AppError::FormSchemaNotFound))
    );

    // The original record is byte-for-byte what was first submitted.
    let application = w.applications.get_application(&student, &pid);
    assert_eq!(application.data_hash, h(&w, 1));
    assert_eq!(application.form_version, 1);
}

/// A revoked consent cannot be replayed into a submission, and
/// re-recording consent produces a new version rather than resurrecting
/// the old one.
#[test]
fn a_revoked_consent_cannot_be_replayed() {
    let w = World::new();
    let pid = w.publish_program(6, "Revoked Consent");
    let student = w.actors.student.clone();

    w.attest(&student, &pid, WINDOW);
    assert_eq!(w.applications.record_consent(&student, &pid), 1);
    w.applications.revoke_consent(&student, &pid);
    assert!(!w.applications.has_valid_consent(&student, &pid));

    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 1), &1),
        Err(Ok(AppError::ConsentRevoked))
    );

    // Re-consenting against the *same* terms version restores it, and the
    // applicant can then submit — once.
    assert_eq!(w.applications.record_consent(&student, &pid), 1);
    assert!(w.applications.has_valid_consent(&student, &pid));
    w.applications
        .submit_application(&student, &pid, &h(&w, 1), &1);
    assert_eq!(
        w.applications
            .try_submit_application(&student, &pid, &h(&w, 1), &1),
        Err(Ok(AppError::DuplicateApplication))
    );
}

/// An expired attestation is not a live one, and re-issuing over an
/// expired one is the only way back.
#[test]
fn an_expired_attestation_stops_working_without_being_deletable() {
    let w = World::new();
    let pid = w.publish_program(7, "Attestation Expiry");
    let student = w.actors.student.clone();
    let registrar = w.actors.registrar.clone();

    w.attest(&student, &pid, WINDOW);
    assert!(w.eligibility.evaluate_eligibility(&pid, &student));

    // Expiry is compared against ledger time, not the caller's clock, and
    // is exclusive: at `expiry` the attestation is already gone.
    w.env.ledger().set_timestamp(START + WINDOW - 1);
    assert!(w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
    w.env.ledger().set_timestamp(START + WINDOW);
    assert!(!w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
    assert!(!w.eligibility.evaluate_eligibility(&pid, &student));

    // The record is still readable for audit — expiry is not deletion —
    // and the issuer can re-issue over it.
    let record = w
        .eligibility
        .get_attestation(&student, &pid, &w.attestation_type());
    assert!(!record.revoked);
    w.eligibility.issue_attestation(
        &registrar,
        &student,
        &pid,
        &w.attestation_type(),
        &(START + WINDOW * 2),
    );
    assert!(w.eligibility.evaluate_eligibility(&pid, &student));
}

/// An issuer cannot mint an attestation that is already expired — the
/// cheapest possible way to grief an eligibility check would be to write a
/// permanent denial, and the contract refuses to create one.
#[test]
fn an_already_expired_attestation_cannot_be_issued() {
    let w = World::new();
    let pid = w.publish_program(8, "Backdated Attestation");
    let student = w.actors.student.clone();

    assert_eq!(
        w.eligibility.try_issue_attestation(
            &w.actors.registrar,
            &student,
            &pid,
            &w.attestation_type(),
            &START
        ),
        Err(Ok(EligError::InvalidExpiry))
    );
    assert_eq!(
        w.eligibility.try_issue_attestation(
            &w.actors.registrar,
            &student,
            &pid,
            &w.attestation_type(),
            &(START - 1)
        ),
        Err(Ok(EligError::InvalidExpiry))
    );
    assert!(!w
        .eligibility
        .has_valid_attestation(&student, &pid, &w.attestation_type()));
}

/// The award inventory is a counter, and counters are the thing an
/// attacker wants to draw down. Every way of trying to exceed it fails
/// without changing the committed total.
#[test]
fn the_award_inventory_cannot_be_drawn_down_twice() {
    let w = World::new();
    let admin = w.actors.admin.clone();
    let pid = w.publish_program(9, "Double Spend");

    w.programs.reserve_award(&admin, &pid);
    w.programs.reserve_award(&admin, &pid);
    w.programs.reserve_award(&admin, &pid);
    w.programs.reserve_award(&admin, &pid);
    assert_eq!(w.programs.remaining_capacity(&pid), 0);

    for _ in 0..3 {
        assert_eq!(
            w.programs.try_reserve_award(&admin, &pid),
            Err(Ok(ProgError::CapacityExceeded))
        );
    }
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 4);
    assert_eq!(budget.committed_amount, 4_000);

    // Nor can the budget be reconfigured mid-batch to mint capacity. A
    // replacement would otherwise zero `awarded_count` and
    // `committed_amount`, handing back four slots that are genuinely
    // committed and letting the program over-commit against its own
    // `total_budget` with a single call.
    assert_eq!(
        w.programs
            .try_configure_award_budget(&admin, &pid, &8, &1_000, &8_000),
        Err(Ok(ProgError::BudgetAlreadyCommitted))
    );
    let budget = w.programs.get_award_budget(&pid);
    assert_eq!(budget.awarded_count, 4);
    assert_eq!(budget.committed_amount, 4_000);
    assert_eq!(budget.total_budget, 4_000);
    assert_eq!(w.programs.remaining_capacity(&pid), 0);

    // Once every reservation is released, the budget is free to be
    // reconfigured again — the guard is about committed funds, not about
    // having configured once.
    for _ in 0..4 {
        w.programs.release_award(&admin, &pid);
    }
    w.programs
        .configure_award_budget(&admin, &pid, &8, &1_000, &8_000);
    assert_eq!(w.programs.remaining_capacity(&pid), 8);
}

/// A program id is the tenancy boundary. Two programs that happen to share
/// a title, a form, a sponsor, and even a budget are still separate
/// tenants, and nothing crosses between them.
#[test]
fn programs_are_isolated_tenants() {
    let w = World::new();
    let student = w.actors.student.clone();
    let a = w.publish_program(10, "Shared Title");
    let b = w.publish_program(11, "Shared Title");

    // Attestation scoped to A.
    w.attest(&student, &a, WINDOW);
    assert!(w.eligibility.evaluate_eligibility(&a, &student));
    assert!(
        !w.eligibility.evaluate_eligibility(&b, &student),
        "an attestation scoped to program A must not satisfy program B"
    );

    // Consent recorded against A.
    w.applications.record_consent(&student, &a);
    w.applications
        .submit_application(&student, &a, &h(&w, 1), &1);
    assert!(w.applications.has_applied(&student, &a));
    assert!(
        !w.applications.has_applied(&student, &b),
        "an application is keyed by (applicant, program)"
    );

    // Both programs carry their own budget, so the same sponsor cannot
    // spend A's inventory on B's applicants.
    assert_eq!(w.programs.remaining_capacity(&a), 4);
    assert_eq!(w.programs.remaining_capacity(&b), 4);
    w.programs.reserve_award(&w.actors.admin, &a);
    assert_eq!(w.programs.remaining_capacity(&a), 3);
    assert_eq!(w.programs.remaining_capacity(&b), 4);
}

// ── stored-content abuse ────────────────────────────────────────────────────

/// Bounded writes: a program title, description, and eligibility rule are
/// all length-capped, so a single account cannot fill the ledger with
/// megabytes of text and price everyone else out.
#[test]
fn oversized_stored_content_is_refused() {
    let w = World::new();
    let sponsor = w.actors.sponsor.clone();
    let admin = w.actors.admin.clone();
    let long = "x".repeat(300);
    let too_long = "x".repeat(5_000);

    assert_eq!(
        w.core.try_create_program(
            &sponsor,
            &h(&w, 1),
            &h(&w, 2),
            &SorobanString::from_str(&w.env, ""),
            &SorobanString::from_str(&w.env, "Empty title."),
            &Symbol::new(&w.env, "USDC"),
            &FundingModel::FixedAward,
        ),
        Err(Ok(CoreError::InvalidTitle))
    );
    assert_eq!(
        w.core.try_create_program(
            &sponsor,
            &h(&w, 1),
            &h(&w, 2),
            &SorobanString::from_str(&w.env, &long),
            &SorobanString::from_str(&w.env, "Title over the cap."),
            &Symbol::new(&w.env, "USDC"),
            &FundingModel::FixedAward,
        ),
        Err(Ok(CoreError::InvalidTitle))
    );
    assert_eq!(
        w.core.try_create_program(
            &sponsor,
            &h(&w, 1),
            &h(&w, 2),
            &SorobanString::from_str(&w.env, "Fine"),
            &SorobanString::from_str(&w.env, &too_long),
            &Symbol::new(&w.env, "USDC"),
            &FundingModel::FixedAward,
        ),
        Err(Ok(CoreError::InvalidDescription))
    );
    // Nothing was written by any of the three attempts.
    assert_eq!(
        w.core.try_get_program(&h(&w, 1)),
        Err(Ok(CoreError::ProgramNotFound))
    );

    // The same applies to eligibility rules: a rule with no requirements
    // would make every applicant eligible, so an empty one is refused.
    let empty = soroban_sdk::Vec::new(&w.env);
    let pid = w.publish_program(12, "Empty Rule");
    assert_eq!(
        w.eligibility
            .try_publish_eligibility_rule(&admin, &pid, &empty),
        Err(Ok(EligError::EmptyRule))
    );
}

/// The privacy-minimisation claim is testable: what the chain stores for an
/// application is a hash, a version, and a status — never the content. The
/// application record has no field a name, an email, or a free-text answer
/// could be smuggled into.
#[test]
fn what_the_chain_stores_about_an_applicant_is_a_commitment_and_nothing_more() {
    let w = World::new();
    let pid = w.publish_program(13, "Privacy");
    let student = w.actors.student.clone();

    w.prepare_student(&student, &pid, WINDOW);
    w.applications
        .submit_application(&student, &pid, &h(&w, 42), &1);

    let application = w.applications.get_application(&student, &pid);
    assert_eq!(application.applicant, student);
    assert_eq!(application.program_id, pid);
    assert_eq!(application.data_hash, h(&w, 42));
    assert_eq!(application.form_version, 1);
    assert_eq!(application.consent_version, 1);
    assert_eq!(
        application.status,
        scholarship_applications::ApplicationStatus::Submitted
    );

    // The consent record is equally minimal: a version and two timestamps.
    let consent = w.applications.get_consent(&student, &pid);
    assert_eq!(consent.version, 1);
    assert_eq!(consent.accepted_at, START);
    assert!(!consent.revoked);

    // A published schema is a hash, not the schema.
    let schema = w.applications.get_form_schema(&pid, &1);
    assert_eq!(schema.schema_hash, h(&w, 13));
    assert_eq!(schema.published_at, START);
    assert_eq!(schema.version, 1);

    // And the fixture's own address is a random key, not a real identity:
    // an applicant address in a test is `Address::generate`, which is what
    // a production wallet looks like to the contract.
    let generated = Address::generate(&w.env);
    w.prepare_student(&generated, &pid, WINDOW);
    w.applications
        .submit_application(&generated, &pid, &h(&w, 43), &1);
    assert_eq!(
        w.applications.get_application(&generated, &pid).applicant,
        generated
    );
}
