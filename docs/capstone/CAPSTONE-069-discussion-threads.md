# CAPSTONE-069: Project Discussion Threads

**Issue:** #1417 | **Component:** on-chain | **Contract:** `contracts/capstone-discussion-threads`
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the worktree baseline
**Depends on:** tenant/project identity from CAPSTONE-038 and roles from CAPSTONE-041/050

## Scope

On-chain primitive for scoped threaded conversations: thread creation, stable-ordered message
sequencing, parent references, mention and attachment commitments, resolution/reopen lifecycle,
moderation hooks, retention, and page reads. Message bodies, mention identities, and attachment
bytes stay off-chain.

## On-chain stores vs off-chain content

| On-chain | Off-chain |
| --- | --- |
| `tenant_id`, `project_id`, `thread_id`, `message_id`, request/intent IDs | Thread titles, message text, emoji/reactions |
| `sequence`, `parent_id`, `content_hash`, `mention_digest` | Mention display names, notification payloads |
| Attachment hash list, `Resolution`, `MessageModeration` enums | Attachment bytes, filenames, MIME types, URLs |
| `expires_at`, counts, timestamps | Search index, moderation dashboards, analytics text |

No free text is stored. `mention_digest` commits to a sorted set of mention commitments, and
attachment IDs are opaque hashes; neither reveals identities or filenames.

## Authorization

`initialize` requires admin auth and stores `Admin`. `grant_role` requires stored `Admin` and
assigns `Participant`, `Moderator`, or `Admin`. Every entry point calls `require_auth()` and checks
the stored role for the tenant/project:

- Reads and thread/message creation: `Participant` or higher.
- Resolve, reopen, moderate message: `Moderator` or higher.

Missing configuration or role rejects the call. Moderation hooks are separate from content reads,
and no role can read hidden message text from the ledger because no text is stored.

## Idempotency

Mutations take `request_id` (or `intent_id` for resolution/reopen/moderation) and
`expected_version`. Request records are bound to tenant/project/request ID and hash the actor,
thread/message IDs, parent ID, content hash, mention digest/count, attachment hashes, action enum,
resolution/moderation enum, and expected version. Exact replays return the original result with no
duplicate message or event; changed payloads return `RequestConflict`; stale versions return
`StaleVersion`.

Bounds: 32 members, 32 threads per project, 64 messages per thread, 8 mentions per message, 4
attachments per message, page size at most 50, retention `1..=6_220_800` seconds. Sequence and
message counters use `checked_add`. TTL is extended on all touched keys.

## Errors

Append-only `ContractError` codes 1..20: `NotInitialized`, `AlreadyInitialized`,
`NotAuthorized`, `NotFound`, `StaleVersion`, `RequestConflict`, `AlreadyExists`,
`InvalidThreadState`, `ThreadsFull`, `MessagesFull`, `MembersFull`, `Expired`, `InvalidRetention`,
`TimestampOverflow`, `VersionOverflow`, `InvalidPage`, `UnsupportedSchema`, `InvalidParent`,
`TooManyMentions`, `TooManyAttachments`. Codes are stable; additions append.

## Events

Topics: `THRROLE`, `THRNEW`, `THRMSG`, `THRRES`, `THROPN`, `THRMOD`. Data contains thread/message
IDs, action enums, sequence/version amounts, and content/resolution hashes. No addresses, message
text, mention lists, or attachment names are emitted. Replays emit nothing.

## Privacy boundaries

The chain reveals thread/message existence, ordering, moderation state, resolution class, and
commitments. It never reveals what was said, who was mentioned, or attachment contents. Off-chain
services must enforce visibility for hidden/locked messages and must not mirror content into public
event payloads. Retention expiry blocks reads of expired threads.

## Dependencies

- `soroban-sdk` workspace dependency (22.x), `no_std`, `rlib`/`cdylib`.
- Tenant/project identity and roles (CAPSTONE-038/041/050).
- Off-chain messaging store with mention resolution, attachment storage, and moderation UI.
- Indexer for ordered projections and TTL renewal.

## Rollout / rollback

1. Deploy a fresh tenant/project registry; no backfill of legacy chat history.
2. Backend writes message/attachment blobs and mention sets first, then posts commitments with
   idempotency IDs.
3. Frontend reads the indexer projection but verifies ordering and moderation state via the
   contract.
4. Rollback: stop new threads/messages, keep the off-chain store authoritative, and serve cached
   projections. `SCHEMA_VERSION = 1` allows upgrade decoding without mutating history.

## Corresponding work in other repositories (CAPSTONE-069)

- **Frontend:** threaded discussion UI, mentions, attachment picker, resolution and moderation controls.
- **Backend:** message store, mention notifications, attachment service, ordered indexer projection.
- **Moderation/Ops:** hide/restore workflows, retention jobs, audit dashboards with redacted payloads.
