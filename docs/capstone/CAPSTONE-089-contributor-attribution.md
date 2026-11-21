# CAPSTONE-089: Contributor Attribution

**Track:** Publishing | **Component:** on-chain | **Issue:** #1437 | **Crate:** `contracts/capstone-contributor-attribution`

## Scope

Register versioned contributor attribution using a recognized taxonomy. The contract stores declared role codes, bounded contribution commitments, independent approvals, disputes with resolution, and output linkage. It does not store names, biographies, CVs, authorship prose, or the outputs themselves.

## On-chain stores vs off-chain content

| Store | Key shape | Contents |
| --- | --- | --- |
| `Admin` | instance | platform bootstrap address |
| `RoleIndex` | `(tenant_id, project_id)` | bounded role grants |
| `RequestReceipt` / `RequestIndex` | `(tenant_id, project_id, request_id)` | cached result id, operation code, version |
| `Policy` | `(tenant_id, project_id)` | required approvals, max records, version |
| `AttributionRecord` / `RecordIndex` | `(tenant_id, project_id, record_id)` | contributor key, taxonomy code, contribution/dispute/output hashes, state, approvals, version |
| `Approval` | `(tenant_id, project_id, record_id, reviewer)` | single-use approval marker |

Every write extends the 1-to-2-year TTL window.

| Off-chain | On-chain |
| --- | --- |
| Contributor identity details and ORCID profile | public `Address` in the scoped record only |
| Contribution statement and evidence files | `contribution_hash` |
| Dispute text and mediation notes | `dispute_hash` / `resolution_hash` |
| Paper, dataset, or release artifact | `output_hash` |

## Authorization

Deny by default. `initialize` is one-time and stores the platform admin, which alone can call `grant_role`. All roles are scoped to `(tenant_id, project_id)` and every mutation calls `require_auth()` plus the stored-role check. There is no global fallback, so cross-tenant roles do not authorize attribution changes.

| Role | Allowed operations |
| --- | --- |
| `Coordinator` | configure policy, resolve dispute, link output |
| `Contributor` | declare contribution |
| `Reviewer` | approve or dispute a record that is not their own |

A contributor cannot approve their own record: `SelfApprovalNotAllowed`. Each reviewer can approve a record once; duplicate approvals return `AlreadyApproved` while the record is still in `Declared`.

## Idempotency and versioning

All mutations take `request_id` and `expected_version`. Receipts cache `(result_id, op, version)`; identical retries return the cached record without a second event. A reused request id with a different operation or version returns `RequestConflict`; a stale live write returns `StaleVersion`. Approval markers and version checks prevent double counting and out-of-order transitions.

Taxonomy codes are the eight fixed variants: `Conceptualization`, `Methodology`, `Software`, `DataCuration`, `Validation`, `Writing`, `Supervision`, `FundingAcquisition`. Unsupported values fail before state changes. All counters use checked arithmetic and record indices are capped by policy and by the protocol ceiling of 16.

## Errors

Append-only codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`, `NotFound=4`, `AlreadyExists=5`, `InvalidRoleCode=6`, `InvalidPolicy=7`, `InvalidState=8`, `StaleVersion=9`, `RequestConflict=10`, `SelfApprovalNotAllowed=11`, `ArithmeticOverflow=12`, `IndexFull=13`, `RoleLimitReached=14`, `RoleAlreadyGranted=15`, `InvalidHash=16`, `AlreadyApproved=17`.

## Events

| Topic | Data |
| --- | --- |
| `ROLEGRT` | `(tenant_id, project_id, role_code)` |
| `ATTRCFG` | `(tenant_id, project_id, required_approvals, max_records)` |
| `ATTRDECL` | `(record_id, taxonomy_code, contribution_hash)` |
| `ATTRAPPR` | `(record_id, approvals, state_code)` |
| `ATTRDSP` | `(record_id, dispute_hash, state_code)` |
| `ATTRRES` | `(record_id, resolution_hash, state_code)` |
| `ATTRLINK` | `(record_id, output_hash, state_code)` |

No address, name, title, URL, or prose is emitted.

## Privacy boundaries

The record's contributor address is a public key used for authorization and audit, not a real-world identity. Taxonomy codes are deliberately coarse. Dispute existence is public through state and events, but its substance is only a hash. Tenants should avoid putting identity data into hash preimages; the off-chain store must enforce access control.

## Dependencies and rollout

Depends on `capstone-identifiers`, the tenant role registry, and the charter/guardian-consent gates enforced by the calling application. Consumed by publishing backends, DOI/repository metadata services, and audit exports.

Rollout: deploy, grant roles, configure policies with a single reviewer, then raise approval requirements after the dispute queue is monitored. Rollback: stop new declarations at the API and keep dispute resolution available; stored records remain queryable and cannot be duplicated. Version `1` is stable; additive fields preserve existing keys, while enum reordering is a breaking change.

## Other repositories

- Backend: identity directory, contribution statement store, reviewer assignment, dispute workflow, and output-hash registration.
- Frontend: declaration wizard with privacy notice, reviewer queue, dispute form, and publication linkage view.
- Publishing pipeline: resolves `output_hash` to the artifact metadata and emits provenance statements without reading private files.

## Tests

Deterministic tests cover taxonomy use, zero-hash rejection, self-approval and duplicate approval, two-reviewer policies, dispute uphold/overturn and output-link gating, replay/conflict/version errors, cross-tenant denial, policy record bounds, event topics and typed data, timestamps, budget ceilings, one-time initialization, and the stable version constant.
