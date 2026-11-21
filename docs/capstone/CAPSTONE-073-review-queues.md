# CAPSTONE-073: Create Proposal Review Queues

**Track:** Review | **Component:** on-chain | **Issue:** #1421 | **Status:** implemented
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree.
**Contract:** `contracts/capstone-review-queues`

## Scope

Give scoped reviewers a bounded, filterable queue with assignment, expertise
fit, priority, conflict declarations, due dates, workload indicators, and an
append-only audit trail. Proposal content, reviewer notes, rubrics, and reviewer
identity details stay off-chain. The chain stores only ids, roles, enums, bit
masks, timestamps, counters, and status.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin`, `Role(tenant_id, account, role)`, `RoleCount(tenant_id)` | Admin plus tenant-scoped `Coordinator`, `Reviewer`, `Observer` roles (32 grants per tenant). |
| `Version(tenant_id, project_id)`, `Request(...)` | Optimistic version and idempotency record. |
| `Reviewer(tenant_id, project_id, reviewer)`, `ReviewerCount(...)` | Capacity, expertise mask, assigned/decided counters, active flag, creation time, version; 32 reviewers per project, capacity 1..32. |
| `QueueIndex(tenant_id, project_id)` | Item ids, capped at 128. |
| `Item(tenant_id, project_id, item_id)` | Sequence, priority, required expertise mask, computed fit, optional assignee, due date, conflict flag, state, optional outcome, timestamps, version. |
| `Conflict(tenant_id, project_id, item_id, reviewer)` | Per-reviewer conflict marker. |
| `Audit(tenant_id, project_id, item_id)` | Up to 32 audit entries: kind, actor role, ledger timestamp. |

Off-chain: proposal text, reviewer names and profiles, detailed expertise
descriptions, rubric answers, comments, assignment notifications, and SLA
dashboards. `item_id` is the opaque proposal reference from the proposal
service; no title or summary is accepted by the contract.

## Authorization

`initialize` authenticates the admin; the admin grants/revokes tenant-scoped
roles. Every mutation authenticates the caller and checks the stored role:
`Coordinator` configures reviewers, enqueues, assigns, reprioritizes; the
current `Reviewer` assignee declares conflicts and records outcomes; reads
accept `Coordinator`, `Reviewer`, `Observer`, or the admin. A reviewer role in
another tenant is insufficient (deny by default).

## Idempotency

All mutations take `request_id` and `expected_version` and store a
domain-separated input commitment. Replays return the recorded version with no
new event or audit entry. Payload reuse with the same request id fails with
`RequestConflict`; stale new requests fail with `VersionConflict`. Assignment
also takes `expected_assignee` as an optimistic token: a stale token fails with
`AssignmentConflict` and leaves the newer assignee intact. Counters, capacity
checks, and overdue counts use checked arithmetic. Administrative role
grants/revokes are boolean writes on deterministic keys and are naturally
idempotent, so they do not consume a `request_id`.

## Errors

Append-only `ContractError` codes: `1 NotInitialized`, `2 AlreadyInitialized`,
`3 Unauthorized`, `4 VersionConflict`, `5 VersionOverflow`, `6 RequestConflict`,
`7 NotFound`, `8 AlreadyExists`, `9 InvalidInput`, `10 InvalidState`,
`11 NotReviewer`, `12 ReviewerAtCapacity`, `13 AssignmentConflict`,
`14 ConflictOfInterest`, `15 CollectionFull`, `16 InvalidPage`,
`17 InvalidFilter`.

## Events

`CQRINIT(admin)`, `ROLEGR(tenant_id, account, role)`,
`ROLERV(tenant_id, account, role)`, `CQRRV(tenant_id, project_id, capacity,
expertise_mask)`, `CQRENQ(tenant_id, project_id, item_id, priority)`,
`CQRASS(tenant_id, project_id, item_id, fit)`,
`CQRCOI(tenant_id, project_id, item_id, declared)`,
`CQRPRIO(tenant_id, project_id, item_id, priority)`, and
`CQROUT(tenant_id, project_id, item_id, outcome)`. Payloads are ids, enums,
masks, and flags only; no proposal text or reviewer comment is emitted.

## Privacy boundaries

Reviewer addresses are stored as pseudonymous keys for capacity and conflict
enforcement; they never appear in events or audit entries, where only a role
enum is recorded. Expertise fit is derived from bit masks, not free-text skill
descriptions. Due dates are ledger timestamps, not human schedule text. A
declared conflict blocks that reviewer's outcome but does not silently
unassign or erase the item. Bounded queues (128), reviewer pools (32),
capacities (32), and audit trails (32) fail explicitly rather than evicting
records.

## Dependencies

Depends on CAPSTONE-038/039 proposal submission for opaque item ids,
CAPSTONE-072 for feed entries, and the off-chain reviewer directory for
expertise masks and notifications. Matching work in other repositories owns
the reviewer console, conflict UI, workload dashboards, and SLA reporting.

## Rollout / rollback

Deploy behind the review feature flag, initialize, grant coordinators,
configure reviewers before opening intake, then enable assignment. Existing
proposals are enqueued with a fresh `item_id` and an off-chain `Migrated`
marker; nothing from the proposal body is copied on-chain. Rollback: stop new
assignments and outcomes, keep reads/indexed queues available, and revoke
roles if needed. Storage records and error codes are append-compatible; the
audit trail is intentionally immutable.

## Corresponding work in other repositories (CAPSTONE-073)

- **Backend/review-service:** proposal-to-item mapping, reviewer directory,
  expertise mask generation, conflict workflow, notification delivery.
- **Frontend:** reviewer console, filters, due-date indicators, conflict
  declaration, workload dashboard, outcome submission.
- **Mobile:** assignment alerts and read-only queue summary.
- **Docs/indexer:** queue projection schema, audit export, capacity planning.
