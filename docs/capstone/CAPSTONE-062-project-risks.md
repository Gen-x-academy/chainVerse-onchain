# CAPSTONE-062: Manage Project Risks and Mitigations

**Track:** Planning | **Component:** on-chain | **Issue:** #1410 | **Status:** implemented
**Audit baseline:** `main` @ `8713c23` (2026-11-22).
**Contract:** `contracts/capstone-project-risks`

## Scope

Keep a bounded, tamper-evident risk register per project. The chain records each risk's category,
likelihood, impact, derived score, owner, review schedule, lifecycle status, visibility scope, and
commitments to the mitigation and contingency plans. Mitigation text, contingency steps, review notes,
and owner identity details stay in the off-chain risk service.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin` (instance) | One-time deployment administrator. |
| `Role(account, tenant, project, role)` | `RoleGrant { active, expires_at }` for `Manager`, `Reviewer`, `Viewer`. |
| `Request(tenant, project, request_id)` | Idempotency record: input commitment and recorded result version. |
| `Risk(tenant, project, risk_id)` | `RiskRecord`: category, likelihood, impact, score, owner, `mitigation_hash`, `contingency_hash`, `review_hash`, `next_review_at`, `last_reviewed_at`, status, visibility, timestamps, version. |
| `RiskIndex(tenant, project)` | Append-only list of risk ids, capped at 128. |

Off-chain: risk descriptions, mitigation and contingency plans, review minutes, owner contact data, and
any notification text. Clients pass fresh salted SHA-256 commitments; low-entropy text must never be
hashed directly. All-zero hashes are rejected.

## Authorization

Deny by default. Every mutation calls `require_auth()` for the actor and then checks a stored,
unexpired role scoped to the exact `(tenant_id, project_id)`.

| Entry point | Manager | Reviewer | Viewer |
| --- | --- | --- | --- |
| `register_risk`, `update_assessment`, `update_contingency`, `transition_status`, `reassign_owner` | yes | no | no |
| `record_review` | yes | yes | no |
| `get_risk`, `list_risks`, `review_due` (team scope) | yes | yes | yes |
| Same reads for `Restricted` risks | yes | yes | only when the viewer is the named owner |

A role in tenant A or project A grants nothing elsewhere. A risk a caller may not see is reported as
`NotFound`, so restricted risks do not leak their existence. Unauthorized callers learn nothing about
whether a risk exists because the role check runs before any risk lookup.

## Lifecycle

`Identified`, `Mitigating`, `Accepted`, `Closed`, `Materialized`.

| From | Allowed targets |
| --- | --- |
| `Identified` | `Mitigating`, `Accepted`, `Closed`, `Materialized` |
| `Mitigating` | `Accepted`, `Closed`, `Materialized` |
| `Accepted` | `Mitigating`, `Closed`, `Materialized` |
| `Closed`, `Materialized` | none (terminal) |

Same-state moves return `InvalidTransition`. Terminal risks reject assessment edits, reviews, and owner
changes with `TerminalRisk`. Every mutation takes `expected_version`; a stale value returns
`StaleVersion` before any transition rule is evaluated. Likelihood and impact are 1 to 5 and the stored
`score` is their product (1 to 25). Review dates must be in the future and no more than 366 days ahead.
`review_due` is derived: a non-terminal risk is due when `now >= next_review_at`.

## Idempotency

Every mutation supplies `request_id`. A domain-separated SHA-256 over `(domain, tenant, project,
request_id, actor, call fields)` is stored with the result version. Replaying the exact request
returns the recorded version with no state write and no event, even after later calls advanced the
risk. Reusing a `request_id` with a different payload or a different actor returns `RequestConflict`.
Failed calls store nothing, so a corrected retry may reuse its `request_id`. A new request for an
existing `risk_id` returns `AlreadyExists`, so concurrent registrations cannot duplicate a risk.

## Bounds

128 risks per project (`LimitReached`), pages of at most 32 (`InvalidPage`), review horizon 366 days.
All persistent entries are TTL-extended on write and read. Arithmetic is checked.

## Errors

Append-only `ContractError` codes: `1 AlreadyInitialized`, `2 NotInitialized`, `3 Unauthorized`,
`4 RequestConflict`, `5 StaleVersion`, `6 NotFound`, `7 AlreadyExists`, `8 InvalidInput`,
`9 InvalidTransition`, `10 LimitReached`, `11 InvalidPage`, `12 ArithmeticError`, `13 RoleExpired`,
`14 TerminalRisk`. Codes 1 to 13 are shared by all four CAPSTONE-062 to 065 contracts.

## Events

Topics are `symbol_short!` values; payloads carry only ids, hashes, enums, and counters.

| Topic | Payload |
| --- | --- |
| `RSK_INIT` | `(admin)` |
| `RSK_ROLE` | `(tenant, project, account, role, active, expires_at)` |
| `RSK_REG` | `(tenant, project, risk_id, category, score, version)` |
| `RSK_UPD` | `(tenant, project, risk_id, score, version)` |
| `RSK_REV` | `(tenant, project, risk_id, next_review_at, version)` |
| `RSK_STS` | `(tenant, project, risk_id, from, to, version)` |
| `RSK_OWN` | `(tenant, project, risk_id, new_owner, version)` |

Replays and failed calls emit nothing.

## Privacy boundaries and off-chain boundary

Addresses are pseudonymous identifiers. The owner address is stored so `Restricted` scoping and owner
visibility can be enforced on-chain; deployments that need stronger pseudonymity should use a
per-tenant owner commitment key. Visibility is enforced for contract reads only: ledger storage is
public, so `Restricted` hides a risk from the contract API and indexer authorization, not from a raw
ledger reader. Anything that must be secret from ledger readers stays off-chain behind its hash.

## Dependencies

`capstone-identifiers` for stable project ids. `capstone-schedule-risk` (CAPSTONE-059) analyzes plan
graphs and may feed a risk's `Schedule` category through the service. `capstone-project-decisions`
(CAPSTONE-070) records accept/mitigate decisions that a risk's `mitigation_hash` can reference.
`capstone-project-timeline` (CAPSTONE-063) can display register events.

## Rollout and rollback

Deploy, initialize with a dedicated administrator key, grant tenant/project roles, then enable the
risk UI behind a feature flag. Existing registers backfill by registering with salted commitments;
there is no backfill shortcut on-chain. Rollback: stop invoking mutating entry points; reads remain
valid. There is no deletion path, so removing the contract is a deliberate retention decision.

## Upgrade compatibility

Error codes and enum discriminants are fixed and covered by snapshot tests. `DataKey` variants are
append-only. Soroban encodes stored structs by field name, so adding or removing a field of
`RiskRecord`, `RoleGrant`, or `RequestRecord` is not decode-compatible: a layout change must ship as
a new `DataKey` variant with a migration. Tests write records directly under the documented keys to
prove a newer build reads data an earlier build produced.

## Test coverage map

Authorization tree and cross-tenant denial; replay and conflict; stale versions; lifecycle and
terminal rules; review dates and derived due state; role expiry and revocation; restricted
visibility; 128-risk and paging bounds; event schemas; error and enum snapshots; legacy record decode;
CPU and memory budgets for write, transition, and full-page read.

## Corresponding work in other repositories (CAPSTONE-062)

- **Backend/risk-service:** risk register CRUD, salted commitments, review reminders, event verification.
- **Frontend:** register, heat map by score, review calendar, owner assignment; never send raw plan text on-chain.
- **Mobile:** read-only risk list and review reminders through the service.
- **Docs/indexer:** projection of `RSK_*` events, retention policy, restricted-scope handling.
