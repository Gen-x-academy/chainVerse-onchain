# CAPSTONE-040: Proposal History and Status Timeline

**Track:** Proposals | **Component:** on-chain | **Status:** specified (not yet implemented)
**Issue:** #1388 | **Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against `main` @ `4471755`
**Depends on:** CAPSTONE-038, CAPSTONE-039

Goal: present permission-filtered revisions, submissions, assignments,
decisions, requests and deadlines in stable chronological order.

## Design decision

The **timeline view is assembled off-chain**. The chain provides an
append-only, ordered, tamper-evident audit trail that the view is built from
and checked against. Storing a full rendered timeline on-chain would be
unbounded and would leak reviewer and decision detail.

## On-chain scope

`contracts/capstone-proposals` records a bounded `TimelineEntry` for every
state-changing call:

| Field | Type | Notes |
| --- | --- | --- |
| `seq` | `u32` | Per-proposal monotonic counter, gap-free |
| `kind` | `EntryKind` | `Submitted`, `Withdrawn`, `Resubmitted`, `Assigned`, `ChangesRequested`, `Decided`, `DeadlineSet` |
| `actor_role` | `Role` | `Submitter`, `Reviewer`, `Coordinator`. A role, never an address, for reviewer entries |
| `revision` | `u32` | Proposal revision the entry applies to |
| `ref_hash` | `Option<BytesN<32>>` | Commitment to off-chain detail (decision letter, request text) |
| `at` | `u64` | Ledger timestamp |
| `visibility` | `Visibility` | `Submitter`, `Reviewers`, `Coordinators` (minimum audience) |
| `prev_hash` | `BytesN<32>` | `sha256(prev entry)`, giving a hash chain for tamper evidence |

Read entry points:

```text
timeline_len(tenant_id, proposal_id) -> u32
timeline_page(viewer, tenant_id, proposal_id, from_seq, limit /* <= 50 */) -> Vec<TimelineEntry>
timeline_head(tenant_id, proposal_id) -> BytesN<32>   // latest chain hash
```

`timeline_page` calls `viewer.require_auth()` and returns only the entries
whose `visibility` the viewer's role satisfies. Others are omitted, not
redacted in place, and `seq` gaps reveal only that hidden entries exist.
Unknown or foreign-tenant viewers get `Unauthorized` (deny by default).

## Ordering guarantees

- The order is total and stable: `(seq)` per proposal, assigned in the same
  invocation as the state change. Two entries in the same ledger keep their
  invocation order.
- Revisions created by resubmission (CAPSTONE-039) link through `prior`.
  Clients walk the chain to build a cross-revision timeline ordered by
  `(at, proposal_id revision, seq)`.
- Entries are append-only. No update or delete entry point exists.
- Replayed intents (CAPSTONE-038/039) append no entry.

## Storage bounds

- Each entry has a fixed size (no `String`/`Bytes`).
- Per-proposal cap is `MAX_TIMELINE = 256` entries. Mutations that would exceed
  it fail with `TimelineFull`, which surfaces a coordinator action.
- Entries use persistent storage with TTL extended on write. Archival beyond
  TTL is handled by the off-chain indexer (see
  `contracts/docs/indexer-projection-schema.md`).

## Errors (appended)

| Code | Name |
| --- | --- |
| 19 | `TimelineFull` |
| 20 | `InvalidPageRange` (`limit == 0`, `limit > 50`, or `from_seq > len`) |

## Events

Each appended entry emits `("CAPPROP", "tl")` → `(tenant_id, proposal_id, seq, kind, revision)`.
Indexers project the full timeline from these events. `ref_hash`, visibility
and roles are fetched through permissioned reads, not broadcast.

## Off-chain boundary

- Decision letters, reviewer comments, change-request text, scores and
  reviewer identities live in the proposals/review services, referenced only
  by `ref_hash`.
- The UI timeline merges on-chain entries with off-chain detail the viewer may
  see, and verifies each entry against `prev_hash` / `timeline_head`.
- Deadlines shown in the UI come from call configuration. On-chain
  `DeadlineSet` entries record only that the deadline changed and its new
  timestamp.

## Test plan

- Permission filtering: each role sees exactly its allowed entry set; a foreign tenant gets `Unauthorized`.
- Ordering: multiple mutations in one ledger keep `seq` order; the cross-revision walk is deterministic.
- Hash chain: recomputing `prev_hash` over all entries matches `timeline_head`.
- Replay: an idempotent replay adds no entry.
- Bounds: the 256th entry passes and the 257th gets `TimelineFull`; page limits are enforced.
- Event schema snapshot, error codes, page-read budget at `limit = 50`, upgrade decode of v1 entries.

## Rollout / rollback

Timeline recording ships with the first `capstone-proposals` release, so no
backfill is needed. If shipped later, proposals created before then start their
timeline at the upgrade (documented gap, `seq = 0` marker `Migrated`).
Rollback: the read entry points can be removed in an upgrade without touching
stored entries.

## Cross-repository work (CAPSTONE-040)

- **Backend:** indexer projection, merge of on-chain entries with off-chain detail, hash-chain verification.
- **Frontend:** timeline component with role-filtered entries and revision grouping.
