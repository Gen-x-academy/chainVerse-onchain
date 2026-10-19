# MENTOR-042: Create Rematch Workflow

**Track:** Matching | **Component:** on-chain | **Status:** implemented (v1)

Goal: Create a rematch workflow for incompatibility or availability
changes.

## On-chain

`contracts/mentorship-rematch` implements request -> review -> closure
plan -> successor proposal -> continuity:

- A participant opens a rematch request on an active match with a public
  blame-free reason code, a hash of the private explanation, and a
  mandatory closure/handover plan hash.
- A coordinator reviews, can open financial/safety case flags, can reject,
  and can propose a successor mentor with a commitment hash.
- Only the mentee accepts the successor; proposal approval expires.
- A coordinator closes the engagement (no successor) or applies the
  successor, which creates a new active match and marks the old one
  superseded with pointers in both directions.
- Match records and request history are never deleted or rewritten.

## Off-chain boundary

Private explanations, closure/handover plan text, case details (safety
team, MENTOR-086/087) and coordinator notes stay off-chain. The chain
stores reason codes, commitments, statuses, case existence flags,
pointers and aggregate counters. Metrics are aggregate-only so they cannot
become a blame score.

## Rules

- Only match participants can request a rematch; only coordinators can
  review, flag cases, propose successors, close or apply.
- One open request per match; at most 8 requests per match; at most 16
  coordinators.
- Open financial or safety cases block closure and supersession.
- Successor must be a new person (not the current mentor or mentee) and
  must be accepted by the mentee before the proposal window closes.
- Retrying the same request id and payload is a no-op; reusing it for a
  different payload fails.
- Events `RMREQ`, `RMRVW`, `RMREJ`, `RMFLAG`, `RMSUCC`, `RMAPPR`,
  `RMCLSE`, `RMSUPER`; the private commitments are never emitted.

## Events and errors

Errors: `NotAuthorized`, `NotCoordinator`, `MatchNotFound`,
`MatchAlreadyRegistered`, `MatchNotActive`, `RematchRequestNotFound`,
`RematchAlreadyOpen`, `RequestLimitReached`, `InvalidRequestState`,
`InvalidSuccessor`, `ProposalExpired`, `UnsafeClosureBlocked`,
`ClosurePlanMissing`, `InvalidConfig`, `IdempotencyConflict`,
`CoordinatorLimitReached`, `CoordinatorNotFound`.

## Dependencies and rollout

Match creation will be owned by MENTOR-037/038; until then the registrar
anchors matches. Settlement (MENTOR-077/080) must read the supersession
before releasing funds, and safety tooling (MENTOR-086/087) feeds the case
flags. Configure the proposal window and coordinator set before enabling
the coordinator console. The matching MENTOR-042 work (request UI,
coordinator review, successor matching, closure checklist, aggregate
metrics) lives in the other ChainVerse repositories.
