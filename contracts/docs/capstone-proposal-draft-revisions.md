# Capstone Proposal Draft Revisions Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-034] Autosave proposal revisions
- Related issues: GitHub issue — Proposals track, shared work item CAPSTONE-034
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (autosave service, conflict resolution), chainVerse-frontend (draft UI, offline recovery)

## Scope

This contract (`contracts/capstone-proposal-draft-revisions`) implements the
on-chain primitive required by **CAPSTONE-034 — Autosave proposal revisions**.

The goal is to persist resumable drafts with revision tokens, field timestamps,
conflict detection, offline recovery, and explicit last-saved feedback.

**What the contract stores on-chain:**

- A bounded, keyed draft record per `(author, call_id)` pair containing: a
  content commitment (`BytesN<32>` hash of the off-chain draft payload), the
  current revision token (`u32`), a last-saved ledger timestamp, and an
  optional conflict flag.
- A per-field-group timestamp map commitment (`BytesN<32>`) that the
  off-chain service uses to implement field-level conflict detection without
  exposing field names or values to the ledger.
- An explicit `draft_state` enum: `Active`, `Conflicted`, `Recovered`,
  `Submitted` (terminal, set by the submission contract).

**What stays off-chain:**

- All draft content: answer text, file references, section order, partial
  responses. Only the SHA-256 commitment over the canonically serialized draft
  document is stored.
- Field-level timestamps used for three-way merge: the commitment to the
  field-timestamp map is stored; the map itself lives in the off-chain
  autosave service.
- Recovery payloads: the off-chain service holds the versioned snapshot
  history; the contract records only the latest committed revision.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `open_draft(author, call_id, content_hash, field_ts_hash)` | author | Opens a new draft or no-ops if one already exists (idempotent). |
| `save_revision(author, call_id, expected_token, content_hash, field_ts_hash)` | author | Atomically bumps the revision token if `expected_token` matches current; rejects stale tokens with `StaleRevision`. |
| `mark_conflicted(author, call_id, expected_token, conflict_hash)` | author | Transitions state to `Conflicted` and records a commitment to the conflict diff produced off-chain. |
| `resolve_conflict(author, call_id, expected_token, resolved_hash, field_ts_hash)` | author | Clears the conflict flag, records the resolved content commitment, and bumps the token. |
| `get_draft(call_id, author)` | any | Returns the current `DraftRecord` or `None`. |
| `mark_submitted(author, call_id, expected_token)` | submission contract (cross-contract) | Sets state to `Submitted`; further `save_revision` calls on this draft fail with `DraftAlreadySubmitted`. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `DraftNotFound` | 1 | No draft exists for `(author, call_id)`. |
| `DraftAlreadyExists` | 2 | `open_draft` called when a non-submitted draft already exists. |
| `StaleRevision` | 3 | `expected_token` does not match the stored token; caller must reload and retry. |
| `DraftAlreadySubmitted` | 4 | The draft has been finalised; no further mutations are allowed. |
| `Unauthorized` | 5 | Caller is not the draft author or the designated submission contract. |
| `InvalidContentHash` | 6 | Supplied content hash is the zero hash or otherwise structurally invalid. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPDRNEW` | `(author, call_id, revision_token)` | New draft opened. |
| `CAPDRSAV` | `(author, call_id, revision_token, saved_at)` | Revision saved. |
| `CAPDRCON` | `(author, call_id, revision_token)` | Draft marked conflicted. |
| `CAPDRREC` | `(author, call_id, revision_token)` | Conflict resolved. |
| `CAPDRSBM` | `(author, call_id, revision_token)` | Draft submitted (terminal). |

### Storage layout

```
Persistent:
  Draft(author: Address, call_id: BytesN<32>)  → DraftRecord
```

`DraftRecord` fields:
- `author: Address`
- `call_id: BytesN<32>`
- `revision_token: u32`
- `content_hash: BytesN<32>` — commitment to the full draft payload
- `field_ts_hash: BytesN<32>` — commitment to the field-timestamp map
- `conflict_hash: Option<BytesN<32>>` — commitment to conflict diff, set during `Conflicted`
- `state: DraftState` — `Active | Conflicted | Recovered | Submitted`
- `last_saved_at: u64` — ledger timestamp of last successful save
- `opened_at: u64`

TTL: persistent, bumped to `[3_110_400, 6_220_800]` ledger ticks on every
write (consistent with other capstone contracts). Drafts that outlive TTL
without a touch are archived rather than deleted; the off-chain autosave
service must periodically extend entries it expects to keep alive.

## Privacy

The contract never stores draft content. `content_hash` and `field_ts_hash`
are caller-supplied `BytesN<32>` commitments whose preimages remain
exclusively off-chain. `conflict_hash` follows the same pattern.

Audit events carry only the author address, call identifier, and revision
token — no draft text, field names, or answer fragments.

## Authorization

- `save_revision`, `mark_conflicted`, `resolve_conflict` all call
  `author.require_auth()` and verify that the stored draft's `author` matches
  the caller. No relayer or admin may mutate another author's draft.
- `mark_submitted` additionally accepts a call from the designated submission
  contract address (stored at `initialize` time). No other caller may
  transition a draft to `Submitted`.
- Authorization is deny-by-default: any call that does not pass both
  `require_auth` and the ownership check is rejected with `Unauthorized`.

## Idempotency and replay resistance

`save_revision` requires an `expected_token` that must equal the stored
`revision_token`. Because the token is incremented atomically on success, a
duplicate or replayed save transaction with the same `expected_token` fails
with `StaleRevision`. The off-chain autosave service must treat `StaleRevision`
as a signal to reload the latest revision before retrying, not as a transient
error.

`open_draft` is idempotent: a second call for the same `(author, call_id)`
returns `DraftAlreadyExists` without modifying state.

## Concurrency and conflict detection

Two concurrent offline sessions saving against the same draft will produce
divergent `expected_token` values. The second committer receives `StaleRevision`,
fetches the current record (including the winning `content_hash`), and must
run three-way merge off-chain using the stored `field_ts_hash` commitments.
After merge, the caller calls `resolve_conflict` or `mark_conflicted` as
appropriate. The on-chain contract never performs merge logic; it only
witnesses the author's attestation that a conflict was detected or resolved.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Draft content | `content_hash` commitment | Storage, encryption, versioned snapshot history |
| Field-level timestamps | `field_ts_hash` commitment | Three-way merge, conflict detection algorithm |
| Last-saved feedback | `last_saved_at` ledger timestamp in `DraftRecord` | Rendering "Saved N seconds ago" in the UI |
| Offline recovery | Latest `content_hash` as recovery anchor | Snapshot ring-buffer, replay of queued saves |
| Autosave scheduling | — | Debounce interval, network retry, exponential backoff |

## Dependencies

- **capstone-identifiers**: `call_id` values are expected to be valid
  `BytesN<32>` public identifiers produced by that contract.
- **Proposal submission contract** (future): must call `mark_submitted` to
  close the draft. The submission contract address is configured at
  `initialize` time and is the only additional caller permitted on
  `mark_submitted`.
- **CAPSTONE-035** (configurable questions): the structure of the off-chain
  draft payload committed by `content_hash` is defined by the question
  schema versioned by that contract. The `form_version` committed in the
  draft payload must match a valid published question schema version.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: verify that `save_revision`, `mark_conflicted`, and
  `resolve_conflict` reject callers who are not the stored author; verify
  `mark_submitted` rejects callers who are neither the author nor the
  submission contract.
- **Replay resistance**: save a revision, then replay the same call with the
  now-stale token and assert `StaleRevision`.
- **State transitions**: valid path `Active → Conflicted → Recovered`;
  invalid transitions (e.g. saving on a `Submitted` draft) must return
  `DraftAlreadySubmitted`.
- **Idempotency**: double `open_draft` returns `DraftAlreadyExists`; no state
  mutation occurs on the second call.
- **Expiry / TTL**: assert that the TTL extension is applied on every write by
  reading `env.storage().persistent().get_ttl()` after each mutating call.
- **Storage bounds**: a single draft record per `(author, call_id)` — confirm
  no unbounded growth path.
- **Event schemas**: assert topic and payload types for every event code.
- **Error codes**: every `ContractError` variant has at least one test that
  triggers it.
- **Resource budget**: smoke-test that `save_revision` completes within
  Soroban's default instruction and memory limits on a maximally-sized record.
- **Upgrade compatibility**: the `DraftRecord` type must include a `version`
  field bump path; test that an old record shape can be read and upgraded.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live and after the submission
   contract address is known (required for `initialize`).
2. Configure the off-chain autosave service with the deployed contract address
   before enabling draft UI.
3. The autosave service must handle `StaleRevision` as a retriable conflict,
   not a fatal error.
4. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rollback to a prior WASM: safe as long as the `DraftRecord` XDR schema has
  not changed between versions. If fields were added, an old WASM will
  deserialize unknown fields to defaults; test with the upgrade-compatibility
  test suite before rollback.
- If a rollback removes the `mark_submitted` entry point, drafts that were
  already submitted remain `Submitted` and are not affected; in-flight saves
  will return `Unauthorized` or `NotFound` once the submission contract can no
  longer call in.
- Off-chain autosave service must be rolled back in concert with the contract
  to avoid sending `expected_token` values in a schema the contract no longer
  understands.

## Corresponding CAPSTONE-034 work in other repositories

- **chainVerse-backend**: autosave service — debounce, snapshot ring-buffer,
  field-timestamp merge algorithm, offline queue flush, `content_hash` and
  `field_ts_hash` computation, `StaleRevision` retry logic.
- **chainVerse-frontend**: draft UI — "Saved N seconds ago" indicator sourced
  from `last_saved_at`, conflict resolution dialog, offline recovery banner,
  resume-on-login flow.
