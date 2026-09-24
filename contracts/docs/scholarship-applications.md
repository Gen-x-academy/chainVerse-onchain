# Scholarship Applications Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1074 (duplicate prevention), #1075 (atomic submission),
  #1076 (withdrawal — not yet implemented), #1077 (tamper-evident receipts —
  not yet implemented)

## Scope

This contract (`contracts/scholarship-applications`) implements two of the
four behaviors tracked under the "Scholarships On-chain/Applications" epic:

- **#1074 — Prevent duplicate applications per program.** Enforced
  structurally: an application is stored under the key
  `(applicant, program_id)`, so a second `submit_application` call for the
  same pair fails with `DuplicateApplication` before any state changes.
- **#1075 — Submit applications atomically.** All eligibility checks
  (program exists and is active, deadline not passed, consent given,
  no prior application) run before any write. Soroban invocations are
  themselves atomic, so a failed check reverts the whole call — there is no
  path that leaves a partial or draft record behind, and a retried call
  after a transient failure cannot duplicate a prior success.

**Not implemented here** (tracked separately): withdrawal with
policy-aware consequences (#1076) and tamper-evident submission receipts
(#1077). Both build on this contract's `Application` record and are natural
follow-ups — #1076 would add a `Withdrawn` status and a reason code without
deleting the record (preserving audit history), and #1077 would derive a
receipt from the same `data_hash` commitment already stored here plus the
ledger sequence/timestamp of the submission.

Also out of scope for this pass: draft editing before submission (an
applicant currently either has no application or has one `Submitted`
record — there is no in-progress draft state to reconcile), and the
"controlled merge process" for duplicate drafts described in #1074's
acceptance criteria, since there is no draft state yet to merge.

## Privacy

This contract never stores application content. `submit_application` takes
a caller-supplied `data_hash: BytesN<32>` — an integrity commitment (e.g. a
hash) over the actual form answers and documents, which live entirely
off-chain. On-chain state only ever contains: the applicant's address, the
program ID, a submission timestamp, and that commitment. This keeps the
chain's public state free of PII while still letting anyone verify, given
the off-chain content, that it matches what was actually submitted.

## Ownership

The registering admin (set once via `initialize`) is the sole party able to
register programs and toggle their active state. Applicants require their
own signature (`require_auth`) to submit — no admin or relayer can submit
on an applicant's behalf, which matters for the "exact authorization"
requirement shared across this epic's issues.

## Migration

This is a new contract with no prior on-chain state to migrate. Programs
must be registered (`register_program`) after deployment before any
applicant can submit against them; there is no bulk-import path from an
off-chain system in this pass.

## Operational impact

- Storage: `Program` and `Application` records are `persistent` entries
  with a ~1-year TTL bump on write/read (matching `course_registry`'s
  convention), so an admin or indexer needs to periodically touch
  long-lived records (e.g. via `get_application`) if they must outlive
  that window without being restored from archival state.
- Events: a `SUBMIT` event is published on every successful submission,
  carrying `(applicant, program_id)`, for off-chain indexing.
- No upgrade/pause admin function exists yet in this contract (unlike
  `course_registry`'s `pause`/`upgrade`); adding one is recommended before
  this contract is used to accept real submissions, but was left out of
  this pass to keep the change bounded to #1074/#1075.
