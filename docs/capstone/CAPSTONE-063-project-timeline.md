# CAPSTONE-063: Build Project Timeline and Roadmap

**Track:** Planning | **Component:** on-chain | **Issue:** #1411 | **Status:** implemented
**Audit baseline:** `main` @ `8713c23` (2026-11-22).
**Contract:** `contracts/capstone-project-timeline`

## Scope

Provide a stable, permission-aware, append-only timeline anchor for objectives, tasks, milestones,
decisions, reviews, funding, and charter changes. Each entry commits to a record that lives in its
source contract or service. The chain fixes the order and visibility scope; the content stays off-chain.

## On-chain stores vs off-chain content

| On-chain store | Contents |
| --- | --- |
| `Admin` (instance) | One-time deployment administrator. |
| `Role(account, tenant, project, role)` | `RoleGrant { active, expires_at }` for `Manager`, `Publisher`, `Viewer`. |
| `Request(tenant, project, request_id)` | Idempotency record: input commitment and recorded sequence. |
| `Head(tenant, project)` | Highest assigned sequence number. |
| `Entry(tenant, project, seq)` | `TimelineEntry`: id, sequence, kind, `ref_hash`, `redaction_hash`, `occurred_at`, visibility, state, `recorded_at`. |
| `EntrySeq(tenant, project, entry_id)` | Lookup from entry id to sequence; also the duplicate guard. |

Off-chain: titles, descriptions, assignees, dependency drawings, and the records `ref_hash` points to.

## Stable ordering

Sequence numbers are assigned in strictly increasing order by the chain and never change. Order is
insertion order, not `occurred_at` order, so a later-dated plan item appended first keeps its slot.
Clients that want a chronological roadmap sort by `occurred_at` off-chain; the sequence is the stable
tiebreaker and cursor. A cursor (`next_seq`) returned by `list_entries` stays valid forever.

## Authorization

| Entry point | Manager | Publisher | Viewer |
| --- | --- | --- | --- |
| `append_entry` | yes | yes | no |
| `redact_entry` | yes | no | no |
| `get_entry`, `list_entries` (team entries) | yes | yes | yes |
| Same reads for `Restricted` entries | yes | no | no |

Deny by default and scoped to the exact tenant and project. A restricted entry a caller may not see
is reported as `NotFound` from `get_entry` and is skipped by `list_entries` without shifting the
cursor, so paging never reveals gaps beyond the consumed sequence slot.

## Lifecycle

`Active` to `Redacted`, once. Redaction replaces `ref_hash` with the zero hash and stores a
`redaction_hash` commitment so a mistaken or withdrawn pointer stops resolving, while the entry keeps
its sequence slot. A second redaction under a new request returns `InvalidTransition`. Entries are
otherwise immutable; corrections are new entries.

## Idempotency and validation

Mutations supply `request_id`; the stored commitment binds domain, tenant, project, request id,
actor, and all fields. Replays return the recorded sequence with no write and no event, even after
later appends. Reusing a request id with a different payload or actor returns `RequestConflict`.
A second append of an existing `entry_id` under a new request returns `AlreadyExists`, so concurrent
publishers cannot duplicate an entry. `entry_id` and `ref_hash` must be non-zero, `occurred_at` must
be non-zero and at most five years ahead of the ledger clock.

## Bounds

256 entries per project (`LimitReached`), pages of at most 32 (`InvalidPage`). `from_seq` starts at
1 and may equal `head + 1`, which returns an empty page. TTLs extend on write and read.

## Errors

Append-only codes: `1 AlreadyInitialized`, `2 NotInitialized`, `3 Unauthorized`, `4 RequestConflict`,
`5 StaleVersion`, `6 NotFound`, `7 AlreadyExists`, `8 InvalidInput`, `9 InvalidTransition`,
`10 LimitReached`, `11 InvalidPage`, `12 ArithmeticError`, `13 RoleExpired`. `StaleVersion` is
reserved for shared-code parity; entries are immutable so no timeline call can be stale.

## Events

| Topic | Payload |
| --- | --- |
| `TLN_INIT` | `(admin)` |
| `TLN_ROLE` | `(tenant, project, account, role, active, expires_at)` |
| `TLN_ADD` | `(tenant, project, entry_id, seq, kind, occurred_at, visibility)` |
| `TLN_RED` | `(tenant, project, entry_id, seq)` |

No titles, descriptions, or reference payloads are broadcast.

## Privacy boundaries and off-chain boundary

`ref_hash` must be a salted commitment, never a hash of guessable text. The contract does not verify
that a reference resolves to a real record in another contract: it records that an authorized
publisher asserted it. Publish from the service after observing the source contract's own event,
so the timeline mirrors verified state. `Restricted` scoping limits the contract API only; ledger
storage is public, so sensitive content must remain behind its hash.

## Dependencies

Entry kinds correspond to `capstone-objectives` (CAPSTONE-057), `capstone-project-milestones`
(CAPSTONE-060), `capstone-project-decisions` (CAPSTONE-070), the review contracts (CAPSTONE-073 to
077), funding contracts (CAPSTONE-081 to 086), and charter changes (CAPSTONE-052). Indexers join
`ref_hash` to those sources.

## Rollout and rollback

Deploy, initialize, grant roles, then enable timeline publishing in the service. Historical items can
be appended in any order and keep their insertion sequence. Rollback: stop appending; reads remain
valid. Redaction is the only retraction mechanism and is itself auditable.

## Upgrade compatibility

Error codes, enum discriminants (`Role`, `EntryKind`, `Visibility`, `EntryState`) and `DataKey`
variants are append-only and snapshot-tested. A stored-struct field change is not decode-compatible
and must ship as a new `DataKey` variant with a migration. A test writes `Entry`, `EntrySeq`, `Head`,
`Role`, and `Request` records directly to prove a newer build continues an older sequence.

## Test coverage map

Every entry kind and stable sequence; insertion order versus timestamp order; authorization tree;
tenant and project isolation; replay and conflict; duplicate ids; restricted visibility with cursor
stability; single redaction and slot retention; input validation including the horizon boundary;
role expiry; 256-entry and paging bounds; event schemas; error and enum snapshots; legacy decode;
CPU and memory budgets for append, redact, and a full-page read.

## Corresponding work in other repositories (CAPSTONE-063)

- **Backend/timeline-service:** aggregate source events, publish entries with salted commitments, serve the chronological roadmap.
- **Frontend:** roadmap and timeline views, filters by kind, restricted-entry handling.
- **Mobile:** read-only timeline through the service.
- **Docs/indexer:** projection schema keyed by `(tenant, project, seq)`, cursor semantics, redaction handling.
