# CAPSTONE-053: Pause and Resume Projects

**Track:** Governance | **Component:** on-chain | **Issue:** #1401 | **Status:** specified (not yet implemented)

Goal: Pause and resume projects.

## Baseline

Revalidated against `main` at `4471755`. No capstone contract tracks project run
state. The `escrow` and `escrow-vault` contracts hold funds but don't know about
capstone pauses.

## On-chain

New crate `contracts/capstone-pauses`. It keeps a per-project run state and a
bounded log of pause records. Other contracts (charters, escrow release) query
`is_paused` before guarded actions.

### Data model

| Type | Fields | Notes |
| --- | --- | --- |
| `PauseRequest` | `pause_id: u32`, `project_id`, `tenant_id`, `requested_by`, `reason_code: u32`, `reason_ref: BytesN<32>`, `starts_at: u64`, `max_until: u64`, `deadline_shift_secs: u64`, `funding_action: FundingAction`, `required_count: u32`, `approvals: u32`, `state: PauseState`, `resumed_at: Option<u64>` | |
| `PauseState` | `Requested`, `Active`, `Resumed`, `Expired`, `Rejected`, `Cancelled` | |
| `FundingAction` | `None`, `HoldReleases`, `HoldAndReturnUnspent` | This only tells escrow what to do. The contract never moves funds itself. |
| `RunState` | `Running`, `Paused(pause_id)` | Stored under `Run(project_id)`. |

`reason_code` comes from a fixed policy list: `1 = medical`, `2 = family`,
`3 = sponsor_hold`, `4 = supervisor_unavailable`, `5 = safety_review`,
`6 = administrative`. Any other value fails with `InvalidReason`. `reason_ref`
commits to the off-chain justification, which may include sensitive details.

### Policy

Each tenant sets this with `set_policy(admin, tenant_id, PausePolicy)`:
`max_duration_secs`, `max_pauses_per_project` (≤ 8), `cumulative_cap_secs`, and
an approval rule for each reason code: `SelfService`, `Supervisor`, or
`SupervisorAndSponsor`. `safety_review` is always `Coordinator` only.

### Entry points

- `request_pause(actor, project_id, reason_code, reason_ref, starts_at, max_until, deadline_shift_secs, funding_action, request_nonce)`:
  `actor` must be a project participant, the supervisor, or a coordinator.
  The request is checked against the policy limits.
- `approve_pause(approver, project_id, pause_id, intent_nonce)`: once the
  required approvals are in, the request becomes `Active`, either at
  `starts_at` or straight away if `starts_at <= now`.
- `reject_pause(approver, project_id, pause_id)` and
  `cancel_pause(requester, project_id, pause_id)`: these apply only to a
  `Requested` pause.
- `resume(actor, project_id, pause_id, resume_checks_hash)`: the supervisor or a
  coordinator can call it early, before `max_until`. `resume_checks_hash`
  commits to the off-chain readiness checklist.
- `expire_pause(caller, project_id, pause_id)`: anyone can call it once
  `now > max_until`. It marks the pause `Expired` and sets the project back to
  `Running`, so a pause can never last forever. Expiry and resume both emit
  `PSRESUME` with the reason, which lets downstream consumers handle them the
  same way.
- `is_paused(project_id) -> bool`, `get_pause`, `get_pause_history` (capped at
  `max_pauses_per_project`), `effective_deadline_shift(project_id) -> u64`.
  The last one returns the total shift from completed pauses, capped at the
  real paused duration.

### Lifecycle

```
Running ──request──▶ [Requested] ──approvals──▶ Active (project Paused)
                         │                          │
                         ├──reject──▶ Rejected      ├──resume──▶ Resumed (project Running)
                         └──cancel──▶ Cancelled     └──now > max_until, expire──▶ Expired (project Running)
```

Rules:
- A project can have only one `Requested` or `Active` pause at a time
  (`PauseAlreadyOpen`).
- A pause can't be requested while a charter amendment is `Proposed`
  (`AmendmentOpen`), and an amendment can't be proposed while the project is
  `Paused`.
- When a project resumes, the deadline shift that applies is the smaller of the
  requested `deadline_shift_secs` and the time actually spent paused. The rest
  of the requested shift is not applied. The backend applies the shift to
  milestone dates.
- If the charter became material-amended during the pause, or guardian consent
  lapsed during it, `resume` fails with `ReconsentRequired` until consent is
  given again (CAPSTONE-051). `expire_pause` still sets the run state back to
  `Running`, but the guarded actions in the other contracts stay blocked until
  consent is renewed.

## Authorization and tenant boundaries

- Deny by default. The approval rule is resolved from the tenant's policy, never
  from what the caller passes in.
- A requester can't approve their own pause unless the rule is `SelfService`.
- Calls from another tenant fail with `WrongTenant`. The policy can only be
  changed by the tenant admin.

## Replay and idempotency

- `request_nonce` and `intent_nonce` are single-use per actor.
- Approving twice returns `AlreadyApproved`. Calling `resume` or `expire_pause`
  on a pause that has already ended returns `PauseNotActive` and changes
  nothing, so escrow never receives two hold or release signals.
- The funding action takes effect only through the `PSACTIVE` and `PSRESUME`
  events, which escrow and the backend consume idempotently, keyed by
  `(project_id, pause_id)`.

## Off-chain boundary

| Stays off-chain | On-chain |
| --- | --- |
| Medical, family or safety details behind a pause | `reason_code` (coarse category) plus the `reason_ref` commitment |
| Evidence documents, supervisor notes | Nothing on-chain |
| Milestone dates and the rescheduled plan | `deadline_shift_secs` only |
| Fund amounts and refund calculations | `funding_action` enum. Escrow owns the amounts. |
| Resume readiness checklist | `resume_checks_hash` |

If a tenant decides even the coarse category is too revealing, it can map every
reason to `6 = administrative`.

## Events and errors

| Event | Topics | Data |
| --- | --- | --- |
| `PSREQ` | `(project_id,)` | `(pause_id, reason_code, starts_at, max_until)` |
| `PSAPPR` | `(project_id, pause_id)` | `(approver,)` |
| `PSACTIVE` | `(project_id,)` | `(pause_id, funding_action)` |
| `PSREJ` / `PSCANCEL` | `(project_id, pause_id)` | `()` |
| `PSRESUME` | `(project_id,)` | `(pause_id, applied_shift_secs, expired: bool)` |

`ContractError`: `Unauthorized = 1`, `WrongTenant = 2`, `PolicyNotSet = 3`,
`InvalidReason = 4`, `DurationExceedsPolicy = 5`, `PauseLimitReached = 6`,
`CumulativeCapExceeded = 7`, `PauseAlreadyOpen = 8`, `PauseNotFound = 9`,
`PauseNotActive = 10`, `PauseNotRequested = 11`, `AlreadyApproved = 12`,
`SelfApprovalNotAllowed = 13`, `NonceUsed = 14`, `InvalidWindow = 15`,
`NotExpiredYet = 16`, `AmendmentOpen = 17`, `ReconsentRequired = 18`,
`ProjectNotRunning = 19`.

## Required tests

- Authorization: each approval rule, self-approval, a cross-tenant approver, and
  `safety_review` approved by a non-coordinator.
- Replay and concurrency: two requests in parallel, approving twice, and calling
  resume twice or resume after expiry (only one `PSRESUME` should be emitted).
- Transitions: every edge in the diagram, plus the guards involving amendments
  and consent.
- Expiry: `expire_pause` at `max_until` fails, and at `max_until + 1` it
  succeeds. The deadline shift should be capped at the actual paused duration.
- Storage bounds: `max_pauses_per_project` and the cumulative cap.
- Event snapshots, error-code assertions, resource budgets for `approve_pause`
  and `resume`, and upgrade compatibility of `PauseRequest` and `PausePolicy`.

## Dependencies, rollout and rollback

- Depends on `capstone-identifiers`, `capstone-charters` (open-amendment check),
  `capstone-guardian-consent` (resume gate), and the tenant role registry.
  Consumed by `escrow` / `escrow-vault`, which need a small adapter that checks
  `is_paused` before releasing funds (tracked separately).
- Rollout: deploy it, set conservative policies per tenant, and turn on the
  escrow hold adapter only after the event consumer is idempotent in
  production.
- Rollback: turn off the escrow adapter first, then resume or expire every
  active pause so that no project stays `Paused` while no contract is running.
  The pause history stays on-chain.
- Other repositories: the backend handles the policy configuration UI and API,
  the evidence store, deadline recalculation and the escrow orchestration. The
  frontend handles the pause request form, the approval queue, the paused
  project banner and the resume checklist.
