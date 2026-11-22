# CAPSTONE-064: Handle Deadline Extension Requests

**Track:** Planning | **Component:** on-chain | **Issue:** #1412 | **Status:** implemented
**Audit baseline:** `main` @ `8713c23` (2026-11-22).
**Contract:** `contracts/capstone-deadline-extensions`

## Scope

Record reasoned deadline extension requests with an impact preview commitment, a required number of
approvals, a revised schedule commitment, the funding consequence, and an immutable final decision.
The reason text, the impact preview content, the revised schedule, and reviewer commentary stay
off-chain. The chain stores commitments, deadlines, a public funding amount, counters, and states.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin` (instance) | One-time deployment administrator. |
| `Role(account, tenant, project, role)` | `RoleGrant { active, expires_at }` for `Manager`, `Requester`, `Approver`, `Viewer`. |
| `Request(tenant, project, request_id)` | Idempotency record: input commitment and recorded result version. |
| `Extension(tenant, project, extension_id)` | `ExtensionRecord`: target id, requester, `reason_hash`, `impact_hash`, current and proposed deadline, `funding_delta`, approvals required and received, `expires_at`, state, `decision_hash`, `revised_schedule_hash`, timestamps, version. |
| `ExtensionIndex(tenant, project)` | Append-only list of extension ids, capped at 64. |
| `Open(tenant, project, target_id)` | Pointer to the single open request for a target. |
| `Approval(tenant, project, extension_id, approver)` | One-shot approval marker storing the approval commitment. |

## Authorization

| Entry point | Manager | Requester | Approver | Viewer |
| --- | --- | --- | --- | --- |
| `submit_request` | no | yes | no | no |
| `withdraw` | no | yes, only the original requester | no | no |
| `approve`, `reject` | no | no | yes, never the requester | no |
| `publish_revision` | yes | no | no | no |
| `expire` | yes | yes | yes | yes |
| `get_extension`, `list_extensions`, `open_request_for` | yes | yes | yes | yes |

Deny by default and scoped to the exact tenant and project. An address that also holds `Requester`
still cannot approve or reject its own request (`SeparationOfDuty`). Withdrawal checks the stored
requester, so a different requester gets `Unauthorized`.

## Lifecycle

```
Requested --(approvals >= required)--> Approved --publish_revision--> Applied
    |                                      |
    |-- reject ---------> Rejected         |-- expire (after expires_at) --> Expired
    |-- withdraw -------> Withdrawn
    |-- expire (after expires_at) --> Expired
```

`Rejected`, `Withdrawn`, `Expired`, and `Applied` are terminal and immutable. `Applied` stores the
`revised_schedule_hash` and final `decision_hash` once; no later call can change them. Each approval
counts once per approver (`AlreadyApproved`). Every call that acts on an existing request, except `expire`, takes
`expected_version`; `approve` bumps the version, so two approvers acting on the same version
serialize and the loser receives `StaleVersion` and retries with the fresh version. Stale checks run
before transition rules.

## Concurrency and duplicate protection

At most one request may be open per `target_id` (`AlreadyOpen`). The pointer is released on
`Rejected`, `Withdrawn`, `Expired`, and `Applied`, so a new request can follow. An existing
`extension_id` can never be reused (`AlreadyExists`). Idempotent replays return the recorded version
with no write and no event; reusing a `request_id` with a different payload or actor returns
`RequestConflict`.

## Validation and time rules

`proposed_deadline` must exceed `current_deadline` by at most 90 days. `current_deadline` must still
be in the future (`DeadlinePassed`). `expires_at` must be in the future, within 30 days, and no later
than `current_deadline`, so a decision cannot outlive the deadline it concerns. Approvals required is
1 to 8. `funding_delta` is 0 to 10^18 base units (zero means no funding consequence). Approving,
rejecting, or publishing at or after `expires_at` returns `RequestExpired`. Any scoped role may call
`expire` once the window has passed (`NotYetExpired` before) so a stalled review cannot block a target
forever. Hashes must be non-zero salted commitments.

## Funding consequences

`funding_delta` is a public amount in the project's funding asset base units, stored and emitted so
escrow and budget services can act on it. This contract moves no funds. Budget and escrow adjustments
are performed by the funding contracts (`capstone-budget-plans`, `capstone-escrow-intents`) in a
separate transaction, triggered by an `Applied` record. That transaction must read the record rather
than trust an event alone.

## Errors

Append-only codes: `1 AlreadyInitialized`, `2 NotInitialized`, `3 Unauthorized`, `4 RequestConflict`,
`5 StaleVersion`, `6 NotFound`, `7 AlreadyExists`, `8 InvalidInput`, `9 InvalidTransition`,
`10 LimitReached`, `11 InvalidPage`, `12 ArithmeticError`, `13 RoleExpired`, `14 AlreadyOpen`,
`15 DeadlinePassed`, `16 SeparationOfDuty`, `17 RequestExpired`, `18 AlreadyApproved`,
`19 NotYetExpired`.

## Events

| Topic | Payload |
| --- | --- |
| `EXT_INIT` | `(admin)` |
| `EXT_ROLE` | `(tenant, project, account, role, active, expires_at)` |
| `EXT_REQ` | `(tenant, project, extension_id, target_id, proposed_deadline, funding_delta, version)` |
| `EXT_APPR` | `(tenant, project, extension_id, approvals, state, version)` |
| `EXT_REJ` | `(tenant, project, extension_id, version)` |
| `EXT_WDR` | `(tenant, project, extension_id, version)` |
| `EXT_APL` | `(tenant, project, extension_id, proposed_deadline, funding_delta, version)` |
| `EXT_EXP` | `(tenant, project, extension_id, version)` |

Reasons, impact previews, schedules, and comments are never broadcast. Replays and failures emit nothing.

## Privacy boundaries and off-chain boundary

The impact preview is computed and rendered off-chain; only `impact_hash` is committed so reviewers
can prove which preview they saw. Approver identities are stored as addresses to enforce one-shot
approval; use per-tenant approver keys if stronger pseudonymity is required. The amount and deadlines
are intentionally public.

## Dependencies

`capstone-project-milestones` (CAPSTONE-060) supplies `target_id` values and the deadline being moved.
`capstone-project-decisions` (CAPSTONE-070) can reference `decision_hash`. Funding contracts consume
applied `funding_delta`. `capstone-project-timeline` (CAPSTONE-063) displays the decision.

## Rollout and rollback

Deploy, initialize, grant roles, then enable the request flow in the service. In-flight requests at
upgrade time stay readable and can complete or expire. Rollback: stop invoking mutating entry points;
open requests expire naturally and release their targets.

## Upgrade compatibility

Error codes, enum discriminants, and `DataKey` variants are append-only and snapshot-tested. A
stored-struct field change is not decode-compatible and must ship as a new `DataKey` variant. A test
writes `Extension`, `Open`, `Role`, and `Request` records directly to prove a newer build continues
an older request through its lifecycle and still honours its open-target pointer.

## Test coverage map

Reasoned request storage; full authorization tree including requester ownership; tenant isolation;
replay and conflict; one-open-per-target and duplicate ids; approval counting; separation of duties;
stale concurrent decisions; immutable rejection; withdrawal rules; single publication; expiry of
pending and approved requests; every validation boundary; role expiry; 64-request and paging bounds;
event schemas for all eight topics; error and enum snapshots; legacy decode; CPU and memory budgets.

## Corresponding work in other repositories (CAPSTONE-064)

- **Backend/schedule-service:** store reasons and impact previews, compute previews, produce revised schedules, notify approvers, call funding services on `Applied`.
- **Frontend:** request form, impact preview, approval inbox, decision record.
- **Mobile:** approval actions and notifications through the service.
- **Docs/indexer:** projection of `EXT_*` events and applied schedule history.
