# CAPSTONE-068: Project Resource Library

**Issue:** #1416 | **Component:** on-chain | **Contract:** `contracts/capstone-resource-library`
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the worktree baseline
**Depends on:** tenant/project identity from CAPSTONE-038 and project roles from CAPSTONE-041/050

## Scope

On-chain registry for approved project resources: kind (file, dataset, link, citation), content and
integrity commitments, license enum plus license reference hash, access policy, version history,
approval/deprecation lifecycle, retention, and bounded page reads. Files, datasets, URLs, and
citation text remain off-chain.

## On-chain stores vs off-chain content

| On-chain | Off-chain |
| --- | --- |
| `tenant_id`, `project_id`, `resource_id`, request/intent IDs | Actual files, datasets, target URLs, citation text |
| `content_hash`, `integrity_hash`, `license_ref_hash`, version hashes | Object storage, checksum manifests, license documents |
| `ResourceKind`, `LicenseKind`, `AccessPolicy`, `ResourceState` | Human-readable titles, descriptions, tags |
| `size_bytes`, version counters, `expires_at` | Download metrics, caching, CDN configuration |

No free text or URL is stored. `integrity_hash` must be non-zero and is committed together with
`content_hash` and `size_bytes` for tamper checking.

## Authorization

`initialize` requires admin auth and stores `Admin`. `grant_role` requires stored `Admin` and
assigns `Member`, `Reviewer`, `Curator`, or `Admin`. All entry points call `require_auth()` and
check the stored role for the tenant/project:

- Reads and `can_access`: `Member` or higher.
- Propose resource, add version, set policy: `Curator` or higher.
- Approve and deprecate: `Admin` only.

`can_access` denies unless the resource is `Approved` and the stored `AccessPolicy` allows the
viewer: `OwnerOnly` compares the stored owner, `ProjectMembers` allows any member, `Reviewers`
requires `Reviewer` or higher. Unknown roles, proposed/deprecated resources, and expired records
deny by default.

## Idempotency

Mutations take `request_id` (or `intent_id` for lifecycle/policy actions) and `expected_version`.
Request records are scoped by `(tenant_id, project_id, request_id)` and bind actor hash, resource
ID, kind/license/policy enums, content and integrity hashes, license reference, size, retention,
action enum, and expected version. Exact replay returns the stored result with no second event;
payload drift is `RequestConflict`; stale versions are `StaleVersion`.

Bounds: 32 members, 32 resources, 16 versions per resource, page size at most 50, retention
`1..=6_220_800` seconds. All counters and expiries use checked arithmetic. TTL is extended on every
read/write.

## Errors

Append-only `ContractError` codes 1..19: `NotInitialized`, `AlreadyInitialized`,
`NotAuthorized`, `NotFound`, `StaleVersion`, `RequestConflict`, `AlreadyExists`,
`InvalidTransition`, `ResourcesFull`, `VersionsFull`, `MembersFull`, `Expired`,
`InvalidRetention`, `TimestampOverflow`, `VersionOverflow`, `InvalidPage`, `UnsupportedSchema`,
`AccessDenied`, `InvalidIntegrity`. New codes append only.

## Events

Topics: `RSROLE`, `RSCP`, `RSCA`, `RSCD`, `RSCV`, `RSCPOL`. Event data contains resource IDs, action
enums, version amounts, and content hashes. No URL, title, address, or private license text is
emitted. Replays emit nothing.

## Privacy boundaries

The ledger reveals which opaque resources exist, their state, license class, access policy, and
integrity commitments. Restricted resources are still visible as metadata; access to content is
enforced in the off-chain store using the on-chain `can_access` decision. Sensitive dataset
contents, link targets, and citation text never reach the ledger or events.

## Dependencies

- `soroban-sdk` workspace dependency (22.x), `no_std`, `rlib`/`cdylib`.
- Tenant/project identity and roles (CAPSTONE-038/041/050).
- Off-chain object store, integrity manifest service, and license document store.
- Indexer for approved-resource projections and TTL renewal.

## Rollout / rollback

1. Deploy a fresh tenant/project registry; no automatic migration of legacy links.
2. Backend dual-writes commitments and validates integrity before proposing.
3. Frontend switches to on-chain policy checks for download authorization.
4. Rollback: freeze approvals, keep the off-chain policy engine authoritative, and continue serving
   already-approved resources. Records use `SCHEMA_VERSION = 1` for upgrade-safe decoding.

## Corresponding work in other repositories (CAPSTONE-068)

- **Frontend:** resource browser, upload/link forms, license display, policy badges.
- **Backend:** object storage, checksum generation, license metadata service, indexer.
- **Ops:** CDN access control wired to `can_access`, retention and archival jobs.
