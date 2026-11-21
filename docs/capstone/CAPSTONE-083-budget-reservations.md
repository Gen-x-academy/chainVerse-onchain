# CAPSTONE-083: Reserve Approved Project Budgets

**Track:** Funding | **Component:** on-chain | **Issue:** #1431 | **Contract:** `contracts/capstone-budget-reservations`

## Scope

Atomically reserve sponsor or program funds for an approved project budget. A reservation consumes
part of a tenant-scoped, asset-specific cap, respects expiry, and can be released or expired. One
active reservation is allowed per project; retries and concurrent decisions cannot double-reserve.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `BudgetCap`: tenant, cap id, asset hash, limit, reserved, version, expiry, request id/hash | Sponsor identity, cap memo, bank/payment-provider account details |
| `Reservation`: request id, cap id, plan hash, asset hash, amount, state, version, timestamps | Plan prose, sponsor display names, reservation receipts, approval emails |
| `RequestMarker`, release/expiry markers, role grants | Canonical request payloads and notifications |
| Events with IDs, hashes, amounts, state | Audit UI and reporting exports |

`plan_hash` is the commitment to the approved budget plan from CAPSTONE-082. `asset` is a stable
32-byte asset identifier, not a token contract call in this primitive.

## Authorization

- One-time `initialize` requires the administrator's `require_auth`.
- Tenant-scoped stored roles: `ProgramManager` configures caps, `Sponsor` reserves/releases,
  `Auditor` marks expiry. `grant_role`/`revoke_role` are admin-only.
- Every mutating call calls `require_auth` on the caller and checks the stored role for the
  supplied `tenant_id`. No role means `Unauthorized`; there is no default allow and no override.
- All keys include `tenant_id` and `project_id` or `cap_id`; cross-tenant lookups return `NotFound`
  and cross-tenant writes fail the role check before any storage change.

## Idempotency and versioning

- `request_id` plus `expected_version` is the cap/reservation replay key. A retry returns the
  stored cap version or reservation unchanged and emits no event.
- A reused key with a different hash or base version returns `RequestConflict`.
- `release` and `mark_expired` have separate request markers, so retries are no-ops even after the
  reservation state has changed; a different key on a terminal state returns `InvalidState`.
- Transitions: cap `v -> v+1` on configuration; reservation `Active -> Released | Expired`.
  A new reservation for the same project is only possible after a terminal state.
- Bounds: `MAX_CAP_VERSIONS = 8`, `MAX_RESERVATIONS_PER_PROJECT = 8`. Reserved totals use
  `checked_add`/`checked_sub` and are compared against the cap limit before writing. All persistent
  keys extend TTL.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`WrongTenant=4`, `NotFound=5`, `InvalidAmount=6`, `InvalidExpiry=7`, `InvalidLimit=8`,
`CapExpired=9`, `AssetMismatch=10`, `CapExceeded=11`, `AlreadyReserved=12`, `VersionConflict=13`,
`RequestConflict=14`, `InvalidState=15`, `CapacityExceeded=16`, `Overflow=17`, `Underflow=18`.
Append new codes only; never renumber existing ones.

## Events

| Symbol | Data |
| --- | --- |
| `CBPROLG` / `CBPRVRK` | `(tenant_id, account, role)` |
| `CBRGCAP` | `(tenant_id, cap_id, version, limit)` |
| `CBRRES` | `(tenant_id, project_id, request_id, amount, asset)` |
| `CBRREL` | `(tenant_id, project_id, request_id, amount)` |
| `CBREXP` | `(tenant_id, project_id, request_id, amount)` |

No provider, sponsor, or memo text is included in event topics or data.

## Privacy boundaries

Caps and amounts are financial metadata and are public on-chain by design. Sponsor names, cap
descriptions, bank details, and plan contents stay in the service and are represented only by
`cap_id`, `plan_hash`, `request_hash`, and `asset`. Addresses are public keys and must be allocated
for ledger use only. No private reference string appears in storage or events.

## Dependencies

- CAPSTONE-082 supplies the approved `plan_hash`.
- `capstone-identifiers` supplies `project_id`; the shared tenant registry supplies role assignments
  mirrored by the backend.
- CAPSTONE-084 reads the reservation outcome to create a funding intent off-chain; there is no
  cross-contract call in this primitive.

## Rollout and rollback

Deploy behind the funding flag, seed caps from the finance service, and run a reconciliation job
that compares `reserved` with the database for one cohort. Rollback stops new reservations; the
service can release or expire outstanding reservations through existing entry points. Records are
hashes and amounts only, so no private purge is required. Keeping cap records alive via TTL renewal
preserves upgrade continuity.

## Other-repository work

- Backend: cap provisioning, sponsor roster sync, idempotency key generation, reconciliation, and
  expiry sweeps.
- Frontend: reservation status, remaining-cap display, and release/expiry controls.
- Indexer/ops: monitor cap utilization, `CapExceeded`, `AssetMismatch`, and TTL of active caps.
