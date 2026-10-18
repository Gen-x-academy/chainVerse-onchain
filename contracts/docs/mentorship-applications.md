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
