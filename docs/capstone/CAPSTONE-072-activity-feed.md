# CAPSTONE-072: Generate Project Activity Feed

**Track:** Workspace | **Component:** on-chain | **Issue:** #1420 | **Status:** implemented
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the current worktree.
**Contract:** `contracts/capstone-activity-feed`

## Scope

Provide an append-only, permission-filtered, paginated activity stream for a
project. Each entry records a sequence number, event kind, actor role,
visibility class, commitment to off-chain detail, commitment to a deep link,
timestamp, and hash-chain link. Activity text, titles, comment bodies, and
actual destination URLs remain off-chain. Read state is a per-viewer cursor.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin`, `Role(tenant_id, account, role)`, `RoleCount(tenant_id)` | Admin plus tenant-scoped `Coordinator`, `Member`, `Observer` roles (32 grants per tenant). |
| `Version(tenant_id, project_id)`, `Request(...)` | Optimistic version and idempotency record. |
| `Index(tenant_id, project_id)` | Head sequence, head hash, retention seconds, version. |
| `Entry(tenant_id, project_id, slot)` | `FeedEntry` in a bounded 64-slot ring: seq, id, kind, actor role, visibility, ref hash, deep-link hash, timestamp, previous hash, entry hash. |
| `Read(tenant_id, project_id, viewer)` | Highest sequence acknowledged by a viewer. |

Off-chain: the rendered message, actor display name, avatar, deep-link target,
comment text, notification copy, and the data behind `ref_hash` and
`deep_link_hash`. Hashes must be salted commitments to off-chain records, not
hashes of raw text.

## Authorization

`initialize` authenticates the admin, who grants tenant-scoped roles. Appends
authenticate and require `Coordinator` or `Member`; only `Coordinator` may
publish `Coordinators`-visibility entries. Reads authenticate and require an
active tenant role or the admin. Visibility filtering is enforced on-chain:
`Public` is visible to all permitted viewers, `Members` to members and
coordinators, `Coordinators` only to coordinators/admin. Foreign-tenant viewers
are denied by default.

## Idempotency

Every append, retention change, and read-cursor update carries `request_id` and
`expected_version` plus a domain-separated input commitment. Replays return
the stored version without writing or emitting; payload reuse with the same
request id yields `RequestConflict`, and stale new requests yield
`VersionConflict`. Sequence arithmetic and retention cutoffs use checked
operations. Duplicate entry ids are rejected within the live ring.
Administrative role grants/revokes are boolean writes on deterministic keys
and are naturally idempotent, so they do not consume a `request_id`.

## Errors

Append-only `ContractError` codes: `1 NotInitialized`, `2 AlreadyInitialized`,
`3 Unauthorized`, `4 VersionConflict`, `5 VersionOverflow`, `6 RequestConflict`,
`7 NotFound`, `8 AlreadyExists`, `9 InvalidInput`, `10 InvalidPage`,
`11 InvalidSequence`, `12 InvalidVisibility`.

## Events

`CFAINIT(admin)`, `ROLEGR(tenant_id, account, role)`,
`ROLERV(tenant_id, account, role)`, `CFARET(tenant_id, project_id,
retention_seconds)`, `CFAENT(tenant_id, project_id, seq, kind, visibility)`, and
`CFAREAD(tenant_id, project_id, seq)`. Payloads are ids, enums, sequence
numbers, and retention amounts. No actor text, message, or URL is broadcast.

## Privacy boundaries

The actor is represented only by a role enum, never an address in the entry.
Observers see only public entries even though all entries share one chain;
hidden entries are omitted from pages rather than redacted in place. The 64-slot
ring bounds storage; rotation is deterministic and older entries are available
only from the off-chain indexer while TTL allows. Retention is a view filter:
old entries remain addressable by direct read until overwritten or expired, so
erasure must be performed off-chain by discarding salts and documents.

## Dependencies

Depends on `capstone-identifiers` for project ids and consumes events from
CAPSTONE-070 (decisions), CAPSTONE-071 (source refs), and CAPSTONE-073
(review queue actions), which are mirrored into feed entries by the off-chain
aggregator. The indexer supplies long-range history and search.

## Rollout / rollback

Deploy behind the workspace flag, initialize, grant roles, set retention, then
enable the feed UI. Backfill is an off-chain projection of historical events
into fresh on-chain entries with a `Migrated` marker; no historical text is
copied on-chain. Rollback: stop appends and serve cached/indexed pages; reads
and stored layouts remain compatible, and error codes are never renumbered.

## Corresponding work in other repositories (CAPSTONE-072)

- **Backend/aggregator:** build entries from authorized service events,
  generate salted commitments and deep links, maintain indexer projections.
- **Frontend:** role-filtered feed, pagination, unread cursor, deep-link
  resolution, retention-aware rendering.
- **Mobile:** compact feed cards and read acknowledgement through the service.
- **Docs/indexer:** projection schema, retention/erasure runbook, event-to-kind
  mapping.
