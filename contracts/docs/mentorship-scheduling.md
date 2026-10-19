# Mentorship Scheduling Contract

- Status: Implemented (v1 — session booking; v1.1 — reschedule)
- Owner: On-chain Mentorship working group
- Related issues: #1252 (MENTOR-056, book mentorship sessions);
  #1253 (MENTOR-057, reschedule sessions)
- Depends on / relates to: the canonical agreements contracts
  (MENTOR-045/046/047, not yet in this repository), whose registrar
  address is intended to own the agreement anchor, and the matching
  contracts (MENTOR-037/038) whose engagements become agreements

## Scope

This contract (`contracts/mentorship-scheduling`) implements the
Scheduling track's "book mentorship sessions" work item (MENTOR-056):

- **Book from an active agreement.** Only the mentor and mentee recorded
  on a registered, active agreement anchor may book sessions under it
  (`AgreementNotActive`, `NotAuthorized`).
- **No double-booking.** `book_session` rechecks the proposed slot for
  overlap against every existing session of *both* participants before
  writing (`ScheduleConflict`). Back-to-back sessions (end == next start)
  are allowed. The check is per-invocation, so two concurrent booking
  transactions cannot both succeed for the same slot: the second sees the
  first one's stored session.
- **Permissions and lead time enforced.** The caller must be a
  participant (`require_auth`), the start must be in the future
  (`StartTimeInPast`) and at least `min_lead_time` away
  (`LeadTimeNotMet`). Duration must be 15–480 minutes
  (`InvalidDuration`), and the display offset must be within
  UTC-12..UTC+14 (`InvalidTimezoneOffset`).
- **Confirmations use local time plus UTC.** A session stores the
  canonical UTC start plus a bounded display offset; `session_times`
  renders both. Both participants confirm before the session becomes
  `Confirmed`.
- **One canonical event.** A successful booking publishes exactly one
  `BOOKED` event carrying `(agreement_id, mentor, mentee, start_utc,
  duration_minutes)` in its data and `(BOOKED, session_id)` in its
  topics. Confirmations publish one `CFMD` event each.
- **Safe retries.** Booking takes an idempotency key scoped to
  `(agreement_id, caller, idempotency_key)`. A retry with the same
  payload returns the original session id with no state or event
  duplication; reusing the key for a different payload fails with
  `IdempotencyConflict`.
- **Bounded storage.** Each participant's session index is capped at
  `MAX_SESSIONS_PER_PARTICIPANT` (64); the 65th booking fails with
  `SessionLimitReached` rather than growing without bound. All
  persistent entries carry TTL bumps (~1 year window).

### Reschedule (MENTOR-057, #1253)

- **Propose or apply time changes under notice policies.** Either
  participant proposes a new slot for a session. The original start must
  still be at least `reschedule_notice_seconds` away (default 24h,
  admin-tunable); otherwise `RescheduleNoticeNotMet`.
- **Participant approval.** The proposer records an implicit approval;
  only the *counterparty* can approve (`approve_reschedule`) or decline
  (`decline_reschedule`). Self-approval fails with `NotAuthorized`.
- **Reason visibility.** The public `reason` is a blame-free code
  (`AvailabilityChange`, `ScheduleConflict`, `TimezoneChange`,
  `Personal`, `Other`); the free-text explanation stays off-chain behind
  `reason_hash`, which is stored but never emitted.
- **Conflict recheck.** The new slot is checked against both
  participants at proposal *and* at approval, so a booking created in
  between cannot cause a double-booking (`ScheduleConflict`).
- **Original time remains traceable.** Applying appends a
  `ScheduleChange` (previous start/duration/offset, new values, proposal
  id, timestamp) to the session, capped at
  `MAX_RESCHEDULES_PER_SESSION` (16). `revision` increments on every
  change.
- **Stale approvals fail.** A proposal expires after the approval window
  (default 72h, capped at the original start); approval afterwards fails
  with `RescheduleExpired`, and approval against a changed session fails
  with `StaleRescheduleApproval`.
- **Payment consequences are previewed.** Each proposal stores a
  `PaymentImpact` (current/proposed billable minutes, signed delta, and
  whether settlement must be adjusted); `reschedule_payment_impact`
  gives the same preview before a proposal exists. This contract never
  moves funds.
- **Repeated requests are safe.** Same proposal id + payload returns the
  existing proposal; a different payload fails with
  `IdempotencyConflict`; only one proposal per session may be open
  (`RescheduleAlreadyPending`). Approving an already applied proposal is
  a no-op returning the current session.
- **Calendar updates converge.** Applying mutates the session in place
  (same id and participant indexes), resets both confirmations and sets
  the status back to `Booked`, so every calendar derived from
  `get_session`/`session_times` converges on the new slot.

**Not implemented here:** cancellation (MENTOR-058), no-shows
(MENTOR-059), completion, payments and escrow (MENTOR-077/078/080), and
settlement adjustment execution (the reschedule preview only tells the
settlement layer what changed).

## Privacy boundary

On-chain storage never holds meeting links, venues, or agenda text. A
session carries a `location_ref: BytesN<32>` commitment/reference to the
off-chain meeting access or venue, and an `agenda_hash: BytesN<32>`
commitment over the off-chain agenda. The readable facts are addresses,
times, duration, mode, status and commitments. The public
`has_conflict(participant, start, duration)` view reveals only whether a
slot conflicts — never who the other participant is or what the session
is about.

## Storage layout (instance vs persistent)

- Instance: `Admin`, `Registrar`, `MinLeadTime`, `RescheduleNotice`,
  `ApprovalWindow`.
- Persistent (TTL-bounded): `Agreement(agreement_id) ->
  AgreementAnchor`, `Session(session_id) -> Session` (with bounded
  `reschedule_history` and `revision`),
  `ParticipantSessions(participant) -> Vec<BytesN<32>>` (<= 64),
  `BookingIdem(agreement_id, caller, idempotency_key) -> session_id`,
  `Reschedule(proposal_id) -> RescheduleProposal`,
  `OpenReschedule(session_id) -> proposal_id`.

## Ownership and authorization

Deny-by-default throughout:

- Admin (`initialize`): `set_registrar`, `set_min_lead_time`,
  `set_reschedule_policy`, `upgrade`.
- Registrar (defaults to the admin; movable via `set_registrar`):
  `register_agreement`, `set_agreement_active`. When the canonical
  agreements contract ships, its address takes the registrar role and
  this anchor becomes a projection of that contract.
- Participants: `book_session` (mentor or mentee of the agreement),
  `confirm_session` (mentor or mentee of the session),
  `propose_reschedule` (either participant),
  `approve_reschedule`/`decline_reschedule` (the non-proposing
  participant only).

## Events and error codes

Topics are short symbols; data never carries free text.

- `BOOKED (session_id)` -> `(agreement_id, mentor, mentee, start_utc, duration_minutes)`
- `CFMD (session_id)` -> `(caller, timestamp, both_confirmed)`
- `RSPROP (proposal_id)` -> `(session_id, reason, new_start_utc)` — the
  private `reason_hash` is deliberately not emitted
- `RESCHED (proposal_id)` -> `(session_id, previous_start_utc,
  new_start_utc, new_duration_minutes, approver)`
- `RSDECL (proposal_id)` -> `(session_id, decliner, timestamp)`
- `RESPPOL` -> `(notice_seconds, approval_window_seconds)`
- `AGREG (agreement_id)`, `AGACT (agreement_id, active)`,
  `REGISTRAR (registrar)`, `LEADTIME (seconds)`,
  `UPGRADED (wasm_hash)`

Errors: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAdmin=3`,
`NotRegistrar=4`, `NotAuthorized=5`, `AgreementNotFound=6`,
`AgreementAlreadyRegistered=7`, `AgreementNotActive=8`,
`SessionAlreadyExists=9`, `SessionNotFound=10`, `ScheduleConflict=11`,
`LeadTimeNotMet=12`, `StartTimeInPast=13`, `InvalidDuration=14`,
`InvalidTimezoneOffset=15`, `SessionLimitReached=16`,
`IdempotencyConflict=17`, `InvalidLeadTime=18`,
`RescheduleNotFound=19`, `RescheduleAlreadyPending=20`,
`RescheduleNoticeNotMet=21`, `RescheduleExpired=22`,
`StaleRescheduleApproval=23`, `InvalidReschedule=24`,
`RescheduleLimitReached=25`, `RescheduleNotOpen=26`,
`InvalidReschedulePolicy=27`.

## Upgrade compatibility

`upgrade` is admin-gated and swaps the WASM under the same contract
address; `version()` reports ABI generation 1. Reschedule work
(MENTOR-057) is an additive increment: existing entry points, storage
keys, and the `BOOKED`/`CFMD` event schemas stay unchanged, so clients
built against v1 keep working. The `Session` struct gained `revision`
and `reschedule_history`; because no instance was live before this pass,
there is no stored-state migration — but any future field addition must
follow the same additive rule, since `get_session` returns the struct
whole.

## Dependencies and rollout

Ships behind a feature flag. Deploy order: this contract first (it owns
session state), then the reschedule increment (MENTOR-057) as a WASM
upgrade under the same address. Set the notice and approval windows with
`set_reschedule_policy` before enabling reschedule in the UI; the
defaults (24h notice, 72h approval window) are deliberately
conservative. When the agreements contract (MENTOR-045/046) is deployed,
call `set_registrar` with its address and stop hand-registering anchors.

Off-chain, the indexer should project `RESCHED` into calendar updates
and notifications (MENTOR-102/103), and the settlement service
(MENTOR-070/071) should consume the proposal's `PaymentImpact` when it
is non-zero. The matching MENTOR-056/057 work (booking and reschedule
UI, calendar projection, notification delivery, payment adjustment)
lives in the other ChainVerse repositories.
