# CAPSTONE-058: Build Work Breakdown Structures

**Track:** Planning | **Component:** on-chain | **Issue:** #1406 | **Contract:** `contracts/capstone-work-breakdown`

## Scope

Create ordered work packages and tasks with dependencies, estimates, assignees, acceptance hashes,
and progress weights. Tasks move through `Planned -> InProgress/Blocked -> Done/Cancelled`. Weights
are basis points summing to 10,000 per package, and each package is bounded to 64 tasks with at most
16 dependencies per task.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Package`: tenant, project, package id, planned weight vector, task count, version, timestamps | Package narrative, WBS diagram, assumptions |
| `Task`: order, spec/acceptance hashes, dependency ids, estimate, assignee, weight, state, version | Task descriptions, acceptance checklist documents, attachments |
| Role grants and events | Notifications, UI labels, reviewer identity data |

Spec and acceptance fields are SHA-256 commitments over canonical off-chain documents. No prose,
URI, memo, or private reference is written to the ledger.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Planner` and `Participant` roles.
- Package and task creation and state changes require the caller's `require_auth` and an active role
  for the exact `(tenant_id, project_id)`; a missing or mismatched role returns `Unauthorized`.
- Participants may advance assigned tasks; planners may manage structure. There is no admin override
  for structural mutations.
- Deny is the default for every read that requires a stored record.

## Idempotency and versioning

- Replaying a package or task creation with an identical payload returns the stored record and emits
  no second event; a different payload or stale version returns `VersionMismatch`.
- Dependency ids must exist in the same project and cannot be self-referential; duplicates, order
  collisions, and limit overruns return stable errors.
- State transitions check the expected version; a retry with the same version is a no-op, and
  transitions from `Done` or `Cancelled` return `TerminalState`.
- Bounds: `MAX_TASKS = 64`, `MAX_DEPENDENCIES = 16`, weights sum to `TOTAL_WEIGHT_BPS = 10_000`.
  Persistent keys extend TTL on read and write.

## Errors

Append-only `ContractError` codes: `AlreadyInitialized=1`, `NotInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `PackageExists=5`, `PackageNotFound=6`, `TaskExists=7`, `TaskNotFound=8`,
`InvalidOrder=9`, `InvalidWeight=10`, `InvalidEstimate=11`, `InvalidDependency=12`,
`DuplicateOrder=13`, `DependencyLimitReached=14`, `TaskLimitReached=15`, `InvalidState=16`,
`TerminalState=17`, `VersionMismatch=18`, `LimitReached=19`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `WBK_ROLE` | `(tenant, project, account, role, active)` |
| `WBK_PKG` | `(tenant, project, package_id, timestamp)` |
| `WBK_TASK` | `(tenant, project, package_id, task_id, order)` |
| `WBK_STATE` | `(tenant, project, task_id, state, version)` |

Payloads contain only IDs, enums, versions, and bounded values.

## Privacy boundaries

Task content and acceptance criteria are private and stay off-chain; only their hashes are stored.
Assignee addresses are public Stellar accounts and must not be derived from private identity data.
Events never include descriptions, documents, or references.

## Dependencies

- CAPSTONE-057 objectives supply measurable criteria that progress weights roll up into.
- CAPSTONE-059 schedule risk consumes task dependencies, estimates, and deadlines.
- CAPSTONE-060 milestones consume package/task completion state and acceptance hashes.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the planning-WBS flag on testnet for one cohort, dual-writing packages and tasks to
the service database and the contract. Promote after reconciling weight sums, orders, and versions.
Rollback stops new WBS writes; stored records are append-only and hash-only, so no private content
needs purging and a later deployment resumes from the last stored version.

## Other-repository work

- Backend: canonicalize and hash task specifications, mirror roles, manage ids and versions, and
  expose reconciliation plus progress aggregation.
- Frontend: WBS editor, dependency view, weight validation, and task state controls.
- Indexer/ops: monitor `WBK_TASK`/`WBK_STATE`, alert on `VersionMismatch`, and renew TTLs for active
  packages and tasks.
