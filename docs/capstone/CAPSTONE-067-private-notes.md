# CAPSTONE-067: Private Researcher Notes

**Issue:** #1415 | **Component:** on-chain | **Contract:** `contracts/capstone-private-notes`
**Audit baseline:** `main` @ `5b62b369` (2026-11-18), revalidated against the worktree baseline
**Depends on:** tenant/project identity from CAPSTONE-038 and researcher provisioning from CAPSTONE-041

## Scope

Owner-only registry commitments for encrypted researcher notes within a tenant/project. The
contract records ciphertext/key-wrap commitments, content lengths, algorithm enums, revision
hashes, retention, and archival state. It intentionally exposes no team-wide read path and no
private payload.

## On-chain stores vs off-chain content

| On-chain | Off-chain |
| --- | --- |
| `tenant_id`, `project_id`, owner-scoped `note_id`, request/intent IDs | Plaintext notes, research findings, drafts |
| `ciphertext_hash`, `key_wrap_hash`, revision `prev_hash` | Encrypted blob storage and object-store URLs |
| `content_length`, `CryptoPolicy`, state/revision enums | Keys, KMS policies, decryption audit logs |
| `expires_at`, owner index page IDs | Search, analytics, reviewer/team views (excluded) |

No private or free text is stored on-chain. `ciphertext_hash` commits to the encrypted blob;
`key_wrap_hash` commits to the wrapped key envelope. Neither is a hash of plaintext.

## Authorization

`initialize` requires admin auth and stores the `Admin` role. `grant_researcher` requires stored
`Admin` and grants `Researcher`. Every read and mutation calls `actor.require_auth()` and requires
a stored `Researcher` (or `Admin`) role for that tenant/project, then requires the note's stored
owner to equal the actor. Consequences:

- A researcher can only read/revise/archive/expire their own notes.
- An admin can provision researchers but cannot read another researcher's note.
- There is no team-wide note listing; `owner_notes` is keyed by the authenticated owner only.
- Missing role or missing/non-owned note returns `NotAuthorized`/`NotFound` (deny by default).

## Idempotency

Mutations take `request_id` (or `intent_id` for archive) and `expected_version`. Request records are
scoped to `(tenant_id, project_id, request_id)` and include an actor hash, target ID, ciphertext
and key-wrap hashes, content length, algorithm enum, action enum, expected version, and retention
in the payload digest. Exact replay returns the prior result with no duplicate index entry or
event; a changed payload with the same ID yields `RequestConflict`; a fresh request with a stale
version yields `StaleVersion`.

Bounds: 32 researchers, 32 notes per owner, 16 revisions per note, 1 MiB maximum committed
content length, page size at most 50, retention `1..=6_220_800` seconds. All arithmetic is checked;
timestamp overflow returns `TimestampOverflow`. Reads and writes extend TTL.

## Errors

Append-only `ContractError` codes 1..19: `NotInitialized`, `AlreadyInitialized`,
`NotAuthorized`, `NotFound`, `StaleVersion`, `RequestConflict`, `AlreadyExists`, `NoteArchived`,
`NotesFull`, `RevisionsFull`, `MembersFull`, `Expired`, `InvalidRetention`, `TimestampOverflow`,
`VersionOverflow`, `InvalidPage`, `UnsupportedSchema`, `InvalidContentLength`, `NotExpired`.
New failures append; existing codes never change meaning.

## Events

Topics: `PRVROLE`, `PRVNEW`, `PRVREV`, `PRVARC`, `PRVEXP`. Data contains note IDs, action enums,
revision/version amounts, and ciphertext hashes only. No address, plaintext, key material, content
length, or object URL is broadcast. Replays emit no event.

## Privacy boundaries

The ledger reveals that a researcher owns an opaque note commitment and when it changed state. It
does not reveal content, keywords, recipients, or review participation. Notes must never be fed
into team review, analytics content, or general application logs. Clients must use the owner-scoped
read path and must not attempt cross-owner enumeration (none exists).

## Dependencies

- `soroban-sdk` workspace dependency (22.x), `no_std`, `rlib`/`cdylib`.
- Tenant/project identity and researcher role provisioning (CAPSTONE-038/041).
- Off-chain encrypted blob store plus KMS/wrapping service.
- Indexer limited to event metadata; it must not project content.

## Rollout / rollback

1. Deploy with a fresh tenant/project config and register researchers explicitly.
2. Backend writes encrypted blobs first, then commits hashes with idempotency keys.
3. Frontend enables the private-notes view only after owner-scoped reads are verified.
4. Rollback: stop new commitments and keep the off-chain encrypted store as the authoritative copy;
   no migration of private content is performed by a contract upgrade. Records carry
   `SCHEMA_VERSION = 1` for forward-compatible decoding.

## Corresponding work in other repositories (CAPSTONE-067)

- **Frontend:** private note editor with local/client-side encryption and owner-only views.
- **Backend:** blob storage, key wrapping, commitment generation, strict log redaction.
- **Security/Platform:** KMS policy, retention enforcement job, exclusion from analytics pipelines.
