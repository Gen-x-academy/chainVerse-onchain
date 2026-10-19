# Mentorship Applications Contract

- Status: Implemented (decisions and withdrawal/reapplication)
- Owner: Mentorship on-chain working group
- Contract: `contracts/mentorship-applications`
- Related issues: #1231 (**MENTOR-035**, structured application decisions),
  #1232 (**MENTOR-036**, withdrawal and reapplication)
- Depends on (off-chain or later on-chain work): MENTOR-030..034 (application
  intake, drafts, configurable questions, eligibility preflight, review
  queues), MENTOR-018 (program eligibility rules), MENTOR-038
  (matching/acceptance), MENTOR-044/MENTOR-091 (blocking), MENTOR-111
  (audit log), MENTOR-106..110 (privacy controls)

## Scope

This contract is the on-chain authority for *how an application moves
through review* once it exists. It deliberately keeps the intake surface
minimal (`open_application`) so the decision and withdrawal flows are
independently testable; the richer intake behaviours are tracked by
MENTOR-030..034 and are expected to compose with this contract rather than
duplicate its status machine.

### MENTOR-035 — structured application decisions

`record_decision(coordinator, application_id, action, DecisionInput)`
records one append-only decision for an application:

- **Actions.** `Review` (moves to `UnderReview`), `RequestInfo` (`InfoRequested`),
  `Waitlist` (`Waitlisted`), `Accept` (`Accepted`), `Reject` (`Rejected`).
- **Reason codes.** `public_reason` is a non-zero, stable, applicant-visible
  code. Free text is never stored.
- **Separated reasons.** `internal_reason_hash` commits to internal
  reasoning/notes that stay off-chain. It is mandatory for `Waitlist` and
  `Reject` and is never returned by the applicant-facing read. The full
  record is coordinator-gated (`decision`).
- **Effective time and communication.** `effective_at` is when the decision
  becomes effective for applicant communication (`0` means "now") and may
  not be backdated. `communication` records one of `NotifyImmediately`,
  `NotifyAtEffectiveTime`, `InternalOnly` for the off-chain notifier.
  On-chain state changes are atomic; `effective_at` does not delay them.
- **Valid transitions.** Decisions are accepted from `Submitted`,
  `UnderReview`, `InfoRequested`, and `Waitlisted`; `Accepted`, `Rejected`,
  and `Withdrawn` are terminal. Coordinators cannot record `Withdraw`
  (`WithdrawalIsApplicantAction`) — that path belongs to the applicant.
- **Duplicate decisions are safe.** Repeating a decision with the same
  action, public reason, and internal-reason commitment returns the original
  sequence number without appending history or emitting an event.
- **Immutable history.** Every decision is stored under
  `(application_id, seq)` and is never overwritten. History is bounded at
  32 entries per application (`DecisionHistoryFull`).

### MENTOR-036 — withdrawal and reapplication

`withdraw_application(applicant, application_id, WithdrawalInput)`:

- Applicant-only (`Unauthorized` for anyone else) and only before the
  program's `withdrawal_cutoff` (`WithdrawalWindowClosed` once reached).
- Requires `acknowledge_consequences = true`; the acknowledgement is part of
  the submitted action, so UX copy can state the consequences before the
  call is signed.
- Releases the program reservation (`active_application_count` decrements),
  appends the withdrawal to the immutable history using
  `DecisionAction::Withdraw`, and blocks all further decisions
  (`InvalidTransition`).
- Preserves the application record and every earlier decision. Nothing is
  deleted.
- Retrying the identical withdrawal returns the original sequence number
  without duplicating effects.

`reapply(applicant, previous_application_id, consent_version)`:

- Only a `Withdrawn` or `Rejected` attempt can be reapplied to, only when
  the *current* policy has `allow_reapplication`, and only once the program's
  `reapply_cooldown` has elapsed (`ReapplicationTooSoon`).
- `max_attempts` (0 = unlimited) bounds the attempt chain
  (`ReapplicationLimitReached`).
- The new application links `previous_application`, gets `attempt + 1`, and
  records its own `consent_version` and `consent_affirmed_at`. The previous
  attempt's consent is never copied: `consent_version = 0` fails with
  `InvalidConsent` and an older version fails with `StaleConsent`.
- Repeating the call returns the already-created application id rather than
  creating a second attempt.

## Data model

| Key | Contents |
|---|---|
| `Admin` | Instance storage; the only address allowed to configure policy and coordinators |
| `Coordinator(Address)` | Decision authority granted by the admin |
| `Policy(program_id)` | `ProgramPolicy` (cutoffs, reapply policy, attempt/active caps) |
| `ApplicationCount` | Monotonic id nonce |
| `Application(application_id)` | `Application` record |
| `ActiveCount(program_id)` | Active (non-final) applications reserving a slot |
| `Decision(application_id, seq)` | Immutable `DecisionRecord` |
| `ReappliedFrom(previous_id)` | Reapplication link used for replay safety |

`not_before`-style window conventions follow the rest of the suite: a cutoff
is exclusive, i.e. the action is closed **at and after** the cutoff.
Persistent entries are bumped to a 100,000/500,000-ledger style TTL range
(`3,110,400` / `6,220,800` ledgers, ~180/~360 days) via `extend_ttl`.

## Events

| Topics | Data |
|---|---|
| `MNT_INIT` | `admin` |
| `MNT_UPG` | `new_wasm_hash` |
| `MNT_COORD` | `(coordinator, allowed)` |
| `MNT_POL` | `(program_id, decision_cutoff, withdrawal_cutoff, allow_reapplication)` |
| `MNT_OPEN` | `(application_id, program_id, applicant, attempt)` |
| `MNT_DEC` | `(application_id, seq, action_code, public_reason, effective_at)` |
| `MNT_WDRAW` | `(application_id, applicant, public_reason, effective_at)` |
| `MNT_REAP` | `(new_application_id, previous_application_id, applicant, attempt)` |

`action_code`: `Review = 1`, `RequestInfo = 2`, `Waitlist = 3`,
`Accept = 4`, `Reject = 5`, `Withdraw = 6`. Status codes:
`Submitted = 1`, `UnderReview = 2`, `InfoRequested = 3`, `Waitlisted = 4`,
`Accepted = 5`, `Rejected = 6`, `Withdrawn = 7`.

## Errors

| Code | Error | Meaning |
|---|---|---|
| 1 | `NotInitialized` | `initialize` has not run |
| 2 | `AlreadyInitialized` | `initialize` already ran |
| 3 | `Unauthorized` | Not admin/coordinator, or not the owning applicant |
| 4 | `PolicyNotFound` | No policy configured for the program |
| 5 | `ApplicationNotFound` | Unknown application or decision |
| 6 | `InvalidConsent` | Consent version is zero |
| 7 | `InvalidReasonCode` | Public reason code is zero |
| 8 | `InvalidEffectiveTime` | `effective_at` is in the past |
| 9 | `DecisionWindowClosed` | `decision_cutoff` reached |
| 10 | `InvalidTransition` | Action not valid from the current status |
| 11 | `DecisionHistoryFull` | 32-entry history bound reached |
| 12 | `WithdrawalWindowClosed` | `withdrawal_cutoff` reached |
| 13 | `WithdrawalNotAcknowledged` | Consequences were not acknowledged |
| 14 | `WithdrawalNotAllowed` | Final decision already exists |
| 15 | `ReapplicationNotAllowed` | Policy/status forbids reapplying |
| 16 | `ReapplicationTooSoon` | Cooldown not elapsed |
| 17 | `ReapplicationLimitReached` | `max_attempts` reached |
| 18 | `StaleConsent` | Reapplication consent older than the previous attempt's |
| 19 | `CapacityExceeded` | Program active-application bound reached |
| 20 | `CounterOverflow` | Monotonic counter exhausted |
| 21 | `InternalReasonRequired` | Waitlist/reject without an internal-reason commitment |
| 22 | `InvalidPolicy` | Inconsistent policy (attempt cap without reapplication) |
| 23 | `WithdrawalIsApplicantAction` | A coordinator tried to record `Withdraw` |

## Authorization

Deny-by-default:

- `admin` (set once in `initialize`) configures policy and coordinators and
  is the only address allowed to call `upgrade`.
- Admin **and** registered coordinators may call `record_decision` and the
  internal `decision` read. Everyone else gets `Unauthorized`.
- Applicants must sign their own `open_application`, `withdraw_application`,
  and `reapply`; no coordinator or relayer can act on their behalf.

## Privacy boundary

On-chain state holds only: application identifiers, addresses, status
values, applicant-visible reason codes, `BytesN<32>` commitments to internal
reasoning, timestamps, and the communication policy. Application answers,
drafts, review notes, and any free text live off-chain and are referenced
only by hash. `applicant_visible_decision` intentionally omits both the
internal-reason commitment and the deciding coordinator.

## Replay protection and atomicity

- Identical decisions/withdrawals are no-ops returning the original `seq`.
- Reapplication retries return the same new application id.
- Soroban invocations are atomic: a failed validation writes nothing, so a
  rejected retry cannot leave partial state.

## Storage bounds

- At most 32 decision entries per application.
- Per-program active applications are capped by
  `ProgramPolicy.max_active_applications`, with a hard contract-wide ceiling
  of 10,000 even when the policy says "unlimited".
- All persistent entries are TTL-bumped on write.

## Dependencies and rollout

- **On-chain:** compose with MENTOR-030/034 (intake and review queues) by
  letting those contracts own submission and queue assignment while this
  contract owns decisions. Program eligibility (MENTOR-018) and blocks
  (MENTOR-044/MENTOR-091) are checked by the matching contract; this
  contract assumes the coordinator only decides on applications that already
  passed intake.
- **Off-chain:** the notifier must honour `CommunicationPolicy`, and the
  admin UI must publish the reason-code catalogue (operator-defined,
  stable integers). Internal notes stay in the off-chain case-management
  system behind the committed hash.
- **Rollout:** new contract, no migration. Deploy, `initialize`, then
  `set_program_policy` per program before opening intake. Policies are read
  at action time, so changing cutoffs affects only future calls; recorded
  decisions never change.
- **Upgrade compatibility:** `upgrade` is admin-gated and only swaps the WASM
  hash; storage keys and error/event schemas in this document are the
  compatibility contract for indexers. Additive changes should append new
  error codes and event types rather than renaming existing ones.
- **Cross-repository work:** MENTOR-035 and MENTOR-036 also exist as
  application/API/UI work items in the other ChainVerse repositories; the
  reason-code catalogue, communication copy, and internal-note schema must
  match the constants and error codes above.

## Non-goals

- Draft autosave, configurable questions, and eligibility preflight
  (MENTOR-031..033).
- Reviewer queue assignment and blind review (MENTOR-034).
- Appeals, rematching, and disputes (MENTOR-042, MENTOR-089/090).
- Sending notifications: the contract records policy; delivery is off-chain.

## Tests

`cargo test -p mentorship-applications` covers bootstrap and role checks,
transition validity, duplicate/replayed decisions and withdrawals,
backdating and window cutoffs, reason-code and internal-reason rules,
applicant-visible vs. coordinator-gated reads, withdrawal consequences and
reservation release, reapplication linking/cooldown/attempt/stale-consent
rules, capacity enforcement, the 32-entry history bound, event topics, and
admin-gated upgrades.
- Status: Implemented
- Owner: Mentorship On-chain working group
- Related issues: #1226 (MENTOR-030 — create mentorship
  applications), #1227 (MENTOR-031 — autosave application drafts), #1228
  (MENTOR-032 — configurable application questions), #1230 (MENTOR-034 —
  application review queues)
- Other repositories: the matching MENTOR-030/031/032/034 work (application
  wizard, draft autosave client, form builder, reviewer console) lives in the
  other ChainVerse repositories. This contract is the on-chain primitive those
  clients share.

## Scope

`contracts/mentorship-applications` implements one privacy-minimized Soroban
contract covering the four "On-chain Mentorship / Applications" work items.
They are deliberately delivered together because they are one workflow:
a program publishes a form, applicants save drafts and submit against that
form, and scoped reviewers work the resulting queue.

### MENTOR-030 — Create mentorship applications

- `submit_application` is separate from every draft entry point and is atomic:
  the program window, the accepted published form version, every visible
  required answer, the applicant's current consent, and the
  `(applicant, program)` uniqueness constraint are validated before the first
  write. A failure therefore leaves no partial application, no allocated id
  and no queue row, so retries cannot duplicate effects.
- The application record stores the accepted `form_version`, the accepted
  `consent_version`, the answer count, and one `data_hash` commitment over the
  off-chain content (goals, expectations, schedule, preferred mode, answers,
  attachments). A successful submission removes any draft for the same pair.
- `has_applied`/`get_application_id` expose the duplicate check cheaply.
- Submission requires affirmative consent (`consent: bool`) plus an
  unrevoked consent record at the program's current terms version. Publishing
  new terms therefore requires re-consent before a further submission.

### MENTOR-031 — Autosave application drafts

- `autosave_draft` creates (`base_revision = 0`) or updates
  (`base_revision = stored revision`) a draft, merging field patches so each
  patched field carries its own `updated_at` ledger timestamp.
- Every accepted save increments a monotonic `revision` and stores the
  client-supplied `request_token`. Replaying a request whose token matches the
  last successful save returns `replayed: true` with the original revision and
  timestamp and changes nothing — safe offline recovery.
- A save whose `base_revision` is stale fails with `DraftConflict` instead of
  overwriting a concurrent edit. `discard_draft` uses the same check.
- A draft is bound to the latest published form version when it is created and
  keeps that version even if a newer form is later published, so a
  mid-window form change cannot silently reinterpret saved fields.
  (`submit_application` may still submit against any published version that
  the applicant accepted; see "Migration".)
- Once an application exists for the pair, autosave fails with
  `DuplicateApplication`, and the draft created before submission is removed
  by the successful submission. A submitted application cannot be mutated as a
  draft.
- `DraftReceipt` is the explicit last-saved feedback: `revision`,
  `saved_at`, `request_token`, `replayed`.

### MENTOR-032 — Configurable application questions

- `publish_form` publishes an immutable, versioned form. Publishing again
  always creates version N+1; an older version is never overwritten.
- Each `Question` has a stable id, a `QuestionType`, a required flag, an
  explicit `order`, label/help-text commitments, an options commitment where
  the type needs one, and a deterministic `VisibilityCondition`.
- Validation at publish time rejects: empty forms, more than 32 questions,
  id 0, duplicate ids, non-increasing `order`, zero commitments, an options
  commitment mismatch for the question type, an expected value without a
  dependency, and any condition that references itself or a later question.
  Because only `BytesN<32>` commitments are accepted, labels, help text and
  options can never carry markup on chain — unsafe markup has no path into
  storage.
- Conditions are evaluated in one forward pass: a question is visible only
  when its dependency is answered and, if `required_answer_hash` is set, the
  dependency's answer commitment equals it. The result depends only on the
  submitted answer commitments.
- Every `Application` records the form version its answers were accepted
  against, and forms are immutable, so historical responses remain renderable
  with the exact question set they were answered on. `validate_answers` exposes
  the same rules to clients without changing state.

### MENTOR-034 — Application review queues

- `grant_reviewer`/`revoke_reviewer` (admin only) give a reviewer a
  program-scoped role and non-zero active-assignment capacity. A revoked
  reviewer keeps their profile but loses every queue permission.
- `set_priority`, `assign_reviewer` and `declare_conflict` are available to the
  admin and to reviewers scoped to that application's program; `record_decision`
  is available only to the current assignee. Calls from anyone else fail with
  `NotAuthorized`/`NotReviewer` **before** any signature is demanded.
- `assign_reviewer` is concurrency-safe via `expected_assignee`
  (`None` while unassigned): a stale expectation fails with
  `AssignmentConflict` and leaves the newer assignment intact. Repeating an
  identical assignment is an idempotent no-op. A reviewer whose active load
  equals their capacity is rejected with `ReviewerAtCapacity`.
- `declare_conflict` records a per-reviewer, per-application conflict.
  A declared conflict blocks that reviewer's decision
  (`ConflictOfInterest`) without silently unassigning them, so coordinators see
  the conflict in the audit trail and can reassign.
- `record_decision` is final and single-shot: the reviewer must be the current
  assignee, still scoped, free of a declared conflict, and the application must
  not already be decided.
- `list_queue` is itself permissioned: only the admin or an active reviewer of
  that program can read it, so applications from other programs can never
  appear. Filters (status, priority, assignee/unassigned, due-before) combine
  with AND, ordering is deterministic (priority descending, then due date
  ascending with undated last, then ascending application id), and the page is
  capped at 64 rows.
- `reviewer_workload` reports capacity, active assigned load, decided count and
  overdue active assignments. Each queue row also carries an `overdue` flag.
- Every state-changing queue action is appended to a bounded audit trail
  (`get_audit_trail`) and emitted as an event. Audit entries are written before
  the state change they describe, so a full trail cannot leave an unaudited
  mutation behind.

## Authorization summary

| Entry point | Who may call it |
|---|---|
| `initialize` | anyone, once |
| `register_program`, `set_program_active` | admin |
| `publish_form`, `publish_consent_terms` | admin (program owner) |
| `grant_reviewer`, `revoke_reviewer` | admin |
| `record_consent`, `revoke_consent` | the applicant, for themselves |
| `autosave_draft`, `discard_draft` | the applicant, for themselves |
| `submit_application` | the applicant, for themselves |
| `set_priority`, `assign_reviewer` | admin or a reviewer scoped to the program |
| `declare_conflict` | a reviewer scoped to the program, for themselves |
| `record_decision` | the current assignee, who must still be scoped and unconflicted |
| `list_queue` | admin or a reviewer scoped to the program |
| read-only getters | anyone (queue reads are permissioned as above) |

## Privacy and the off-chain boundary

On-chain state never contains form labels, help text, options, goals,
expectations, schedule, preferred mode, free-text answers, attachments,
reviewer notes or consent-terms text. All of that lives off-chain and is
represented only by caller-supplied `BytesN<32>` integrity commitments
(`schema_hash`, `label_hash`, `help_hash`, `options_hash`, `answer_hash`,
`data_hash`, `terms_hash`). The chain stores addresses, enum choices, counters,
timestamps and those commitments.

Consequently, these decisions are explicitly off-chain:

- answer content validation (word limits, email/date format, option
  membership) beyond presence, type of question and visibility;
- rendering labels/help text and resolving option commitments;
- storing and retrieving the actual draft payload and attachments;
- reviewer notes and rationale (reviewers record only a decision outcome);
- computing a reviewer's current queue in the UI (the contract exposes a
  permissioned, filtered, bounded page instead of an unbounded enumeration).

## Determinism and storage bounds

| Bound | Value | Enforced by |
|---|---|---|
| questions per form version | 32 | `MAX_QUESTIONS` (`InvalidForm`) |
| answers per draft or submission | 32 | `MAX_ANSWERS` (`TooManyAnswers`) |
| applications per program queue | 256 | `MAX_QUEUE` (`QueueFull`) |
| reviewers per program | 32 | `MAX_REVIEWERS` (`ReviewerPoolFull`) |
| audit entries per application | 64 | `MAX_AUDIT_ENTRIES` (`AuditTrailFull`) |
| rows per `list_queue` page | 64 | `MAX_QUEUE_PAGE` (`InvalidFilter`) |

All counters are `checked_add`-guarded (`VersionOverflow`) and all validation
happens before the first write of a call. Queue ordering is integer-only and
insertion-sorted over a bounded list, so repeated reads with unchanged state
return byte-identical pages.

## Events

| Topic | Payload | Emitted by |
|---|---|---|
| `FORMPUB` | `(program_id, version)` | `publish_form` |
| `CONSPUB` | `(program_id, version)` | `publish_consent_terms` |
| `CONSENT` | `(applicant, program_id, version)` | `record_consent` |
| `DRAFT` | `(applicant, program_id, revision, replayed)` | `autosave_draft` |
| `DRFTDEL` | `(applicant, program_id)` | `discard_draft` |
| `SUBMIT` | `(application_id, applicant, program_id)` | `submit_application` |
| `REVGRANT` | `(program_id, reviewer, capacity)` | `grant_reviewer` |
| `REVOKE` | `(program_id, reviewer)` | `revoke_reviewer` |
| `PRIO` | `(application_id, priority)` | `set_priority` |
| `ASSIGN` | `(application_id, reviewer, due_at)` | `assign_reviewer` |
| `COI` | `(application_id, reviewer, declared)` | `declare_conflict` |
| `DECIDE` | `(application_id, reviewer, outcome)` | `record_decision` |

The `SUBMIT` tuple is the canonical submission event schema and is covered by
a test that decodes the topic symbol and payload types.

## Error codes

Stable, actionable codes are declared in `ContractError` (1–43). The main
groups:

- lifecycle/program: `NotInitialized`, `AlreadyInitialized`, `NotAdmin`,
  `NotAuthorized`, `ProgramNotFound`, `ProgramAlreadyExists`,
  `ProgramInactive`, `DeadlinePassed`, `VersionOverflow`;
- forms: `NoFormPublished`, `FormVersionNotFound`, `InvalidForm`;
- drafts: `DraftNotFound`, `DraftConflict`, `TooManyAnswers`,
  `DuplicateAnswer`, `AnswerNotInForm`, `AnswerNotVisible`,
  `MissingRequiredAnswer`, `InvalidAnswerHash`;
- submissions: `DuplicateApplication`, `ApplicationNotFound`,
  `InvalidDataHash`;
- consent: `NoConsentTermsPublished`, `ConsentTermsNotFound`,
  `ConsentNotFound`, `ConsentRevoked`, `ConsentOutOfDate`,
  `ConsentRequired`, `InvalidConsentTerms`;
- review: `AlreadyReviewer`, `NotReviewer`, `ReviewerPoolFull`,
  `ReviewerAtCapacity`, `AssignmentConflict`,
  `ApplicationAlreadyDecided`, `ConflictOfInterest`,
  `NotAssignedReviewer`, `InvalidDecision`, `QueueFull`,
  `AuditTrailFull`, `InvalidFilter`, `InvalidCapacity`.

## Migration and upgrade compatibility

This is a new contract with no prior on-chain state. `version()` returns the
ABI/storage version (`1`). Upgrade notes for future versions:

- `DataKey` variants and `ContractError` discriminants are part of the storage
  and error ABI; append new variants, never renumber existing ones.
- Soroban structs encode by field name. Adding or removing a field in a stored
  `contracttype` struct (for example `Application` or `Draft`) makes existing
  entries undecodable, so a schema-changing upgrade must ship an explicit
  migration entry point (read old key, write new key) rather than reusing the
  same storage keys blindly.
- Published forms and applications are immutable by design. `CONTRACT_VERSION`
  in `version()` must be bumped for any ABI change so clients can gate on it.
- `DataKey` namespaces are separate (`Application(u64)` vs
  `ApplicationByApplicant(Address, BytesN<32>)`), so a future re-application
  flow can add records without rewriting the uniqueness constraint.

## Deployment and rollout

1. `initialize(admin)`.
2. `register_program(admin, program_id, deadline)`.
3. `publish_form(admin, program_id, schema_hash, questions)` and
   `publish_consent_terms(admin, program_id, terms_hash)`.
4. `grant_reviewer(admin, program_id, reviewer, capacity)` for the review team.
5. Applicants call `record_consent`, `autosave_draft`, then
   `submit_application`; reviewers work `list_queue`.

Recommended rollout order matches MENTOR-120: internal, pilot tenants, limited
availability, general availability. Ship behind the mentorship feature flag in
the client applications; the contract itself has no pause switch in this pass,
so operational rollback is "stop submitting traffic" plus an admin
`set_program_active(false)` per program.

## Operational notes

- All records are `persistent` with a ~1-year TTL bump on write (and on the
  main read paths), matching `course_registry`'s convention. Long-lived queues
  and applications should be periodically touched by an indexer/admin if they
  must outlive that window.
- Publishing new consent terms instantly makes every applicant's consent stale;
  plan terms updates outside intake windows and notify applicants to
  re-consent.
- `MAX_QUEUE = 256` is a hard per-program intake cap. Operators must archive or
  start a new program before it is reached; submission then fails with
  `QueueFull` rather than growing unbounded storage.
- The audit trail is capped at 64 entries per application. A normal workflow
  uses far fewer; a coordinator repeatedly re-prioritising one application can
  exhaust it (`AuditTrailFull`) without changing state.
- `data_hash` is the only commitment over goals/expectations/schedule/preferred
  mode. Clients must include those fields in the hashed payload so the
  submitted application is verifiable.

## Related work in other ChainVerse repositories

| Item | This repository | Other repositories |
|---|---|---|
| MENTOR-030 | contract primitive above | mentee application wizard, consent capture, submission receipt UI |
| MENTOR-031 | revision/token/idempotency primitive | autosave client, offline queue, conflict-resolution UI |
| MENTOR-032 | immutable form-version primitive | form builder, markup sanitisation, renderer for historical versions |
| MENTOR-034 | queue/assignment/COI primitive | reviewer console, workload and SLA dashboards, assignment notifications |
