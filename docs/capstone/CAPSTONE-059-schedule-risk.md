# CAPSTONE-059: Detect Dependency Cycles and Schedule Risks

**Track:** Planning | **Component:** on-chain | **Issue:** #1407 | **Contract:** `contracts/capstone-schedule-risk`

## Scope

Analyze a bounded project plan and reject invalid graphs before they enter the schedule. The contract
detects self-loops, duplicate nodes and edges, unknown node references, and cycles, then derives
deterministic flags and hashes for blocked critical paths, impossible deadlines, resource conflicts,
and deadline risk. Plans are bound to 32 nodes, 128 edges, and a maximum duration of one year.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `PlanAnalysis`: tenant, project, plan id, plan hash, derived counts/flags/hashes, risk value, version, timestamps | Task names, resource names, calendar rules, schedule narrative |
| Role grants and events | Notifications, Gantt rendering, planner identity data |

The plan and all derived evidence are committed as SHA-256 hashes. No prose, URI, memo, or private
reference is published; only counters, enums, bounded values, and hashes are stored.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Planner` and `Participant` roles.
- `submit_plan` requires `require_auth` from the planner and an active role for the exact
  `(tenant_id, project_id)`. A missing role, revoked role, or different tenant returns `Unauthorized`.
- There is no admin override for plan submission, and derived analysis is deterministic so results
  do not depend on the caller.

## Idempotency and versioning

- Resubmitting an identical plan hash and expected version returns the stored analysis with no second
  event; a changed plan or stale version returns `VersionMismatch`.
- Graph validation is deterministic: the same nodes, edges, durations, and deadline always produce
  the same counts, flags, and hashes.
- Cycles and self-loops are rejected at submission with `CycleDetected` / `SelfLoop`; invalid bounds,
  unknown nodes, duplicate ids, and excessive durations return their own stable errors.
- Bounds: `MAX_NODES = 32`, `MAX_EDGES = 128`, `MAX_DURATION_SECONDS = 31_536_000`. TTLs extend on
  read and write.

## Errors

Append-only `ContractError` codes: `AlreadyInitialized=1`, `NotInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `PlanExists=5`, `PlanNotFound=6`, `InvalidBounds=7`, `InvalidDeadline=8`,
`DuplicateNode=9`, `UnknownNode=10`, `SelfLoop=11`, `DuplicateEdge=12`, `CycleDetected=13`,
`InvalidDuration=14`, `VersionMismatch=15`, `LimitReached=16`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `SKR_ROLE` | `(tenant, project, account, role, active)` |
| `SKR_PLAN` | `(tenant, project, plan_id, plan_hash)` |

No task names, resource names, or calendar details appear in events.

## Privacy boundaries

Schedules can expose staffing and capacity information, so the contract stores only hashes and
aggregate flags. Resource conflicts are expressed as counts and bounded hashes, never as resource
identities. Planner addresses are public Stellar accounts and must not be derived from private
identity data.

## Dependencies

- CAPSTONE-058 work breakdown supplies task dependencies, estimates, and assignees.
- CAPSTONE-060 milestones supply due dates and dependency edges for project-level risk analysis.
- CAPSTONE-061 check-ins supply confidence and blocker signals consumed by the off-chain risk model.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the schedule-risk flag on testnet for one cohort, dual-writing analyses to the service
database and the contract. Compare flags and hashes before promoting. Rollback stops new submissions
while stored analyses remain readable; all values are hashes, counters, and enums, so no private
content needs purging and a later deployment resumes from the last stored analysis.

## Other-repository work

- Backend: validate and hash canonical plans, mirror roles, manage request ids, and expose risk
  reconciliation and alerting.
- Frontend: schedule editor, cycle/error display, and risk-flag dashboard using only hashes and
  counters.
- Indexer/ops: monitor `SKR_PLAN`, alert on `CycleDetected`, and renew TTLs for active analyses.
