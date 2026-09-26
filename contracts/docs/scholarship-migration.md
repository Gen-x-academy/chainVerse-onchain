# Scholarship Migration Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issue: #1143 (migrate legacy financial-aid records), #1056
  (platform boundaries — see
  `docs/adr/0002-scholarships-on-chain-boundaries.md`)

## Scope

This contract (`contracts/scholarship-migration`) brings legacy
financial-aid application records into the scholarship domain **without
losing history**.

"Losing history" is the whole risk. A migrated record has to stay traceable
to the record it came from, with its original identifier and its original
timestamp intact. A migration that quietly renumbers identifiers or stamps
everything with the migration date destroys the evidence that an award was
applied for, when, and on what basis — and it does so in a way nobody
notices until a dispute.

### What it records, and what it does not do

It **records** the mapping. It does not create scholarship applications, and
it cannot: ADR 0002 forbids the scholarship contracts from calling one
another, so a migration contract cannot reach into `scholarship-applications`
any more than the outbox can.

The orchestrator applies the real domain write and this mapping record in
**one Stellar transaction** — the same atomicity pattern the outbox uses. The
chain proves a mapping was recorded alongside a committed state change; it
does not perform the change, and does not claim to.

## Resumable

A legacy dataset does not fit in one transaction, so a migration is
necessarily many transactions, and any such sequence has to survive being
interrupted.

A **run** carries a checkpoint that advances as records are processed, so a
re-run asks the legacy system for "everything after N" rather than
re-reading what it already gave us. The checkpoint is the legacy system's
own ordering, which is why a run is scoped to a single `source_system`:
merging two systems' orderings would make the cursor meaningless.

Re-offering a record at or before the checkpoint is answered from the record
rather than rejected. A resume must not be able to trip over its own earlier
work — that is the whole point of resumability — and the checkpoint itself
never rewinds, or a resume would replay the tail of the dataset.

A record the run has *not* mapped, arriving at a stale cursor, is refused
with `NotMapped` rather than quietly mapped. That is a data problem the
operator needs to see, not something to absorb silently.

## Dry-runnable

`begin_run` takes the mode, and a dry run **cannot write a mapping** no
matter what it is later asked to do. The report a dry run produces comes
from exactly the code that would do the migration, so it cannot drift from
what a real run would do.

A dry run still advances its checkpoint — so the operator can see how far the
dataset actually goes and size the real run — and still reports unmapped
records, because the report is the entire value of a rehearsal. It does not
increment `mapped`, so a rehearsal's numbers cannot be mistaken for a real
run's.

**The mode is fixed for the life of the run and has no setter.** A run
promoted from rehearsal to real halfway through would leave part of the
dataset mapped while the operator believed the whole thing had been
rehearsed, which is the specific failure a dry run exists to prevent.

## Idempotent, in the strong sense

Mapping the same `legacy_id` with the same contents returns the recorded
state and **writes nothing**. The tests assert whole-record equality, not
field spot checks, because a partial guarantee is not a guarantee. The
`mapped` counter tracks mappings rather than calls, so three attempts at one
record report one mapping — a count that tracked calls would report a
migration three times larger than reality.

## Preserving legacy identifiers and timestamps

This is enforced, not just intended, and the enforcement is the interesting
part. A legacy id is a **primary key**, so the same id arriving with
different contents or a different `submitted_at` is a `RecordConflict`, not
an update. Without that rule, "we preserve timestamps" would mean *we store
a timestamp* — and whichever copy of a record arrived last would win. The
test that re-offers a mapped record with an edited submission time is the one
that makes the claim true.

`submitted_at` is the legacy system's own value and is never replaced with
the migration time. The migration time is recorded separately, in
`mapped_at`, and the two are distinct fields with distinct names.

## Reporting unmapped data

Six `UnmappableReason` codes: `NotEligible`, `NoMatchingProgram`,
`DuplicateInLegacy`, `MalformedRecord`, `WithdrawnByApplicant`,
`OutsideRetentionWindow`.

**Codes, never free text.** A reason string on chain would be a place for
applicant information to end up, and would cost more per record than the
mapping it is explaining.

The **count is exact and uncapped**; only the browsable detail is capped at
1,000 items per run. A dataset with a systematic problem must not be able to
hide inside a truncated list, so `unmapped_total` keeps climbing past the
retention cap while `get_reported(run, index)` covers the first 1,000. An
operator always learns the true size and can investigate the remainder
off-chain.

`RunSummary` separates `mapped` from `unmapped` and is what `seal_run`
returns — a seal is the moment an operator decides whether the migration is
complete, so the outstanding count has to be in the result.

## Privacy

Legacy application records contain exactly the data this platform exists not
to put on chain: names, addresses, free-text answers, documents. This
contract stores a `payload_hash` and nothing else about a record's contents.

The eligibility verdict is a **code**, never a score, a ranking, or a
probability. A stored likelihood would be a derived fact about a person that
leaks far more than the answer it was meant to help with — and it would
become a permanent record of a judgment made once, about someone, at a
moment when the criteria were different.

## Ownership

A single platform operator authorizes every write: beginning a run, mapping,
reporting, sealing, and archiving. A migration rewrites history, so it is
deliberately the narrowest authority in the platform.

Reads are unauthenticated: `get_run`, `run_summary`, `get_record`,
`record_state`, `get_reported`, and the counters. Migration progress is
operational information an applicant should be able to check.

## Migration

New contract, no prior on-chain state, so there is nothing to migrate *into*.

Migrating *off* a deployment means pointing the orchestrator at the new
address. Because mapping is idempotent and the legacy id is a primary key, a
failover mid-run is safe: re-running against the new contract re-proposes
records that were never mapped and produces no duplicates. A run that was
already sealed stays sealed.

Note that `dedup_key` in the outbox is per-deployment for the same reason —
see `scholarship-outbox.md`.

## Operational impact

- **Run a dry run first, and read the whole summary.** The unmapped count is
  the number that matters. A migration where 30% of records cannot be mapped
  is a data-quality problem, not a migration.
- **Migrate in batches and seal deliberately.** `seal_run` is the decision
  point; after it, the run accepts nothing further. Sealing is idempotent, so
  a retry after a crash mid-seal is safe.
- **`tracked_records` is capped at 50,000 and `archive_record` is the
  release valve.** Archiving drops a row from the browsable table but leaves
  the archival event and the record's `Archived` state, so the mapping stays
  legible. Only `Mapped` records may be archived, so an unresolved problem
  cannot be discarded to make room. Past 50,000 mapped records, archive the
  finished ones.
- **Preserved timestamps mean old data stays old.** Anything downstream that
  infers an award's age from `submitted_at` will correctly see records that
  predate the platform. That is the intended behaviour, and a pipeline
  expecting recent dates needs to handle it.
- **A run's `source_system` is fixed.** Records from another legacy system
  are refused. Two systems means two runs, with separate checkpoints and
  separate reports — do not try to interleave them.
