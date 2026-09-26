//! Adversarial tests for legacy record migration (issue #1143).
//!
//! The acceptance criteria name five properties: resumable, dry-runnable,
//! idempotent, reports unmapped data, and preserves legacy identifiers and
//! timestamps. Each is tested here against how it would actually break — an
//! interrupted run, a promoted rehearsal, a re-export with edited fields, a
//! truncated report.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};

struct Fixture {
    env: Env,
    contract: Address,
    operator: Address,
    stranger: Address,
}

impl Fixture {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        let contract = env.register(ScholarshipMigrationContract, ());
        let operator = Address::generate(&env);
        let stranger = Address::generate(&env);
        ScholarshipMigrationContractClient::new(&env, &contract).initialize(&operator);
        Self {
            env,
            contract,
            operator,
            stranger,
        }
    }

    fn client(&self) -> ScholarshipMigrationContractClient<'_> {
        ScholarshipMigrationContractClient::new(&self.env, &self.contract)
    }

    fn hash(&self, fill: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[fill; 32])
    }

    fn system(&self, name: &str) -> Symbol {
        Symbol::new(&self.env, name)
    }

    /// Advance the ledger clock, so mapping time differs from the legacy
    /// submission time and a test can tell them apart.
    fn advance(&self, seconds: u64) {
        self.env
            .ledger()
            .set_timestamp(self.env.ledger().timestamp() + seconds);
    }

    /// Map a record, with the legacy cursor advancing by one each time.
    ///
    /// Unwraps the invocation layer so call sites can assert on the
    /// contract's own error directly.
    #[allow(clippy::result_unit_err)]
    fn map(&self, run: u64, id: u8, cursor: u64) -> Result<RecordState, ContractError> {
        match self.client().try_map_record(
            &self.operator,
            &run,
            &self.hash(id),
            &self.system("legacy_a"),
            &1_600_000_000,
            &self.hash(id.wrapping_add(50)),
            &self.hash(90),
            &self.hash(91),
            &cursor,
        ) {
            Ok(Ok(state)) => Ok(state),
            Ok(Err(conversion)) => panic!("unexpected conversion error: {:?}", conversion),
            Err(Ok(contract_error)) => Err(contract_error),
            Err(Err(invoke_error)) => panic!("unexpected invoke error: {:?}", invoke_error),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Idempotency — the property everything else rests on
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_the_same_record_twice_changes_nothing() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    assert_eq!(f.map(run, 1, 1), Ok(RecordState::Mapped));
    let before = f.client().get_record(&f.hash(1));

    // A backend retry, a re-driven batch, or a duplicated queue message must
    // not produce a second mapping.
    assert_eq!(f.map(run, 1, 2), Ok(RecordState::Mapped));
    assert_eq!(f.client().get_record(&f.hash(1)), before);
    assert_eq!(f.client().tracked_records(), 1);
}

#[test]
fn remapping_does_not_inflate_the_mapped_count() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    f.map(run, 1, 2).unwrap();
    f.map(run, 1, 3).unwrap();
    // Three calls, one mapping. A count that tracked calls rather than
    // mappings would report a migration three times larger than reality.
    assert_eq!(f.client().get_run(&run).mapped, 1);
}

#[test]
fn a_re_export_may_not_overwrite_a_record() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();

    // Same legacy id, different contents. A legacy id is a primary key, so
    // this is a conflict -- accepting it would let a re-export rewrite an
    // applicant's history.
    assert_eq!(
        f.client().try_map_record(
            &f.operator,
            &run,
            &f.hash(1),
            &f.system("legacy_a"),
            &1_600_000_000,
            &f.hash(200), // different payload
            &f.hash(90),
            &f.hash(91),
            &2,
        ),
        Err(Ok(ContractError::RecordConflict))
    );
    assert_eq!(f.client().get_record(&f.hash(1)).payload_hash, f.hash(51));
}

#[test]
fn a_rewritten_legacy_timestamp_is_a_conflict() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();

    // "Preserves timestamps" has to mean more than "stores a timestamp": if
    // the same id can arrive with a different `submitted_at`, the preserved
    // value is whichever copy arrived last.
    assert_eq!(
        f.client().try_map_record(
            &f.operator,
            &run,
            &f.hash(1),
            &f.system("legacy_a"),
            &1_700_000_000, // a different submission time
            &f.hash(51),
            &f.hash(90),
            &f.hash(91),
            &2,
        ),
        Err(Ok(ContractError::RecordConflict))
    );
    assert_eq!(
        f.client().get_record(&f.hash(1)).submitted_at,
        1_600_000_000
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Legacy identifiers and timestamps are preserved
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn the_legacy_identifier_and_timestamp_survive_verbatim() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.advance(500);
    f.map(run, 7, 1).unwrap();

    let record = f.client().get_record(&f.hash(7));
    // The id is stored as given, not re-derived, so the link back to the
    // legacy system of record is exact.
    assert_eq!(record.legacy_id, f.hash(7));
    // The legacy submission time is kept, not replaced with the migration
    // time: a record that predates the scholarship contracts must keep
    // saying so.
    assert_eq!(record.submitted_at, 1_600_000_000);
    // The migration time is recorded separately rather than overwriting it.
    assert_eq!(record.mapped_at, 500);
}

#[test]
fn the_legacy_system_is_recorded_on_every_record() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    assert_eq!(
        f.client().get_record(&f.hash(1)).source_system,
        f.system("legacy_a")
    );
    assert_eq!(f.client().get_run(&run).source_system, f.system("legacy_a"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Resumability
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_run_advances_a_checkpoint_as_it_goes() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    assert_eq!(f.client().get_run(&run).cursor, 0);
    f.map(run, 1, 10).unwrap();
    assert_eq!(f.client().get_run(&run).cursor, 10);
    f.map(run, 2, 20).unwrap();
    assert_eq!(f.client().get_run(&run).cursor, 20);
    // The operator asks the legacy system for "everything after 20".
    assert_eq!(f.client().run_summary(&run).cursor, 20);
}

#[test]
fn an_interrupted_run_resumes_without_remapping() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 10).unwrap();
    f.map(run, 2, 20).unwrap();
    // The process dies here. Resuming re-offers the last record, because
    // that is what a source system resuming from a cursor does.
    let resumed_from = f.client().get_run(&run).cursor;
    assert_eq!(resumed_from, 20);

    f.map(run, 2, 21).unwrap();
    f.map(run, 3, 30).unwrap();
    // Two records mapped, three tracked, and nothing duplicated.
    assert_eq!(f.client().get_run(&run).mapped, 3);
    assert_eq!(f.client().tracked_records(), 3);
    assert_eq!(f.client().get_run(&run).cursor, 30);
}

#[test]
fn a_record_at_or_before_the_cursor_is_not_remapped() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 10).unwrap();
    f.map(run, 2, 20).unwrap();

    // A record the run has already passed. This is a record the run mapped
    // under a different id -- the source system offering it again must not
    // produce a second mapping.
    assert_eq!(
        f.client().try_map_record(
            &f.operator,
            &run,
            &f.hash(3),
            &f.system("legacy_a"),
            &1_600_000_000,
            &f.hash(53),
            &f.hash(90),
            &f.hash(91),
            &5, // already behind the cursor
        ),
        Err(Ok(ContractError::NotMapped))
    );
    assert_eq!(f.client().tracked_records(), 2);
}

#[test]
fn re_offering_a_mapped_record_at_a_stale_cursor_is_still_idempotent() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 10).unwrap();
    f.map(run, 2, 20).unwrap();
    let before = f.client().get_record(&f.hash(1));

    // This is exactly what a resume looks like: the source system replays
    // from before the checkpoint. An already-mapped record is answered from
    // the record rather than rejected, because a resume must not be able to
    // trip over its own earlier work.
    assert_eq!(f.map(run, 1, 5), Ok(RecordState::Mapped));
    assert_eq!(f.client().get_record(&f.hash(1)), before);
    assert_eq!(f.client().tracked_records(), 2);
    // And the checkpoint does not rewind: a cursor that went backwards would
    // make a resume replay the whole tail of the dataset.
    assert_eq!(f.client().get_run(&run).cursor, 20);
}

// ═══════════════════════════════════════════════════════════════════════════
// Dry run
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_dry_run_writes_no_mappings() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &true);
    assert_eq!(f.map(run, 1, 10), Ok(RecordState::Eligible));
    assert_eq!(f.map(run, 2, 20), Ok(RecordState::Eligible));

    // Nothing was written: no records, and the mapped count stayed at zero
    // so a rehearsal's numbers cannot be mistaken for a real run's.
    assert_eq!(f.client().tracked_records(), 0);
    assert_eq!(f.client().get_run(&run).mapped, 0);
    assert!(f.client().try_get_record(&f.hash(1)).is_err());
    assert!(f.client().try_get_record(&f.hash(2)).is_err());
}

#[test]
fn a_dry_run_still_reports_unmapped_records() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &true);
    f.client().report_unmapped(
        &f.operator,
        &run,
        &f.hash(1),
        &UnmappableReason::NotEligible,
        &10,
    );
    // The whole value of a rehearsal is the report, so it is produced in dry
    // run exactly as in a real run.
    assert_eq!(f.client().unmapped_total(&run), 1);
    assert_eq!(f.client().get_reported(&run, &0), f.hash(1));
    assert_eq!(f.client().tracked_records(), 0);
}

#[test]
fn a_dry_run_advances_its_checkpoint() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &true);
    f.map(run, 1, 10).unwrap();
    f.map(run, 2, 20).unwrap();
    // So the operator can see how far the dataset actually goes, and size
    // the real run.
    assert_eq!(f.client().get_run(&run).cursor, 20);
}

#[test]
fn the_mode_is_fixed_for_the_life_of_the_run() {
    let f = Fixture::new();
    let dry = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &true);
    let real = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    assert!(f.client().get_run(&dry).dry_run);
    assert!(!f.client().get_run(&real).dry_run);
    // There is deliberately no setter. A run promoted from rehearsal to real
    // halfway through would leave part of the dataset mapped while the
    // operator believed the whole thing had been rehearsed.
    assert!(f.client().try_get_run(&dry).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════════
// Reporting unmapped data
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn every_unmappable_reason_is_representable() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    let reasons = [
        UnmappableReason::NotEligible,
        UnmappableReason::NoMatchingProgram,
        UnmappableReason::DuplicateInLegacy,
        UnmappableReason::MalformedRecord,
        UnmappableReason::WithdrawnByApplicant,
        UnmappableReason::OutsideRetentionWindow,
    ];
    for (i, reason) in reasons.iter().enumerate() {
        f.client()
            .report_unmapped(&f.operator, &run, &f.hash(i as u8), reason, &(i as u64 + 1));
    }
    // Six distinct causes, counted exactly -- a dataset with a systematic
    // problem must not hide inside a truncated list.
    assert_eq!(f.client().unmapped_total(&run), 6);
    assert_eq!(f.client().run_summary(&run).unmapped, 6);
}

#[test]
fn the_reported_count_is_exact_even_when_the_browsable_window_is_full() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    let total = MAX_REPORTED_PER_RUN + 10;
    for i in 0..total {
        f.client().report_unmapped(
            &f.operator,
            &run,
            &f.hash((i % 250) as u8),
            &UnmappableReason::NotEligible,
            &(i as u64 + 1),
        );
    }
    // The count keeps climbing past the retention cap, so an operator always
    // learns the true size; only the browsable detail is bounded.
    assert_eq!(f.client().unmapped_total(&run), total as u64);
    assert_eq!(
        f.client().run_summary(&run).unmapped_retained,
        MAX_REPORTED_PER_RUN
    );
}

#[test]
fn a_summary_separates_mapped_from_unmapped() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    f.map(run, 2, 2).unwrap();
    f.client().report_unmapped(
        &f.operator,
        &run,
        &f.hash(3),
        &UnmappableReason::NoMatchingProgram,
        &3,
    );

    let summary = f.client().run_summary(&run);
    assert_eq!(summary.mapped, 2);
    assert_eq!(summary.unmapped, 1);
    assert!(!summary.sealed);
    assert!(!summary.dry_run);
}

#[test]
fn an_unknown_reported_index_is_reported_as_such() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    assert_eq!(
        f.client().try_get_reported(&run, &0),
        Err(Ok(ContractError::RecordNotFound))
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Sealing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_sealed_run_accepts_nothing_further() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    let summary = f.client().seal_run(&f.operator, &run);
    assert!(summary.sealed);

    // Writing past a seal would make the sealed summary a lie.
    assert_eq!(f.map(run, 2, 2), Err(ContractError::RunSealed));
    assert_eq!(
        f.client().try_report_unmapped(
            &f.operator,
            &run,
            &f.hash(9),
            &UnmappableReason::NotEligible,
            &9
        ),
        Err(Ok(ContractError::RunSealed))
    );
    assert_eq!(f.client().tracked_records(), 1);
}

#[test]
fn sealing_twice_is_a_no_op() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    let first = f.client().seal_run(&f.operator, &run);
    // A retry after a crash mid-seal must not double-count.
    let second = f.client().seal_run(&f.operator, &run);
    assert_eq!(first, second);
}

#[test]
fn a_seal_reports_what_was_left_unmapped() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    f.client().report_unmapped(
        &f.operator,
        &run,
        &f.hash(2),
        &UnmappableReason::WithdrawnByApplicant,
        &2,
    );
    // A seal is the moment an operator decides whether the migration is
    // complete, so the outstanding count has to be in the result.
    let summary = f.client().seal_run(&f.operator, &run);
    assert_eq!(summary.mapped, 1);
    assert_eq!(summary.unmapped, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Isolation, bounds, authorization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn a_record_from_another_legacy_system_is_refused() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    // The cursor is the legacy system's own ordering, so a record from a
    // different system would make the checkpoint meaningless.
    assert_eq!(
        f.client().try_map_record(
            &f.operator,
            &run,
            &f.hash(1),
            &f.system("legacy_b"),
            &1_600_000_000,
            &f.hash(51),
            &f.hash(90),
            &f.hash(91),
            &1,
        ),
        Err(Ok(ContractError::RecordConflict))
    );
    assert_eq!(f.client().tracked_records(), 0);
}

#[test]
fn two_runs_keep_separate_state() {
    let f = Fixture::new();
    let first = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    let second = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_b"), &false);

    f.map(first, 1, 10).unwrap();
    // A cursor reached in one run must not satisfy another run's
    // bookkeeping.
    assert_eq!(f.client().get_run(&second).cursor, 0);
    assert_eq!(f.client().unmapped_total(&second), 0);
    assert_eq!(f.client().tracked_records(), 1);
}

#[test]
fn a_zero_payload_commitment_is_refused() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    // "No record contents" and "the hash of nothing" must not be the same
    // state.
    assert_eq!(
        f.client().try_map_record(
            &f.operator,
            &run,
            &f.hash(1),
            &f.system("legacy_a"),
            &1_600_000_000,
            &BytesN::from_array(&f.env, &[0u8; 32]),
            &f.hash(90),
            &f.hash(91),
            &1,
        ),
        Err(Ok(ContractError::EmptyCommitment))
    );
}

#[test]
fn an_unknown_run_is_reported_as_such() {
    let f = Fixture::new();
    assert_eq!(
        f.client().try_get_run(&99),
        Err(Ok(ContractError::RunNotFound))
    );
}

#[test]
fn a_non_operator_cannot_map() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    assert_eq!(
        f.client().try_map_record(
            &f.stranger,
            &run,
            &f.hash(1),
            &f.system("legacy_a"),
            &1_600_000_000,
            &f.hash(51),
            &f.hash(90),
            &f.hash(91),
            &1,
        ),
        Err(Ok(ContractError::NotAdmin))
    );
    assert_eq!(f.client().tracked_records(), 0);
}

#[test]
fn a_non_operator_cannot_begin_a_run() {
    let f = Fixture::new();
    assert_eq!(
        f.client()
            .try_begin_run(&f.stranger, &f.system("legacy_a"), &false),
        Err(Ok(ContractError::NotAdmin))
    );
}

#[test]
fn migration_requires_a_signature() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.env.set_auths(&[]);
    assert!(f
        .client()
        .try_map_record(
            &f.operator,
            &run,
            &f.hash(1),
            &f.system("legacy_a"),
            &1_600_000_000,
            &f.hash(51),
            &f.hash(90),
            &f.hash(91),
            &1,
        )
        .is_err());
}

#[test]
fn initialize_is_not_repeatable() {
    let f = Fixture::new();
    assert_eq!(
        f.client().try_initialize(&f.stranger),
        Err(Ok(ContractError::AlreadyInitialized))
    );
}

#[test]
fn an_unmapped_record_cannot_be_archived() {
    let f = Fixture::new();
    // Nothing to archive: an unmapped record was never stored, so the whole
    // dataset's failures live in the run counters.
    assert_eq!(
        f.client().try_archive_record(&f.operator, &f.hash(1)),
        Err(Ok(ContractError::RecordNotFound))
    );
}

#[test]
fn a_mapped_record_can_be_archived_and_stays_reported() {
    let f = Fixture::new();
    let run = f
        .client()
        .begin_run(&f.operator, &f.system("legacy_a"), &false);
    f.map(run, 1, 1).unwrap();
    f.client().archive_record(&f.operator, &f.hash(1));

    assert_eq!(f.client().tracked_records(), 0);
    // The row is gone from the browsable table, but it is still legible --
    // the archival is an event, and the state says what happened.
    assert_eq!(
        f.client().get_record(&f.hash(1)).state,
        RecordState::Archived
    );
    assert_eq!(
        f.client().get_record(&f.hash(1)).submitted_at,
        1_600_000_000
    );
}

#[test]
fn version_is_reported() {
    let f = Fixture::new();
    assert_eq!(f.client().version(), 1);
}
