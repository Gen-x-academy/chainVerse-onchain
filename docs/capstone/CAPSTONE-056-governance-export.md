# CAPSTONE-056: Export Project Governance Records

**Track:** Governance | **Component:** on-chain | **Issue:** #1404 | **Contract:** `contracts/capstone-governance-export`

## Scope

Provide a participant-safe export manifest for governance records. The contract registers a project
with an accepted-terms hash, records bounded snapshot commitments for `AcceptedTerms`, `Amendments`,
`Approvals`, `Membership`, `MilestoneSummary`, and `FinancialSummary`, and issues short-lived export
grants. Grants are rate-limited per requester and recorded in an audit log. The contract never stores
or emits the exported documents themselves.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `ProjectRecord`: tenant, project, accepted-terms hash, registration time, snapshot ids | Charter, amendments, minutes, membership roster, financial detail |
| `SnapshotRef`: section enum, bounded hash, recorded time | Canonical export payload for that section |
| `ExportGrant`: grant id, tenant, project, requester, expiry, state | Download token, file payload, identity verification |
| `ExportQuota`, `AuditEntry`, role grants | Notification content and audit UI rendering |

All section hashes are SHA-256 commitments over canonical off-chain JSON. No document content,
filename, URI, memo, or private reference is written to the ledger.

## Authorization

- `initialize` is one-time, requires the administrator's `require_auth`, and binds the export policy
  (window, quota, grant TTL) which is validated at initialization.
- The administrator grants and revokes tenant/project-scoped `Participant`, `Coordinator`, and
  `Registrar` roles.
- `register_project` and `record_snapshot` require the `Registrar` role for the exact tenant/project.
- `request_export` and `get_export` require the requester's own `require_auth` plus a matching active
  role; a missing role, revoked role, or other tenant returns `NotAuthorized`/`NotRegistrar`.
- Deny is the default: unknown projects, snapshots, or grants fail closed.

## Idempotency and versioning

- Replaying a registration or snapshot with identical hashes returns the stored record with no new
  event; a conflicting payload returns `ProjectAlreadyRegistered` or `SnapshotAlreadyExists`.
- An export request reuses an active, unexpired grant for the same requester and manifest; a new
  request after expiry is allowed. Concurrent retries cannot create a second grant.
- Rate limits use `ExportQuota` keyed by requester and a fixed window; exceeding it returns
  `ExportRateLimited`. Downloading the same grant twice is idempotent and emits one `GVEXDL` event.
- Bounds: `MAX_SNAPSHOTS = 64`, `MAX_AUDIT_ENTRIES = 96`, window 60..2,592,000 seconds, grant TTL
  60..604,800 seconds.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAdmin=3`,
`NotRegistrar=4`, `NotAuthorized=5`, `ProjectNotFound=6`, `ProjectAlreadyRegistered=7`,
`ZeroHash=8`, `SnapshotNotFound=9`, `SnapshotAlreadyExists=10`, `SnapshotLimitReached=11`,
`ExportRateLimited=12`, `GrantNotFound=13`, `GrantAlreadyExists=14`, `GrantExpired=15`,
`InvalidPolicy=16`, `AuditLimitReached=17`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `GVROLE` | `(tenant, project, account, role, active)` |
| `GVREG` | `(tenant, project, terms_hash)` |
| `GVSNAP` | `(tenant, project, section, hash)` |
| `GVEXREQ` | `(tenant, project, grant_id, requester, expires_at)` |
| `GVEXDL` | `(grant_id, requester)` |

Payloads contain only IDs, enums, hashes, and timestamps.

## Privacy boundaries

Governance exports can contain personal and financial data, so only commitments and section metadata
are on-chain. The export grant authorizes retrieval from the off-chain service; the ledger never
holds the file, its contents, or a resolvable reference. Audit entries record the actor and grant id
only, and requester addresses are public Stellar accounts that must not be derived from private
identity data.

## Dependencies

- CAPSTONE-050/052/055 supply accepted charter, amendment, and conduct attestation hashes.
- CAPSTONE-054 supplies closure state; CAPSTONE-060 and CAPSTONE-088 supply milestone and financial
  summary hashes.
- CAPSTONE-071 source references supply canonical hashing rules.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the governance-export flag on testnet with one cohort, dual-writing manifests to the
service database and the contract. Reconcile snapshot hashes and grant counts before promoting.
Rollback stops new grants while existing unexpired grants remain readable; all stored values are
hashes and IDs, so no private content requires purging.

## Other-repository work

- Backend: canonicalize and hash each section, manage grant tokens and identities, enforce the
  download side of rate limits, and produce the participant-safe export file.
- Frontend: export request, grant status, and download UI showing only section metadata.
- Indexer/ops: monitor `GVEXREQ`/`GVEXDL`, alert on `ExportRateLimited`, and renew TTLs for active
  projects and snapshots.
