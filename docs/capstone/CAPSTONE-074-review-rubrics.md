# CAPSTONE-074: Configurable Review Rubrics

**Track:** Review | **Component:** on-chain | **Status:** implemented
**Issue:** #1422 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree

Goal: define versioned criteria, weights, guidance references, score ranges,
required-comment flags, visibility, and aggregation rules without putting
guidance text on a public ledger.

## Scope

`contracts/capstone-review-rubrics` exposes a bounded rubric registry. A rubric
moves `Draft -> Published -> Archived`. Criteria can only be added in `Draft`.
Every mutation takes `expected_version` and returns the new version; stale
writers fail with `StaleVersion`. Weights are `u32` and must sum to exactly 100
before publication. At most 16 criteria are stored per rubric.

Revalidation against `main`: no rubric primitive exists. This contract is the
on-chain configuration anchor that review and outcome contracts reference by
`rubric_id` and `rubric_version`.

## On-chain stores vs off-chain content

| On-chain | Type | Notes |
| --- | --- | --- |
| `Rubric(tenant_id, project_id, rubric_id)` | struct | Public state, version, aggregation, visibility, weight/count totals, hashes, timestamps |
| `Criteria(tenant_id, project_id, rubric_id)` | `Vec<BytesN<32>>` | Bounded index, max 16 |
| `Criterion(tenant_id, project_id, rubric_id, criterion_id)` | struct | Weight, range, flags, visibility, `guidance_hash` |
| `Request(tenant_id, project_id, request_id)` | struct | Temporary idempotency receipt with payload hash and result version |
| `Role(subject, tenant_id, project_id, role)` | `bool` | Stored authorization grants |

Off-chain: criterion guidance prose, examples, rubric titles/descriptions,
localized text, attachments, and the canonical document whose SHA-256 is
`content_hash` / `guidance_hash`. Only those hashes reach the ledger.

## Authorization

`initialize(admin)` is one-time and stores a global admin in instance storage.
`grant_role` / `revoke_role` require the global admin to sign and maintain
tenant/project scoped role entries. Every mutating call invokes
`caller.require_auth()` followed by a stored-role check for `Admin` (global,
tenant-wide, or project scoped) or `Editor`. There is no implicit allow-list:
an address with no stored role is denied. `has_role` is a public read helper.

## Idempotency

Each mutating call carries `request_id` and `payload_hash`. A temporary
`RequestReceipt` scoped by `(tenant_id, project_id, request_id)` records the
hash and resulting version. Replaying the same request returns the stored
version without writes or events. Reusing the ID with a different hash returns
`RequestConflict`. TTLs are extended on both records and receipts.

## Errors (stable, append-only codes)

| Code | Name |
| --- | --- |
| 1 | `AlreadyInitialized` |
| 2 | `NotInitialized` |
| 3 | `Unauthorized` |
| 4 | `NotFound` |
| 5 | `AlreadyExists` |
| 6 | `InvalidTransition` |
| 7 | `StaleVersion` |
| 8 | `VersionOverflow` |
| 9 | `RequestConflict` |
| 10 | `CollectionFull` |
| 11 | `InvalidRange` |
| 12 | `InvalidWeight` |
| 13 | `ArithmeticOverflow` |
| 14 | `InvalidHash` |
| 15 | `MissingPrerequisite` |

Codes are never reused or renumbered; new failures append new codes.

## Events

`RBGRANT`, `RBRVOK` publish `(tenant_id, project_id, role)`. `RBNEW`, `RBCRIT`,
`RBPUB`, `RBARCH` publish `(tenant_id, project_id, rubric_id, version)`. No
subject addresses, guidance text, or private metadata are emitted.

## Privacy boundaries

Visibility is an enum only (`Public`, `ReviewerOnly`, `Hidden`); it instructs
off-chain projections and is not a confidentiality guarantee for ledger data.
Guidance and free text stay in the rubric service. Role subjects are stored as
addresses for authorization but never published in events.

## Dependencies

- `capstone-identifiers`: `tenant_id` / `project_id` scoping and public IDs.
- `capstone-independent-reviews`: consumes `rubric_id` + `rubric_version`.
- `capstone-review-outcomes`: consumes the frozen aggregation rule and weights.

## Test plan

Deterministic `Env::default()` with `mock_all_auths()`, generated clients,
`try_*` assertions for every error code, scoped-role authorization trees,
request replay/conflict, stale versions, weight/range validation, 16-criterion
bound, one-time initialization, and timestamp assertions.

## Rollout / rollback

Deploy behind a feature flag; the rubric service dual-writes (DB first, then
chain) and reconciles by `request_id`. Rollback disables writes while published
records remain immutable and readable; the service falls back to DB-only
rubrics. Because codes and record layouts are append-only, a later version can
read v1 records after upgrade.

## Cross-repository work (CAPSTONE-074)

- **Backend:** canonical rubric hashing, role provisioning, dual-write worker,
  `StaleVersion` retry handling.
- **Frontend:** rubric builder with weight total, range, and visibility controls.
- **Docs/ops:** guidance content repository, deploy configuration, and runbook.
