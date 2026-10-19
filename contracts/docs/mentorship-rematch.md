# Mentorship Rematch Contract

- Status: Implemented (v1)
- Owner: On-chain Mentorship working group
- Related issue: #1238 (MENTOR-042, create rematch workflow)
- Depends on / relates to: the canonical matching contracts
  (MENTOR-037 proposals, MENTOR-038 mutual acceptance — not yet in this
  repository) which will own match creation; agreement records
  (MENTOR-045) referenced by id; safety cases (MENTOR-086/087) whose
  existence is projected as flags; and closure/settlement
  (MENTOR-077/080) which must consume the resulting supersession

## Scope

`contracts/mentorship-rematch` implements the rematch lifecycle:
request -> review -> closure plan -> successor proposal -> continuity.

- **Private reasons are scoped.** `request_rematch` records a public
  blame-free `RematchReason` code plus a `reason_hash` commitment (never
  emitted in events) and a mandatory `closure_plan_hash`. `get_request`
  is deny-by-default: only the match's participants and coordinators may
  read it, and every read requires auth.
- **Prior records remain intact.** `close_engagement` and
  `apply_successor` never delete or rewrite a match: the old record keeps
  every field and only gains `status`, `closed_at` and a
  `successor_match_id`; the new match records `superseded_match_id`, so
  continuity is traceable in both directions. Requests are append-only
  history per match.
- **Open financial or safety cases block unsafe closure.**
  `set_case_flags` records only the *existence* of a case;
  `close_engagement` and `apply_successor` fail with
  `UnsafeClosureBlocked` while either flag is open.
- **Successor approval is mentee-only.** A coordinator proposes a
  successor (must not be a current participant); only the mentee accepts
  it, and acceptance after the proposal window fails with
  `ProposalExpired`. Proposals carry a `proposal_hash` commitment.
- **Rematch metrics avoid blame scoring.** `stats()` exposes aggregate
  counters only — no address, match id, or per-person score appears in
  `RematchStats` at all.
- **Bounded and replay-safe.** One open request per match, at most
  `MAX_REQUESTS_PER_MATCH` (8) requests per match, a coordinator set
  capped at `MAX_COORDINATORS` (16), and idempotent `request_rematch`
  retries (same request id + payload returns the existing request; a
  different payload fails with `IdempotencyConflict`).

## Privacy boundary

On-chain: match/agreement ids, participant addresses, status, reason
*code*, commitments, case existence flags, timestamps and aggregate
counters. Off-chain: the private explanation behind `reason_hash`, the
closure/handover plan text behind `closure_plan_hash`, case contents
(safety team, MENTOR-086/087), and any coordinator notes. The
`RMREQ`/`RMSUCC` events deliberately omit both commitments.

## Storage layout (instance vs persistent)

- Instance: `Admin`, `Registrar`, `Coordinators`, `ProposalWindow`,
  `Stats`.
- Persistent (TTL-bounded): `Match(match_id) -> MatchRecord`,
  `Request(request_id) -> RematchRequest`,
  `MatchRequests(match_id) -> Vec<BytesN<32>>` (<= 8),
  `CaseFlags(match_id) -> CaseFlags`.

## Ownership and authorization

Deny-by-default:

- Admin (`initialize`): `register_coordinator`,
  `revoke_coordinator`, `set_registrar`, `set_proposal_window`,
  `upgrade`.
- Coordinator (admin is the first): `start_review`, `reject_request`,
  `set_case_flags`, `propose_successor`, `close_engagement`,
  `apply_successor`.
- Participants: `request_rematch` (mentor or mentee), scoped
  `get_request`; `approve_successor` is the mentee only.
- Registrar (defaults to the admin): `register_match` — moves to the
  canonical matching contract when it ships.

## Events and error codes

- `RMREQ (request_id)` -> `(match_id, reason)` — public code only
- `RMRVW (request_id)` -> `(coordinator)`
- `RMREJ (request_id)` -> `(coordinator)`
- `RMFLAG` -> `(match_id, financial_open, safety_open)`
- `RMSUCC (request_id)` -> `(successor, expires_at)`
- `RMAPPR (request_id)` -> `(mentee, timestamp)`
- `RMCLSE (request_id)` -> `(match_id, timestamp)`
- `RMSUPER (request_id)` -> `(previous_match_id, successor_match_id)`
- `MREG (match_id)`, `SCOORD`/`RCOORD (address)`, `PWINDOW (seconds)`,
  `REGISTRAR (address)`, `UPGRADED (wasm_hash)`

Errors: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAdmin=3`,
`NotCoordinator=4`, `NotRegistrar=5`, `NotAuthorized=6`,
`MatchNotFound=7`, `MatchAlreadyRegistered=8`, `MatchNotActive=9`,
`RematchRequestNotFound=10`, `RematchAlreadyOpen=11`,
`RequestLimitReached=12`, `InvalidRequestState=13`,
`InvalidSuccessor=14`, `ProposalExpired=15`,
`UnsafeClosureBlocked=16`, `ClosurePlanMissing=17`, `InvalidConfig=18`,
`RequestAlreadyExists=19`, `IdempotencyConflict=20`,
`CoordinatorLimitReached=21`, `CoordinatorNotFound=22`.

## Upgrade compatibility

`upgrade` is admin-gated and swaps the WASM under the same contract
address; `version()` reports ABI generation 1. Additive increments keep
`version()` and never change existing entry points or event schemas;
`MatchRecord`/`RematchRequest` may only gain fields in a coordinated
client release, since they are returned whole.

## Dependencies and rollout

Deploy after (or alongside) the match projection. Set the
successor-proposal window with `set_proposal_window` (60s–30 days)
before enabling the workflow; the default is deliberately short. Off-chain,
the coordinator console consumes `RMREQ`/`RMRVW`, the notification
service consumes `RMSUCC`/`RMAPPR` (MENTOR-102/103), and settlement
(MENTOR-077/080) must read the supersession before releasing funds. The
matching MENTOR-042 work (request UI, coordinator review, successor
matching, closure checklist, metrics dashboard) lives in the other
ChainVerse repositories.
