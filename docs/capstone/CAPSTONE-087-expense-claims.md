# CAPSTONE-087: Expense Claims

**Track:** Funding | **Component:** on-chain | **Issue:** #1435 | **Crate:** `contracts/capstone-expense-claims`

## Scope

Manage expense reimbursement claims with constrained evidence commitments, tenant policy validation, reviewer separation, approval or rejection, and a payout-linkage attestation. The contract never transfers funds and never stores receipts, descriptions, or personal data.

## On-chain stores vs off-chain content

| Store | Key shape | Contents |
| --- | --- | --- |
| `Admin` | instance | platform bootstrap address |
| `RoleIndex` | `(tenant_id, project_id)` | bounded role grants |
| `RequestReceipt` / `RequestIndex` | `(tenant_id, project_id, request_id)` | cached result id, operation code, version |
| `Policy` | `(tenant_id, project_id)` | max amount, max evidence items, required approvals, version |
| `Claim` / `ClaimIndex` | `(tenant_id, project_id, claim_id)` | category, amount, evidence hash vector, state, approvals, payout hash, version |
| `Approval` | `(tenant_id, project_id, claim_id, reviewer)` | single-use approval marker |

Writes extend TTL to the standard 1-to-2-year window.

| Off-chain | On-chain |
| --- | --- |
| Receipts, invoices, and merchant names | bounded `evidence_hashes` vector |
| Reimbursement narrative | coarse `Category` enum |
| Reviewer comments and rejection rationale | `reason_code` 1..=4 |
| Payout instrument and transaction memo | `payout_hash` commitment only |

## Authorization

Deny by default. `initialize` is one-time and stores the platform admin, which is the only account allowed to call `grant_role`. Roles are scoped by `(tenant_id, project_id)`; every mutation calls `require_auth()` and then checks the stored role under that exact scope. There is no global role fallback for claims, so cross-tenant callers fail with `Unauthorized`.

| Role | Allowed operations |
| --- | --- |
| `Coordinator` | configure policy |
| `Claimant` | submit claim |
| `Reviewer` | approve or reject a claim that is not their own |
| `Finance` | link payout after approval |

Reviewer separation is enforced twice: the claim stores the submitter, and `approve_claim` / `reject_claim` return `SelfReviewNotAllowed` when the reviewer is that submitter. Each reviewer can approve a claim once; the approval marker is destroyed by TTL but duplicates are also bounded by the request receipt window.

## Idempotency and versioning

Each entry point accepts `request_id` and `expected_version`. A receipt stores `(result_id, op, version)` so retries with the same inputs return the cached claim or policy without a second event. Reusing a request id for another operation or version returns `RequestConflict`; advancing state with a stale version returns `StaleVersion`. Approval markers and request ids prevent double counting and duplicate side effects.

Evidence vectors are bounded by both the tenant policy (`max_evidence_items <= 8`) and the protocol ceiling; claim indices are capped at 16 per tenant/project. All counters use checked arithmetic.

## Errors

Append-only codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`, `NotFound=4`, `AlreadyExists=5`, `InvalidAmount=6`, `InvalidCategory=7`, `InvalidEvidence=8`, `InvalidPolicy=9`, `InvalidState=10`, `StaleVersion=11`, `RequestConflict=12`, `SelfReviewNotAllowed=13`, `ArithmeticOverflow=14`, `IndexFull=15`, `RoleLimitReached=16`, `RoleAlreadyGranted=17`, `InvalidReason=18`.

## Events

| Topic | Data |
| --- | --- |
| `ROLEGRT` | `(tenant_id, project_id, role_code)` |
| `EXPCFG` | `(tenant_id, project_id, max_amount, required_approvals)` |
| `EXPSUB` | `(claim_id, amount, category_code)` |
| `EXPAPPR` | `(claim_id, approvals, state_code)` |
| `EXPREJ` | `(claim_id, reason_code, state_code)` |
| `EXCPAID` | `(claim_id, payout_hash, amount)` |

Events carry only IDs, enums, hashes, and amounts. No address, description, or document URI appears on-chain.

## Privacy boundaries

Amounts and coarse categories are public because they are needed for audit. Evidence contents and reviewer identity reasoning stay off-chain. A tenant that wants to hide categories can map every internal expense type to `Other` before generating the on-chain commitment.

## Dependencies and rollout

Depends on `capstone-identifiers`, the tenant role registry, and the milestone/funding policy set by the coordinator. Consumed by the payout orchestration service, which links `EXCPAID` to an escrow transfer idempotently by `(project_id, claim_id)`.

Rollout: deploy, configure per-tenant policies with single approval, enable the payout linker only after its consumer deduplicates by claim id, then raise approval requirements. Rollback: stop the payout linker and freeze new submissions at the API; existing claims and approvals remain readable and cannot be duplicated. Version `1` is stable; additive fields preserve existing storage keys.

## Other repositories

- Backend: evidence-object store with hash API, policy administration, approval queue, payout orchestration, and rejection-code mapping.
- Frontend: claim submission form with client-side privacy warning, reviewer queue, status timeline, and payout receipt viewer.
- Finance operations: reconciliation rules that match `EXCPAID` events to external bank records without exposing merchant text on-chain.

## Tests

Deterministic tests cover policy validation, evidence bounds, self-approval, two-reviewer separation, replay/conflict/version errors, invalid state transitions, cross-tenant denial, bounded claim index, event topics and typed data, timestamps, budget ceilings, one-time initialization, and the stable version constant.
