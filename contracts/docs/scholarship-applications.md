# Scholarship Applications Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1070 (form schema), #1073 (consent), #1074 (duplicate
  prevention — foundation), #1075 (atomic submission — foundation), #1133
  (correction and erasure requests), #1071
  (answer validation — not yet implemented), #1072 (document upload — not
  yet implemented), #1076 (withdrawal — not yet implemented), #1077
  (tamper-evident receipts — not yet implemented)

## Scope

This contract (`contracts/scholarship-applications`) implements four of the
"Scholarships On-chain/Applications" epic's behaviors:

- **#1074 — Prevent duplicate applications per program.** Structural: an
  application is stored under the key `(applicant, program_id)`, so a
  second `submit_application` for the same pair fails with
  `DuplicateApplication` before any state changes.
- **#1075 — Submit applications atomically.** All eligibility checks
  (program active, deadline, form-version validity, current-and-valid
  consent, uniqueness) run before any write; Soroban invocations are
  themselves atomic, so a failed check reverts the whole call.
- **#1070 — Configurable application forms.** `publish_form_schema()`
  publishes an immutable, versioned commitment (`schema_hash`) to a
  program's form structure. Versions are never overwritten — publishing
  again always creates version N+1 — and every `Application` records the
  `form_version` its answers were validated against off-chain, so answers
  "remain tied to the accepted form version" even after a newer version
  ships.
- **#1073 — Collect explicit applicant consent.** `publish_consent_terms()`
  publishes an immutable, versioned commitment to a program's terms/privacy
  notice/data-sharing/sponsor-disclosure bundle. `record_consent()` is an
  affirmative, timestamped, per-version action; `revoke_consent()` lets an
  applicant withdraw consent (it does not retroactively invalidate a
  submission already made under it — see Migration/Operational notes
  below); and `has_valid_consent()`/`submit_application()` require the
  applicant's consent to match the program's _current_ terms version, so
  publishing new terms automatically forces re-consent before any further
  submission.

**Not implemented here** (tracked separately): rich answer validation with
word limits and normalized text (#1071) and secure, encrypted, access-logged
document upload (#1072). Both are fundamentally off-chain-heavy concerns —
scanning, encryption, size/type limits, and access logging aren't things a
Soroban contract can itself perform — so they need an explicit off-chain
service boundary that this pass didn't design. Withdrawal (#1076) and
tamper-evident receipts (#1077) also remain unimplemented, as noted in the
previous pass's PR on this contract.

## Privacy

On-chain storage never holds form schemas, consent-terms text, or
application answers. Every one of those is represented on-chain only as a
caller-supplied `BytesN<32>` integrity commitment (`schema_hash`,
`terms_hash`, `data_hash`) over off-chain content. Consent records store
only a version number, timestamps, and a revoked flag — never the terms
text itself or any applicant-specific answer to it.

## Ownership

The registering admin (`initialize`) is the only party able to register
programs and publish form-schema/consent-terms versions. Applicants require
their own signature to record or revoke consent and to submit — no admin or
relayer can do either on an applicant's behalf.

The mutable `Program` record carries a checked revision. Admin
`set_program_active` calls must supply the revision they read and return the
new one; a stale revision returns `VersionConflict` before storage or events
change. Application submissions remain immutable and applicant-authorized.

## Migration

New contract, no prior on-chain state. A program must have both a
published form schema and published consent terms before any applicant can
successfully submit against it (`submit_application` fails with
`NoFormSchemaPublished`/`FormSchemaNotFound` or `NoConsentTermsPublished`
otherwise) — the admin flow is: `register_program` →
`publish_form_schema` → `publish_consent_terms` → applicants can then
`record_consent` and `submit_application`.
Existing deployments must migrate program records to a revision baseline
before enabling the changed ABI. Clients read the current program and pass
its revision to `set_program_active`; after `VersionConflict`, reload and
reconcile rather than replaying stale state.

## Operational impact

- Publishing a new consent-terms version immediately invalidates every
  applicant's existing consent for that program (`has_valid_consent`
  becomes `false` for all of them) until they re-consent. This is
  intentional per the issue's "changes trigger appropriate re-consent"
  criterion, but it means a terms update mid-intake-window will block
  submission for anyone who consented earlier, until they return and
  re-consent — plan terms changes accordingly, and consider notifying
  affected applicants off-chain when this happens.
- Consent revocation only affects _future_ submissions; it intentionally
  does not touch or invalidate an `Application` record that already exists
  (submissions are immutable once made, consistent with "cannot erase
  review history" elsewhere in this epic). An applicant who revokes after
  submitting has revoked consent for any _next_ action requiring fresh
  consent, not for the completed submission itself — this asymmetry should
  be surfaced clearly in any UI built on this contract.
- Storage/TTL/events follow the same conventions as the previous pass on
  this contract (persistent storage, ~1-year TTL bump, no
  upgrade/pause admin function yet — see the prior PR's note on that).
- Revision metadata is not applicant data and does not change the off-chain
  trust boundary: answers, identity, and documents remain outside the
  contract. Admin authorization and persistent-record TTL policy are
  unchanged; deployment tooling must account for the ABI/schema migration.
- Related issues: #1074 (duplicate prevention), #1075 (atomic submission),
  #1076 (withdrawal — not yet implemented), #1077 (tamper-evident receipts —
  not yet implemented)

## Applicant data access and export

`request_data_export(applicant, program_id)` requires the applicant's
authorization and an existing application or consent record. Only one export
request may be pending for an applicant/program pair. Applicants may cancel a
pending request; the configured privacy admin completes it with separate
`export_hash` and `consent_history_hash` commitments. The status, timestamps,
applicant, program, and hashes are available through
`get_data_export_request(request_id)`; `get_latest_data_export_request_id`
provides discovery by applicant and program. `DATAEXP/REQUEST`,
`DATAEXP/CANCEL`, and `DATAEXP/COMPLETE` events provide an indexable request
lifecycle.

Consent acceptance is recorded by `CONSENT` events and revocation by
`CONSRVK`; indexers use these events to reconstruct the history because the
on-chain `ConsentRecord` is only the latest state. The consent-history hash
commits to the machine-readable history included in the export.

### Off-chain trust boundary

The contract does not store or return application answers, identity records,
uploaded documents, or a serialized export. An authenticated off-chain export
service must verify the applicant, retrieve only that applicant's data for the
requested program, include the consent event history, serialize a documented
machine-readable format, compute the two commitments, and deliver the package
through a secure applicant-authenticated channel. The privacy admin's
completion transaction attests to those commitments; it does not prove that
the service delivered the package or that the package is complete. The
applicant should verify both hashes against the received package. Service
access logging, encryption, retention, and delivery authorization remain
off-chain responsibilities.

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
