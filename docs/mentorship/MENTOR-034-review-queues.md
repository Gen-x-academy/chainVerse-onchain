# MENTOR-034: Application Review Queues

**Track:** Applications | **Component:** on-chain | **Status:** implemented

Goal: Add application review queues.

## On-chain

`contracts/mentorship-applications` implements program-scoped review queues on
top of the submitted application records:

- `grant_reviewer(admin, program_id, reviewer, capacity)` gives a reviewer a
  program-scoped role with a non-zero active-assignment capacity;
  `revoke_reviewer` deactivates the scope but retains capacity for an explicit
  re-grant. The pool is bounded at 32 reviewers per program.
- `set_priority` sets `Low`/`Normal`/`High`/`Urgent` (default `Normal`).
- `assign_reviewer(caller, application_id, reviewer, expected_assignee, due_at)`
  assigns or reassigns with a due date (`0` = none). `expected_assignee` is the
  optimistic concurrency token: a stale value fails with `AssignmentConflict`
  and leaves the newer assignment intact, so two coordinators cannot silently
  overwrite each other. Repeating an identical assignment is an idempotent
  no-op. A reviewer at capacity fails with `ReviewerAtCapacity`.
- `declare_conflict(reviewer, application_id, declared)` records a per-reviewer
  conflict of interest. A declared conflict blocks that reviewer's decision
  (`ConflictOfInterest`). The reviewer stays assigned so coordinators can see
  and reassign them; clearing the declaration unblocks the decision.
- `record_decision(reviewer, application_id, outcome)` is available only to the
  current assignee, who must still be scoped and unconflicted. It records
  `Accepted` or `Rejected` once; further decisions fail with
  `ApplicationAlreadyDecided`, as do priority changes and reassignment.
- `list_queue(caller, program_id, filter)` is permissioned: only the admin or an
  active reviewer scoped to that program can read it, so only permitted
  applications appear. Filters (`status`, `priority`, `assignee`,
  `unassigned_only`, `due_before`) combine with AND, ordering is deterministic
  (priority descending, due date ascending with undated last, then ascending
  application id), and pages are capped at 64 rows.
- `reviewer_workload(program_id, reviewer)` reports `capacity`,
  `assigned_active`, `decided` and `overdue`. Queue rows carry `overdue` too.
- `get_audit_trail(application_id)` returns every queue action
  (`Assigned`, `PriorityChanged`, `ConflictDeclared`, `ConflictCleared`,
  `DecisionRecorded`) with actor and timestamp, bounded at 64 entries. Audit
  entries are appended before the corresponding state write, so a full trail
  fails the call without a partial, unaudited mutation.

## Off-chain boundary

Applicant identity is an `Address` (pseudonymous). Application content, reviewer
notes, rubrics and rationale stay off-chain. The contract records only queue
metadata, the decision outcome and the audit log. Reviewer notifications,
workload dashboards beyond the on-chain counters, SLA reporting and reviewer
console filtering UX are application-service concerns; the contract exposes the
permissioned primitive they read.

## Rules

- Only permitted applications appear: queue reads and queue writes require an
  active program-scoped reviewer or the admin.
- Assignment is concurrency-safe (`expected_assignee`), idempotent on retry and
  capacity-bounded per reviewer.
- Conflicts block decisions for the conflicted reviewer; they do not silently
  unassign or erase the assignment.
- Filters are stable: the same state yields the same filtered, ordered page,
  with deterministic tie-breaks.
- Queue actions are audited in order with actor and ledger timestamp; the audit
  trail is bounded and immutable.
- Decisions are final and single-shot; a decided application cannot be
  re-prioritised, reassigned or decided again.
- Errors are stable codes: `NotAuthorized`, `NotReviewer`, `AlreadyReviewer`,
  `ReviewerPoolFull`, `ReviewerAtCapacity`, `AssignmentConflict`,
  `ApplicationAlreadyDecided`, `ConflictOfInterest`, `NotAssignedReviewer`,
  `InvalidDecision`, `InvalidFilter`, `InvalidCapacity`, `QueueFull`,
  `AuditTrailFull`, `ApplicationNotFound`.

## Events and errors

Events: `REVGRANT(program_id, reviewer, capacity)`,
`REVOKE(program_id, reviewer)`, `PRIO(application_id, priority)`,
`ASSIGN(application_id, reviewer, due_at)`,
`COI(application_id, reviewer, declared)`,
`DECIDE(application_id, reviewer, outcome)`. Errors are the variants listed
above.

## Dependencies and rollout

Depends on MENTOR-030 submissions (queue rows are created atomically with an
application), MENTOR-032 forms (reviewers see the accepted form version) and
the contract's scoped-reviewer model. The matching MENTOR-034 work in the other
repositories owns the reviewer console, assignment notifications, conflict
declaration UI and workload dashboards. Ship behind the mentorship feature flag;
grant reviewers before opening intake so assignment capacity is visible from
day one. `MAX_QUEUE = 256` per program is a hard intake cap operators must plan
around. Full contract notes: `contracts/docs/mentorship-applications.md`.
