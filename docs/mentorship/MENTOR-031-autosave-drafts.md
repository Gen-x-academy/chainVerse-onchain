# MENTOR-031: Autosave Application Drafts

**Track:** Applications | **Component:** on-chain | **Status:** implemented

Goal: Autosave application drafts.

## On-chain

`autosave_draft(applicant, program_id, base_revision, request_token, updates)`
persists a resumable `Draft` keyed by `(applicant, program)` with:

- a monotonic `revision` (optimistic concurrency token);
- the `request_token` of the most recent successful save (idempotency key);
- per-field `DraftAnswer { question_id, answer_hash, updated_at }` entries, so
  every field carries its own last-updated ledger timestamp;
- the form version the draft was created against.

It returns `DraftReceipt { revision, saved_at, request_token, replayed }` —
explicit last-saved feedback for the client.

### Semantics

- `base_revision = 0` creates revision 1. A create racing an existing draft
  fails with `DraftConflict` rather than overwriting it.
- Any other `base_revision` must equal the stored revision; otherwise the call
  fails with `DraftConflict` and the newer edit is preserved.
- If `request_token` equals the last successful save's token, the call is an
  idempotent replay: it returns `replayed: true` with the original revision and
  timestamp and changes nothing. This is the offline-recovery path — a client
  that never saw an acknowledgement can resend safely, even with a now-stale
  base revision.
- `answer_hash: None` clears a field; clearing an absent field is a no-op.
- Drafts bind to the latest published form version at creation and keep it if a
  newer form ships. Answers must reference questions in the bound version.

`discard_draft` uses the same revision check, so a stale client cannot delete a
newer draft.

## Off-chain boundary

The draft's actual answer values, labels, help text and attachments live
off-chain; only answer commitments and per-field timestamps are stored. The
client is responsible for generating a fresh `request_token` per logical save
and for resending the same token on retries.

## Rules

- Retries are idempotent: a replayed token has no effect and returns the same
  receipt.
- Concurrent edits do not silently overwrite: a stale `base_revision` fails
  with `DraftConflict`.
- Offline recovery is safe: replay is accepted without applying partial state,
  and a later save with the correct revision continues normally.
- Submitted applications are immutable: submission removes the draft, autosave
  afterwards fails with `DuplicateApplication`, and `get_draft` returns
  `DraftNotFound`.
- Drafts respect the program window (`ProgramInactive`, `DeadlinePassed`) and
  require a published form (`NoFormPublished`).
- Storage is bounded at 32 answers per draft; more than 32 patches in one save
  fails with `TooManyAnswers`. Unknown ids, duplicate patch ids and zero
  commitments fail with `AnswerNotInForm`, `DuplicateAnswer` and
  `InvalidAnswerHash`.

## Events and errors

Event `DRAFT(applicant, program_id, revision, replayed)` per accepted save and
`DRFTDEL(applicant, program_id)` on discard. Errors: `DraftNotFound`,
`DraftConflict`, `ProgramNotFound`, `ProgramInactive`, `DeadlinePassed`,
`NoFormPublished`, `FormVersionNotFound`, `DuplicateApplication`,
`TooManyAnswers`, `DuplicateAnswer`, `AnswerNotInForm`, `InvalidAnswerHash`,
`VersionOverflow`.

## Dependencies and rollout

Depends on MENTOR-032's published forms (the draft binds to the latest version)
and shares the MENTOR-030 uniqueness key so submission closes drafts. The
matching MENTOR-031 work in the other repositories owns the debounced autosave
client, local offline queue, token generation and conflict-resolution UI. Ships
behind the mentorship feature flag; the revision/token fields are additive to
new storage only (no migration of pre-existing drafts, since none exist).
Full contract notes: `contracts/docs/mentorship-applications.md`.
