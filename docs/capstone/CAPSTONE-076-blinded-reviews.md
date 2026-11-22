# CAPSTONE-076: Blinded Review Modes

**Track:** Review | **Component:** on-chain | **Status:** implemented
**Issue:** #1424 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree

Goal: support open, single-blind, and double-blind modes with a single on-chain
projection that queues, exports, notifications, and audit-safe views can apply
consistently.

## Scope

`contracts/capstone-blinded-reviews` stores a `BlindConfig` per
`(tenant_id, project_id)` and a bounded set of assignments (max 64). Lifecycle:
`Configured -> Sealed -> Revealed`. Sealing requires at least two assignments
and freezes the roster. Revealing stores only a commitment to the off-chain
reveal bundle; no participant or reviewer identity is written on-chain. The
`projection` helper returns `Hidden`, `Pseudonymous`, or `Visible`.

Revalidation against `main`: no blinding primitive exists. This contract is the
mode authority consumed by review UX and audit projections.

## On-chain stores vs off-chain content

| On-chain | Type | Notes |
| --- | --- | --- |
| `Config(tenant_id, project_id)` | struct | Mode, state, version, commitments, count, timestamps |
| `Assignments(tenant_id, project_id)` | `Vec<BytesN<32>>` | Bounded blinded references |
| `Assignment(tenant_id, project_id, blinded_ref)` | struct | Commitments and version only |
| `Request(tenant_id, project_id, request_id)` | struct | Temporary receipt |
| `Role(subject, tenant_id, project_id, role)` | `bool` | Stored grants |

Off-chain: the identity maps behind `participant_commitment` and
`reviewer_commitment`, sealed assignment manifests, reveal bundles, queue
display names, notification templates, and export column policies. The chain
sees only SHA-256 commitments and opaque references.

## Authorization

Global admin grants tenant/project scoped `BlindCoordinator` and `Auditor`
roles. `configure`, `register_assignment`, and `seal` require `require_auth()`
plus a stored `Admin` or `BlindCoordinator` role. `reveal` requires a stored
`Admin` role, so a coordinator cannot unblind unilaterally. Unknown addresses
deny by default.

## Idempotency

`request_id` and `payload_hash` produce a temporary receipt keyed by
`(tenant_id, project_id, request_id)`. Retries return the stored configuration
version with no extra event; a different payload returns `RequestConflict`.
State and version checks run after the replay lookup so retries remain stable.

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
| 11 | `InvalidHash` |
| 12 | `InvalidCommitment` |
| 13 | `MissingPrerequisite` |
| 14 | `ArithmeticOverflow` |
| 15 | `DuplicateAssignment` |

Codes are append-only and never renumbered.

## Events

`BLGRANT`, `BLRVOK` publish `(tenant_id, project_id, role)`. `BLNEW` publishes
`(tenant_id, project_id, mode, version)`; `BLASGN` publishes
`(tenant_id, project_id, blinded_ref, version)`; `BLSEAL` and `BLREVL` publish
`(tenant_id, project_id, commitment, version)`. No names, addresses, or
identity payloads are emitted.

## Privacy boundaries

`projection` returns the default `Hidden` when no config exists. Open mode is
`Visible`; single-blind is `Pseudonymous` until reveal then `Visible`;
double-blind is `Hidden` until reveal then `Pseudonymous`. Even after reveal the
ledger stores only a commitment: off-chain systems must apply the projection
before rendering queues, exports, or notifications. The commitment inputs must
include a per-tenant salt so small identity spaces cannot be brute-forced.

## Dependencies

- `capstone-identifiers`: tenant/project scoping.
- `capstone-independent-reviews`: consumes `blinded_ref` as `reviewer_ref`.
- Audit/export services: consume `projection` before materializing views.

## Test plan

`Env::default()`, `mock_all_auths()`, generated clients, `try_*` assertions,
role-scope trees, configure replay/conflict, 64-assignment bound, duplicate and
zero commitments, seal/reveal ordering, coordinator cannot reveal, timestamps,
and projection values for all modes and states.

## Rollout / rollback

Deploy flag-gated; the backend writes mode state after DB writes and reconciles
by `request_id`. Rollback disables new writes; sealed configs remain immutable
and the UI falls back to `Hidden` unless a local override is explicitly
authorized. Upgrades append fields/codes and keep prior record layouts readable.

## Cross-repository work (CAPSTONE-076)

- **Backend:** salted commitment generation, identity mapping vault, manifest
  signing, and reveal workflow.
- **Frontend:** queue/export renderers that honor `projection`.
- **Notifications:** template redaction driven by the same projection enum.
- **Audit:** audit-safe projection service and access logging.
