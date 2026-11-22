# CAPSTONE-070: Record Project Decisions

**Track:** Workspace | **Component:** on-chain | **Issue:** #1418 | **Status:** implemented
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree.
**Contract:** `contracts/capstone-project-decisions`

## Scope

Record the existence, lifecycle, and public audit metadata of a capstone project
decision. The decision statement, context, alternatives, rationale, evidence
documents, and approver identities stay in the off-chain decision service. The
chain stores only high-entropy commitments, state transitions, counters, and
timestamps needed to verify that a displayed decision was approved and became
effective.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin` (instance) | One-time deployment administrator. |
| `Role(tenant_id, account, role)`, `RoleCount(tenant_id)` | Tenant-scoped stored roles and bounded grant count (32). |
| `Version(tenant_id, project_id)` | Optimistic-concurrency counter for all project mutations. |
| `Request(tenant_id, project_id, request_id)` | Idempotency record: input commitment, result id, resulting version. |
| `Decision(tenant_id, project_id, decision_id)` | Decision record: content hash, evidence hash, effective date, required approvals, state, supersession hash, approval addresses, creation time, version. |
| `DecisionIndex(tenant_id, project_id)` | Append-only list of decision ids, capped at 64. |
| `Approval(tenant_id, project_id, decision_id, approver)` | One-shot approval marker per approver. |

Off-chain: the statement, context, alternatives, the actual evidence payload,
approver names and private comments, notification text, and rendered decision
letters. Clients pass a fresh salted SHA-256 commitment as `content_hash` and
`evidence_hash`; low-entropy text must never be hashed directly on-chain.

## Authorization

`initialize` authenticates the administrator once. The administrator grants or
revokes tenant-scoped `Coordinator`, `Approver`, and `Viewer` roles; every
mutation calls `require_auth()` and then checks the stored role. `Coordinator`
proposes, activates, and supersedes; `Approver` approves once; reads require an
active tenant role or the administrator. There is no implicit global role: an
address with a role in tenant A gets `Unauthorized` in tenant B.

## Idempotency

Every state-changing call supplies `request_id` and `expected_version`. A salted
domain-separated SHA-256 over `(tenant_id, project_id, request_id,
expected_version, call-specific fields)` is stored with the request. Replaying
the same request returns the recorded version without writing state or emitting
an event, even after later calls advanced the version. Reusing `request_id`
with a different payload returns `RequestConflict`. `expected_version` must
equal the stored project version for new requests; stale calls get
`VersionConflict`. Versions, counters, and page ends use checked arithmetic.
Administrative role grants/revokes are boolean writes on deterministic keys and
are naturally idempotent, so they do not consume a `request_id`.

## Errors

Append-only `ContractError` codes: `1 NotInitialized`, `2 AlreadyInitialized`,
`3 Unauthorized`, `4 VersionConflict`, `5 VersionOverflow`, `6 RequestConflict`,
`7 NotFound`, `8 AlreadyExists`, `9 InvalidInput`, `10 InvalidState`,
`11 AlreadyApproved`, `12 ThresholdNotMet`, `13 NotEffective`,
`14 CollectionFull`, `15 InvalidPage`, `16 InvalidSupersession`.

## Events

Topics are `symbol_short!` values; payloads contain only ids, hashes, enums, and
counters: `DCINIT(admin)`, `ROLEGR(tenant_id, account, role)`,
`ROLERV(tenant_id, account, role)`, `DCPROP(tenant_id, project_id, decision_id,
version)`, `DCAPPR(tenant_id, project_id, decision_id, approval_count, state)`,
`DCACT(tenant_id, project_id, decision_id, state)`, and
`DCSUP(tenant_id, project_id, prior_id, new_id)`. No statement, evidence, or
comment text is broadcast. Replays emit nothing.

## Privacy boundaries

Addresses are pseudonymous identifiers, not names. Approval addresses are
stored to enforce one-shot approval; if a deployment requires stronger
pseudonymity, map approvers to per-tenant role commitments off-chain and grant
a shared `Approver` key to the service. Hashes must be salted commitments, not
guessable text. Persistent entries and request records are TTL-extended on read
and write; the indexer, not the chain, is responsible for long-range history.

## Dependencies

Depends on `capstone-identifiers` for stable project ids, CAPSTONE-038/039 for
proposal submission boundaries, and CAPSTONE-040 for the timeline that links
decision events. The off-chain decision service owns document storage,
approver directory data, and digest generation. Indexers consume the events to
project a decision history.

## Rollout / rollback

Ship behind the workspace feature flag, deploy, initialize with a dedicated
 administrator key, grant tenant roles, then enable the UI. Existing decisions
 can be backfilled by proposing with hashed commitments and a `Migration` marker
 in the off-chain record; the chain has no backfill shortcut. Rollback: stop
 invoking mutating entry points and keep decoded storage readable by an older
 client; the error enum and record layouts are append-compatible. Removing the
 contract is a deliberate data-retention action, not a routine rollback.

## Corresponding work in other repositories (CAPSTONE-070)

- **Backend/decision-service:** store statement, context, alternatives,
  evidence, approver directory; generate salted commitments; verify events.
- **Frontend:** decision editor, approver workflow, effective-date display,
  supersedingDecision view; never submit raw text to the chain.
- **Mobile:** read-only decision detail and approval actions through the
  service.
- **Docs/indexer:** projection schema, retention policy, and digest guidance.
