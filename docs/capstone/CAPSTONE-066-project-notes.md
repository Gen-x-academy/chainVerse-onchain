# CAPSTONE-066: Collaborative Project Notes

**Issue:** #1414 | **Component:** on-chain | **Contract:** `contracts/capstone-project-notes`
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the worktree baseline
**Depends on:** tenant/project identity from CAPSTONE-038 and project roles from CAPSTONE-041/050

## Scope

On-chain primitive for shared, tenant/project-scoped notes: creation, bounded revision history,
link graph, render policy selection, archival, retention, and ordered page reads. The contract
enforces authorization and integrity only; it does not store or render note content.

## On-chain stores vs off-chain content

| On-chain | Off-chain |
| --- | --- |
| `tenant_id`, `project_id`, `note_id`, request/intent IDs | Note title, body, images, links' URLs |
| `content_hash`, `ref_hash`, revision `prev_hash` | Markdown/HTML source and rendered output |
| `RenderPolicy`, `NoteState`, role enums, version counters | Editor UI state, drafts, comments payloads |
| `expires_at`, `updated_at`, bounded `Vec` indexes | Search index, snapshots, analytics, notifications |

No `String`, `Bytes`, plaintext, or free text is stored. `ref_hash` is a high-entropy commitment
to the off-chain content envelope, never a hash of guessable text.

## Authorization

`initialize` requires the admin's signature and stores `Admin` under
`(tenant_id, project_id, admin)`. `grant_role` requires the stored `Admin` role and can grant
`Viewer`, `Editor`, or `Admin`. Every other entry point calls `actor.require_auth()` and then
checks the stored role for that exact tenant/project:

- Reads (`get_note`, `get_revision`, `notes_page`): `Viewer` or higher.
- Mutations (create, revise, archive, link, render policy): `Editor` or higher.
- Role grants: `Admin` only.

Deny by default: missing configuration (`NotInitialized`) or missing/insufficient stored role
(`NotAuthorized`) rejects the call. Foreign tenants and projects have separate keys and cannot
inherit roles.

## Idempotency

Every state-changing call takes `request_id` (or `intent_id` for archive) plus `expected_version`.
The request record is scoped by `(tenant_id, project_id, request_id)` and stores a payload digest
that includes actor hash, target IDs, action enum, relevant hashes, and the expected version.

- Exact replay returns the original result without a second state change or event.
- Same `request_id` with a different payload returns `RequestConflict`.
- A new request with a stale `expected_version` returns `StaleVersion`.
- All counters use `checked_add`; overflow returns `VersionOverflow` or `TimestampOverflow`.

Collections are bounded: 16 members, 32 notes, 4 links per note, 8 revisions per note, and page
size at most 50. Retention must be in `1..=6_220_800` seconds. All reads and writes extend
persistent TTL to the network maximum window.

## Errors

Append-only `ContractError` codes 1..19: `NotInitialized`, `AlreadyInitialized`,
`NotAuthorized`, `NotFound`, `StaleVersion`, `RequestConflict`, `AlreadyExists`, `AlreadyLinked`,
`NoteArchived`, `NotesFull`, `RevisionsFull`, `LinksFull`, `MembersFull`, `Expired`,
`InvalidRetention`, `TimestampOverflow`, `VersionOverflow`, `InvalidPage`, `UnsupportedSchema`.
Codes are never reordered or reused; new codes append.

## Events

Topics are `symbol_short`: `NTROLE`, `NTCRT`, `NTRVS`, `NTARC`, `NTLNK`, `NTRND`. Data contains only
IDs, action enums, version/count amounts, and hashes (for example
`(note_id, NoteAction::Create, version, content_hash)`). Addresses, titles, bodies, and URLs are
never emitted. Replays emit nothing.

## Privacy boundaries

Notes are shared only within the configured tenant/project role set. The chain reveals that a note
exists, its size-agnostic hash, its state transitions, and its role-gated access graph. Content,
authorship display names, and link targets stay in the notes service. Private researcher notes are
handled exclusively by CAPSTONE-067 and are not readable through this contract.

## Dependencies

- `soroban-sdk` workspace dependency (22.x), `no_std`, `rlib`/`cdylib`.
- Capstone project identity and role provisioning from CAPSTONE-038/041/050.
- Off-chain notes service for content storage and safe Markdown rendering allowlist.
- Indexer for event projection and TTL renewal policy.

## Rollout / rollback

1. Deploy with a fresh tenant/project configuration; no backfill of legacy notes.
2. Backend begins dual-writing commitments and replay-safe request IDs.
3. Frontend switches reads to the contract-verified projection once the indexer catches up.
4. Rollback: stop new writes, keep reads available, and let the off-chain service remain the source
   of truth. Stored records are schema-versioned (`SCHEMA_VERSION = 1`) so an upgrade can decode
   v1 entries; no in-place mutation of history exists.

## Corresponding work in other repositories (CAPSTONE-066)

- **Frontend:** note editor, safe renderer, revision/link views, archive controls.
- **Backend:** content envelope store, commitment generation, idempotency key issuance, indexer.
- **Docs/API:** request/response schemas documenting that payload text never reaches the ledger.
