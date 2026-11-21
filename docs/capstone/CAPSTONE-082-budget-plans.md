# CAPSTONE-082: Create Project Budget Plans

**Track:** Funding | **Component:** on-chain | **Issue:** #1430 | **Contract:** `contracts/capstone-budget-plans`

## Scope

Create versioned budget plans for capstone projects using integer minor units. A plan records
categorized cost lines, quantities, unit amounts, computed totals, funding-source and justification
commitments, an approval threshold, and an expiry. Plans move through `Draft -> Submitted ->
Approved`; an approved plan may be superseded by one higher version.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `BudgetPlan`: tenant, project, version, state, line-set hash, total, threshold, approval count, request id/hash, timestamps | Category names, justification prose, funding-source documents, budget spreadsheet |
| `BudgetLine`: category/justification/funding-source hashes, quantity, unit amount, checked line total | Human-readable labels and attachments |
| `RequestMarker`, approval markers, role grants | Request payloads, signer identity data |
| Events with IDs, version, amount, role enums | Notifications, audit UI rendering |

`lines_hash` and `request_hash` are SHA-256 commitments over canonical off-chain JSON. No free
text, URI, memo, or private reference is written to the ledger.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants or revokes tenant-scoped roles: `Coordinator`, `Reviewer`, `Auditor`.
- Every mutating call invokes `require_auth` on the caller and then checks the stored role for the
  exact `tenant_id`. A missing or revoked role returns `Unauthorized`; deny is the default.
- Keys are `(tenant_id, project_id, ...)`. A caller cannot act on another tenant's project because
  they cannot hold the matching stored role; records also recheck `tenant_id`.
- There is no admin override for proposing or approving a plan.

## Idempotency and versioning

- `request_id` plus `expected_version` form the replay key. A retry with the same `request_hash`
  returns the original target version and emits no second event.
- A reused `request_id` with a different hash or base version returns `RequestConflict`.
- Approvals use their own `request_id` and `expected_version`, one per reviewer per version.
  Replaying an approval is a no-op; a second approval by the same reviewer returns
  `AlreadyApproved`.
- State transitions: `Draft --submit--> Submitted --threshold reached--> Approved --propose-->
  Draft(v+1)`; the previous version is retained under history. Stale versions return
  `VersionConflict`, and transitions out of order return `InvalidState`.
- Bounds: `MAX_LINES = 16`, `MAX_APPROVALS = 8`, `MAX_VERSIONS = 8`. Arithmetic uses
  `checked_mul` and `checked_add`; all persistent keys extend TTL on read/write.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`WrongTenant=4`, `NotFound=5`, `InvalidExpiry=6`, `InvalidAmount=7`, `EmptyLines=8`,
`TooManyLines=9`, `InvalidThreshold=10`, `VersionConflict=11`, `AlreadyExists=12`,
`RequestConflict=13`, `AlreadyApproved=14`, `InvalidState=15`, `Expired=16`, `Overflow=17`,
`CapacityExceeded=18`. Existing codes are never renumbered; new codes append.

## Events

| Symbol | Data |
| --- | --- |
| `CBPROLG` / `CBPRVRK` | `(tenant_id, account, role)` |
| `CBPPLN` | `(tenant_id, project_id, version, total_amount)` |
| `CBPSUB` | `(tenant_id, project_id, expected_version)` |
| `CBPAPP` | `(tenant_id, project_id, version, approval_count)` |

Every payload contains only IDs, enums, versions, or amounts.

## Privacy boundaries

Budgets can reveal sensitive financial planning, so the service stores only hashes of categories,
justifications, and funding sources. Amounts and aggregate totals are public by design because they
are needed for reservation and escrow reconciliation. Address values are public Stellar accounts
and must not be derived from private identity data. Events never carry notes, names, reasons, or
document contents.

## Dependencies

- `capstone-identifiers` supplies the public `project_id`.
- Charter acceptance (CAPSTONE-050) supplies the accepted charter context for the approval flow.
- CAPSTONE-083 consumes an approved plan hash; CAPSTONE-084 consumes plan-derived intent hashes.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Roll out behind the capstone funding feature flag on testnet first, with the backend dual-writing
plan records to its database and the contract for one cohort. Promote to authoritative only after
reconciliation of totals and approvals. Rollback stops new calls to the contract; existing records
remain readable and contain no private content to purge. Version history is append-only, so a
later redeployment can resume with the last stored version.

## Other-repository work

- Backend: canonicalize and hash budget lines/justifications, manage request ids, sync tenant
  roles, expose reconciliation and audit export.
- Frontend: budget editor, approval queue, version diff, and status display using only hashes for
  private documents.
- Indexer/ops: monitor `CBPPLN`/`CBPAPP`, alert on `RequestConflict`, and schedule TTL renewal for
  active projects.
