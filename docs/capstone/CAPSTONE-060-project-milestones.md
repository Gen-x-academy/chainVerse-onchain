# CAPSTONE-060: Create Project Milestones

**Track:** Planning | **Component:** on-chain | **Issue:** #1408 | **Contract:** `contracts/capstone-project-milestones`

## Scope

Define ordered milestones with due dates, intra-project dependencies, reviewers, deliverables,
completion rules, and optional funding linkage. Milestones move from `Pending` to `Completed` only
when dependencies are complete and the configured completion rule is satisfied. Completion rules are
`AllDeliverables`, `Reviewer`, or `ExternalEvidence`; a milestone is bound to 64 per project with at
most 16 dependencies, deliverables, and evidence hashes each.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Milestone`: tenant, project, milestone id, order, due date, reviewer, dependency ids, completion rule, funding reference hash, state, version | Milestone narrative, deliverable documents, reviewer identity data |
| Deliverable/evidence hash lists | Attachments, review notes, external evidence payloads |
| Role grants and events | Notifications and UI rendering |

Deliverables, evidence, and funding references are SHA-256 commitments. No prose, URI, memo, or
private reference is written to the ledger.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Planner` and `Participant` roles.
- Milestone creation requires `require_auth` and an active planner role for the exact tenant/project.
- Completion checks the configured rule: reviewer approval must come from the stored reviewer address,
  and evidence must be supplied by an authorized participant. A caller without the required role or
  identity gets `Unauthorized` / `WrongReviewer`.
- Deny is the default; there is no admin bypass for completion.

## Idempotency and versioning

- Replaying a creation with the same payload and expected version returns the stored milestone with
  no second event; a stale or different payload returns `VersionMismatch`.
- Duplicate orders, duplicate dependencies, missing dependencies, cross-project dependencies, and
  cycles are rejected with stable errors before storage.
- Completion is idempotent: replaying it with the same evidence and version returns the stored
  record; completing twice returns `AlreadyCompleted`, and missing prerequisites return
  `IncompleteDependency` or `MissingEvidence`.
- Bounds: `MAX_MILESTONES = 64`, `MAX_DEPENDENCIES = 16`, `MAX_DELIVERABLES = 16`,
  `MAX_EVIDENCE = 16`. Due dates and funding references are validated at creation.

## Errors

Append-only `ContractError` codes: `AlreadyInitialized=1`, `NotInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `MilestoneExists=5`, `MilestoneNotFound=6`, `DependencyNotFound=7`,
`CrossProjectDependency=8`, `IncompleteDependency=9`, `CycleDetected=10`, `InvalidOrder=11`,
`InvalidDependency=12`, `DependencyLimitReached=13`, `DeliverableLimitReached=14`,
`EvidenceLimitReached=15`, `DuplicateOrder=16`, `WrongReviewer=17`, `AlreadyCompleted=18`,
`MissingEvidence=19`, `VersionMismatch=20`, `LimitReached=21`, `InvalidDueDate=22`,
`InvalidFundingRef=23`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `CPM_ROLE` | `(tenant, project, account, role, active)` |
| `CPM_MILE` | `(tenant, project, milestone_id, order, due_at)` |
| `CPM_DONE` | `(tenant, project, milestone_id, completed_at, version)` |

Only IDs, enums, versions, hashes, and timestamps are emitted.

## Privacy boundaries

Funding linkage is recorded as a bounded reference hash rather than an account balance or private
financial detail. Reviewer and participant addresses are public Stellar accounts and must not be
derived from private identity data. Evidence documents stay off-chain; the ledger holds only hashes.

## Dependencies

- CAPSTONE-057 objectives supply target dates and success criteria.
- CAPSTONE-058 work breakdown supplies deliverables and completion signals.
- CAPSTONE-059 schedule risk consumes milestone dependencies and due dates.
- CAPSTONE-086 milestone payments and CAPSTONE-088 financial ledger consume the completion hash and
  funding reference.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the milestone-planning flag on testnet for one cohort, dual-writing milestones to the
service database and the contract. Reconcile dependencies, orders, and completion state before
promoting. Rollback stops new milestone writes; stored records are append-only and hash-only, so no
private content needs purging and a later deployment resumes from the last stored version.

## Other-repository work

- Backend: canonicalize and hash deliverables/evidence, mirror roles, manage ids and versions, and
  reconcile completion with payment flows.
- Frontend: milestone editor, dependency and due-date view, reviewer completion controls, and
  funding linkage display.
- Indexer/ops: monitor `CPM_DONE`, alert on `CycleDetected`/`WrongReviewer`, and renew TTLs for
  active milestones.
