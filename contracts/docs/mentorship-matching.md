# Mentorship Matching Contract

- Status: Implemented (proposals and coordinator-assisted matching)
- Owner: Mentorship on-chain working group
- Contract: `contracts/mentorship-matching`
- Related issues: #1233 (**MENTOR-037**, mentor match proposals), #1235
  (**MENTOR-039**, coordinator-assisted matching)
- Depends on (off-chain or later on-chain work): MENTOR-008/009 (mentor and
  mentee profiles), MENTOR-018 (program eligibility), MENTOR-038 (mutual
  match acceptance product flow), MENTOR-040 (conflict-of-interest
  detection), MENTOR-041 (mentor capacity), MENTOR-042 (rematch), MENTOR-044
  (blocked-user matching), MENTOR-091 (participant blocking), MENTOR-111
  (audit log)

## Scope

This contract owns the *matching* half of the mentorship workflow: proposal
creation, participant consent, hard eligibility, blocks, and coordinator
overrides with reporting. It stores commitments, never narrative content.

### MENTOR-037 — create mentor match proposals

`create_proposal(mentor, program_id, mentee, ProposalTerms)`:

- A mentor signs for themselves and proposes a match with a mentee.
- Terms carry only what settlement needs on-chain: `terms_hash` (a
  `BytesN<32>` commitment to compatibility reasons, goals, and the
  off-chain agreement), `proposed_fee` (`>= 0`), and `expires_at` (future,
  at most 90 days out).
- The proposal records the initiator (`created_by`), origin (`Mentor`),
  creation time, version `1`, and both consent bits (`false` initially).
- **Duplicate active proposals are prevented** per
  `(program_id, mentor, mentee)`: a second open proposal for the same triple
  fails with `DuplicateActiveProposal`. A declined, expired, or different
  program's proposal does not block a new one.
- **Both parties see permitted details:** the on-chain `Proposal` contains
  no side-specific secret; both participants read the same record through
  `get_proposal`. Narrative details are shared off-chain, referenced by the
  terms hash.
- **Expired proposals cannot be approved** (`ProposalExpired`); expiry is
  also treated as closed by `is_proposal_open` and `open_proposal_for`.
- **Creation is auditable:** every creation emits `MNT_PROP`; coordinator
  creations additionally append to the coordinator action log.

### MENTOR-039 — coordinator-assisted matching

`propose_as_coordinator(coordinator, program_id, mentor, mentee, terms,
reason_hash)` and
`adjust_proposal(coordinator, proposal_id, terms, reason_hash)`:

- Only the admin or a registered coordinator can call them
  (`Unauthorized`); both require a non-zero `reason_hash`
  (`ReasonRequired`).
- **Coordinators cannot bypass hard eligibility or blocks.** Both creation
  and adjustment re-run `require_eligible`: participants must be registered,
  in the correct role, `Active`, and not blocked in either direction. Blocks
  are symmetric, so a block added by either side disqualifies the pair.
- **Participants approve changes.** Adjusting a proposal bumps its
  `version`, resets both approval bits, and returns it to `Proposed`.
  `approve_proposal` takes `expected_version`, so an approval for a version
  the participant never saw fails with `StaleProposalVersion`. Adjusting an
  already-`Active` match reopens consent and re-consumes a pending slot.
- **Actions identify the coordinator.** `last_adjusted_by` records the
  adjusting coordinator on the proposal, `MNT_ADJ` includes the coordinator,
  and the reason commitment is stored under the proposal.
- **Overrides are reportable.** Every coordinator creation/adjustment
  appends an immutable `CoordinatorAction` (kind, proposal, reason hash,
  version, ledger time) under a per-coordinator sequence, readable through
  `coordinator_action_count` and `coordinator_action`.

### Mutual approval primitive

`approve_proposal(participant, proposal_id, expected_version)` records each
participant's consent; when both are present the proposal becomes `Active`
and emits `MNT_MATCH`. `decline_proposal(participant, proposal_id,
reason_code)` closes a pending proposal and releases its pending slot.

Full mutual-acceptance product behaviour (notifications, cooling-off,
match confirmation, rematch flow, feedback loop) belongs to MENTOR-038 and
the rematch/feedback issues; this contract only exposes the consent bits
those flows consume.

## Data model

| Key | Contents |
|---|---|
| `Admin` | Instance storage; configured once in `initialize` |
| `Coordinator(Address)` | Coordinator authority granted by the admin |
| `Participant(Address)` | `Participant` (role + hard eligibility status) |
| `Blocked(Address, Address)` | Directional block marker, enforced symmetrically |
| `ProposalCount` | Monotonic id nonce |
| `Proposal(proposal_id)` | `Proposal` record |
| `PairIndex(program_id, mentor, mentee)` | Open proposal for the triple |
| `PendingCount(program_id)` | Proposals awaiting approval (storage bound) |
| `ProposalReason(proposal_id)` | Last coordinator reason commitment |
| `CoordActionCount(Address)` | Per-coordinator action-log length |
| `CoordAction(Address, seq)` | Immutable `CoordinatorAction` |

Persistent entries are bumped to the suite's `3,110,400` / `6,220,800`
ledger TTL range (`extend_ttl`, ~180/~360 days).

## Events

| Topics | Data |
|---|---|
| `MNT_INIT` | `admin` |
| `MNT_UPG` | `new_wasm_hash` |
| `MNT_COORD` | `(coordinator, allowed)` |
| `MNT_REG` | `(participant, role_code)` |
| `MNT_STAT` | `(participant, status_code)` |
| `MNT_BLOCK` | `(participant, counterparty, blocked)` |
| `MNT_PROP` | `(proposal_id, program_id, mentor, mentee, origin_code, version)` |
| `MNT_APPR` | `(proposal_id, participant, version)` |
| `MNT_MATCH` | `(proposal_id, program_id, mentor, mentee)` |
| `MNT_DECL` | `(proposal_id, participant, reason_code)` |
| `MNT_ADJ` | `(proposal_id, coordinator, version)` |
| `MNT_CACT` | `(coordinator, seq, kind_code, proposal_id)` |

`origin_code`: `Mentor = 0`, `Coordinator = 1`.
`kind_code`: `Proposed = 0`, `Adjusted = 1`.
`role_code`: `Mentor = 0`, `Mentee = 1`.
`status_code`: `Active = 0`, `Inactive = 1`, `Suspended = 2`.

## Errors

| Code | Error | Meaning |
|---|---|---|
| 1 | `NotInitialized` | `initialize` has not run |
| 2 | `AlreadyInitialized` | `initialize` already ran |
| 3 | `Unauthorized` | Not admin/coordinator |
| 4 | `ParticipantNotFound` | Address is not registered |
| 5 | `ParticipantInactive` | Hard eligibility is not `Active` |
| 6 | `RoleMismatch` | Mentor/mentee roles are reversed |
| 7 | `SelfMatch` | An address was matched/blocked against itself |
| 8 | `Blocked` | Either side blocked the other |
| 9 | `InvalidTerms` | Empty terms hash or negative fee |
| 10 | `InvalidExpiry` | Expiry is past or beyond 90 days |
| 11 | `ProposalNotFound` | Unknown proposal |
| 12 | `DuplicateActiveProposal` | Open proposal exists for the triple |
| 13 | `ProposalExpired` | Expiry passed |
| 14 | `InvalidTransition` | Action not valid from the current status |
| 15 | `NotParticipant` | Caller is not a party to the proposal |
| 16 | `AlreadyApproved` | This participant already approved |
| 17 | `StaleProposalVersion` | Approval targets an old version |
| 18 | `ReasonRequired` | Coordinator action without a reason commitment |
| 19 | `InvalidReasonCode` | Decline reason code is zero |
| 20 | `ProgramCapacityReached` | 128 pending proposals for the program |
| 21 | `ActionLogFull` | 1,024-action coordinator log bound reached |
| 22 | `CounterOverflow` | Monotonic counter/version exhausted |
| 23 | `ParticipantExists` | Address is already registered |
| 24 | `InvalidProgram` | Program id is all zeroes |
| 25 | `ReasonNotFound` | No coordinator reason recorded |
| 26 | `ActionNotFound` | Unknown coordinator action sequence |

## Authorization

- `admin` registers participants, sets eligibility, manages coordinators,
  and is the only address allowed to `upgrade`.
- Admin and registered coordinators may create coordinator proposals and
  adjust proposals.
- Mentors may create proposals for themselves only; participants may only
  approve/decline proposals they are party to; blocking is self-service for
  the blocking participant.

## Privacy boundary

The contract stores addresses, roles, status, terms hashes, fees, expiries,
consent bits, coordinator reason hashes, and version/timestamp metadata.
Compatibility assessments, mentor/mentee narratives, coordinator notes,
session plans, and reputation data stay off-chain; only commitments are
published. Coordinator reason hashes are readable as hashes, not content.

## Replay protection and atomicity

- Repeating a decline is a no-op; repeated approvals fail with
  `AlreadyApproved` instead of double-counting consent.
- Approvals are version-scoped, so a replayed approval after an adjustment
  fails deterministically rather than silently re-consenting.
- A failing call reverts entirely, including any lazy expiry finalization.
  Expired proposals are finalized by the next successful creation for the
  same triple; until then they are already unusable (`ProposalExpired`,
  `is_proposal_open == false`).

## Storage bounds

- 128 pending proposals per program.
- 90-day maximum proposal lifetime.
- 1,024 coordinator actions per coordinator (export/reset off-chain after
  that; the log is a reporting aid, not the only audit source).
- 32-bit monotonic ids/versions with checked arithmetic.

## Dependencies and rollout

- **On-chain:** profile and eligibility contracts (MENTOR-008/009/018) can
  drive `register_participant`/`set_participant_status`. Capacity
  (MENTOR-041) and conflict-of-interest checks (MENTOR-040) should veto
  proposals before or alongside this contract; blocks implemented here are
  the local hard stop for MENTOR-044/091 until a shared block registry
  exists. Settlement consumes the `Active` match and its terms hash
  (MENTOR-045 onward).
- **Off-chain:** the matching service supplies compatibility scores and
  reason text behind the hashes, serves participant-facing proposal
  details, and reads the coordinator action log for reporting dashboards
  (MENTOR-093/116).
- **Rollout:** new contract, no migration. Deploy, `initialize`, register
  participants (or sync from the profile service), then optionally grant
  coordinators. The pending cap and proposal lifetime are contract
  constants in this pass; if program-specific limits are needed, add a
  policy key rather than changing semantics silently.
- **Upgrade compatibility:** `upgrade` is admin-gated and preserves the
  contract address. Error codes, event topics, and the `Proposal` field set
  documented here are the indexer-facing contract; add fields/errors
  append-only.
- **Cross-repository work:** MENTOR-037 and MENTOR-039 also exist as
  application/API/UI work items in the other ChainVerse repositories. The
  reason-code catalogue, coordinator permission model, override-reporting
  fields, and participant consent copy must match this document.

## Non-goals

- The full mutual-acceptance product flow and rematch behaviour
  (MENTOR-038/MENTOR-042).
- Mentor capacity allocation (MENTOR-041) and automated conflict detection
  (MENTOR-040) — `require_eligible` is the integration point.
- Ranking, search, shortlists, and comparison (MENTOR-023..026).
- Reputation, reviews, and dispute resolution.

## Tests

`cargo test -p mentorship-matching` covers bootstrap and role checks,
participant registration/eligibility, role and self-match validation, terms
and expiry validation, duplicate-active prevention and replacement after
decline/expiry, symmetric blocks before and after proposal, one-time and
version-scoped approvals, expiry rejection, decline terminality and
idempotency, coordinator authority/reason requirements, eligibility bypass
attempts, consent reset and stale-version rejection after adjustment,
adjustment of declined/expired proposals, pending-cap enforcement, read
errors, coordinator action reporting, event topics, and admin-gated
upgrades.
