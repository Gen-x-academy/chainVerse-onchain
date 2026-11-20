# Capstone Proposal Drafts Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-033] Create capstone proposal drafts
- Related issues: GitHub issue #1381 — Proposals track, shared work item CAPSTONE-033
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (draft persistence service, field validation engine, call-linkage resolver), chainVerse-frontend (proposal draft UI, structured form for all required fields)

## Scope

This contract (`contracts/capstone-proposal-drafts`) implements the on-chain
primitive required by **CAPSTONE-033 — Create capstone proposal drafts**.

The goal is to allow authors to open and persist a structured draft capturing
all required proposal fields — title, abstract, problem, objectives, methods,
expected outputs, timeline, risks, resources, preferred roles, and call
linkage — without storing any of that content on the public ledger.

**What the contract stores on-chain:**

- A draft record per `(author, draft_id)` containing: a `content_hash`
  (`BytesN<32>`) commitment over the full off-chain draft payload, a
  `schema_version` (`u32`) referencing the question-schema version the draft
  conforms to, a `call_id` (`BytesN<32>`) linking the draft to its target
  capstone call, a `draft_state` enum (`Open | Submitted | Withdrawn`), a
  `revision` counter (`u32`) incremented on every save, and the opened-at and
  last-saved-at ledger timestamps.
- A draft count per `author` (`u32`) used to enforce a per-author draft cap
  (`MAX_DRAFTS_PER_AUTHOR = 20`).
- A submission-contract address allowed to call `mark_submitted`.

**What stays off-chain:**

- All draft content: title text, abstract, problem statement, objectives,
  methods, expected outputs, timeline, risk assessment, resource list,
  preferred supervisor/role preferences, and call linkage details. Only the
  SHA-256 commitment over the canonically serialized draft document is stored.
- Field-level validation results: validated by the backend service against the
  question schema; not stored on-chain.
- Attachment file references beyond the opaque `content_hash`.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `open_draft(author, draft_id, call_id, schema_version, content_hash)` | author | Opens a new draft. Idempotent: a second call with the same `(author, draft_id)` returns `DraftAlreadyExists`. |
| `save_draft(author, draft_id, expected_revision, content_hash)` | author | Atomically updates the content commitment if `expected_revision` matches the stored `revision`; increments `revision` on success. Returns `StaleRevision` on mismatch. |
| `update_call_linkage(author, draft_id, call_id)` | author | Updates the target call linkage on an `Open` draft. |
| `withdraw_draft(author, draft_id)` | author | Sets state to `Withdrawn`; no further saves allowed. |
| `mark_submitted(author, draft_id, expected_revision)` | submission contract | Sets state to `Submitted` (terminal); further `save_draft` calls fail with `DraftAlreadySubmitted`. |
| `get_draft(author, draft_id)` | any | Returns `DraftRecord` or `None`. |
| `draft_count(author)` | any | Returns the number of `Open` or unsubmitted drafts for an author. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `DraftNotFound` | 1 | No draft exists for `(author, draft_id)`. |
| `DraftAlreadyExists` | 2 | A draft already exists at this `(author, draft_id)`. |
| `DraftAlreadySubmitted` | 3 | The draft has been finalised; no further mutations are allowed. |
| `DraftWithdrawn` | 4 | The draft was withdrawn; no further mutations are allowed. |
| `StaleRevision` | 5 | `expected_revision` does not match the stored `revision`; caller must reload and retry. |
| `DraftLimitReached` | 6 | Author already holds `MAX_DRAFTS_PER_AUTHOR` open drafts. |
| `Unauthorized` | 7 | Caller is not the draft author or the designated submission contract. |
| `InvalidContentHash` | 8 | `content_hash` is the zero hash. |
| `InvalidCallId` | 9 | `call_id` is the zero hash. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPPRDOPN` | `(author, draft_id, call_id, schema_version)` | Draft opened. |
| `CAPPRDSAV` | `(author, draft_id, revision, saved_at)` | Draft content saved. |
| `CAPPRDLNK` | `(author, draft_id, call_id)` | Call linkage updated. |
| `CAPPRDWDR` | `(author, draft_id)` | Draft withdrawn. |
| `CAPPRDSBM` | `(author, draft_id, revision)` | Draft submitted (terminal). |

### Storage layout

```
Persistent:
  SubmissionContract                                        → Address
  Draft(author: Address, draft_id: BytesN<32>)             → DraftRecord
  Count(author: Address)                                   → u32
```

`DraftRecord` fields:
- `author: Address`
- `draft_id: BytesN<32>`
- `call_id: BytesN<32>` — target capstone call identifier
- `schema_version: u32` — question-schema version the draft conforms to
- `content_hash: BytesN<32>` — commitment to the full off-chain draft payload
- `state: DraftState` — `Open | Submitted | Withdrawn`
- `revision: u32` — monotonically incrementing save counter
- `opened_at: u64`
- `last_saved_at: u64`

TTL: persistent, bumped to `[3_110_400, 6_220_800]` ledger ticks on every
write. The backend draft service must periodically extend entries for active
drafts to prevent expiry.

## Privacy

The contract never stores draft content, field values, or any part of the
proposal narrative. `content_hash` is a caller-supplied `BytesN<32>`
commitment whose preimage remains exclusively off-chain in the backend draft
service.

Events carry only the author address, draft identifier, revision counter, and
call linkage — no title text, abstract, or any proposal content.

## Authorization

- `open_draft`, `save_draft`, `update_call_linkage`, and `withdraw_draft` call
  `author.require_auth()` and verify the stored record's `author` matches the
  caller.
- `mark_submitted` additionally accepts a call from the designated submission
  contract address (stored at initialization). No other caller may transition
  a draft to `Submitted`.
- Authorization is deny-by-default: no mutating entry point has an open-access
  path.

## Idempotency and replay resistance

`open_draft` is idempotent for the same `(author, draft_id)`: a second call
returns `DraftAlreadyExists` without modifying state.

`save_draft` requires an `expected_revision` that must equal the stored
`revision`. Because the revision counter increments atomically on success, a
duplicate or replayed save transaction with the same `expected_revision` fails
with `StaleRevision`. The backend service must treat `StaleRevision` as a
signal to reload the latest draft before retrying.

`withdraw_draft` on an already-withdrawn draft returns `DraftWithdrawn` —
idempotent in that the state does not change further, but the caller is
informed the draft was already terminal.

`mark_submitted` is idempotent: a second call on an already-submitted draft
returns `DraftAlreadySubmitted`.

## State machine

```
Open  ──save_draft──►  Open (revision++)
Open  ──withdraw──────►  Withdrawn (terminal)
Open  ──mark_submitted►  Submitted (terminal)
Withdrawn / Submitted: no further mutations allowed
```

`update_call_linkage` is only permitted in the `Open` state; it returns
`DraftAlreadySubmitted` or `DraftWithdrawn` for terminal states.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Draft content (all fields) | `content_hash` commitment | Storage, encryption, versioned snapshot history |
| Field validation | `schema_version` reference | Validation engine checking draft against published question schema |
| Call linkage resolution | `call_id` reference | Backend resolves call details, deadlines, and eligibility rules |
| Autosave scheduling | — | Debounce interval, network retry, exponential backoff |
| Last-saved feedback | `last_saved_at` ledger timestamp | Rendering "Saved N seconds ago" in the UI |
| Submission eligibility | — | Backend checks preflight result (CAPSTONE-037) before calling `mark_submitted` |

## Dependencies

- **capstone-identifiers**: `draft_id` and `call_id` values should be valid
  public identifiers produced by that contract.
- **CAPSTONE-035** (configurable questions): the `schema_version` stored in the
  draft must reference a valid published version from the question-schema
  contract. The backend validates the draft content against that version before
  computing `content_hash`.
- **CAPSTONE-030** (supervisor directory search): the author selects preferred
  supervisor roles during drafting; the backend resolves supervisor availability
  from that registry before allowing submission.
- **CAPSTONE-034** (autosave revisions): if fine-grained autosave and conflict
  detection are needed, the autosave revisions contract (`CAPSTONE-034`)
  extends this contract's draft lifecycle with field-level timestamps and
  three-way merge support. These two contracts may coexist: this contract
  manages draft identity and call linkage; the revisions contract manages
  fine-grained autosave.
- **Proposal submission contract**: must call `mark_submitted` to close the
  draft. Its address is configured at initialization.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `save_draft` / `withdraw_draft` rejected for non-
  author; `mark_submitted` rejected for callers who are neither the author nor
  the submission contract; cross-author mutation rejected with `Unauthorized`.
- **Replay resistance**: save a revision, replay the same call with the now-
  stale `expected_revision` → `StaleRevision`; double `open_draft` →
  `DraftAlreadyExists`.
- **State machine**: valid path `Open → Submitted`; `save_draft` on `Submitted`
  → `DraftAlreadySubmitted`; `save_draft` on `Withdrawn` → `DraftWithdrawn`;
  `update_call_linkage` on a terminal state → correct terminal error.
- **Draft cap**: create `MAX_DRAFTS_PER_AUTHOR` open drafts → succeeds; open
  one more → `DraftLimitReached`; withdraw one → new `open_draft` → succeeds.
- **Count consistency**: create N drafts, withdraw M, assert `draft_count` =
  N - M at each step.
- **Idempotency**: `open_draft` twice → no state change on second call;
  `mark_submitted` twice → `DraftAlreadySubmitted` on second call.
- **TTL**: assert persistent TTL is extended on every write by reading
  `env.storage().persistent().get_ttl()` after each mutating call.
- **Zero hash guards**: `content_hash = [0u8; 32]` → `InvalidContentHash`;
  `call_id = [0u8; 32]` → `InvalidCallId`.
- **Event schemas**: assert topic symbol and payload types for all five events.
- **Error codes**: every `ContractError` variant has at least one test.
- **Resource budget**: smoke-test `open_draft` and `save_draft` within default
  Soroban instruction and memory limits on a maximally-sized record.
- **Upgrade compatibility**: `DraftRecord` additions must not break reads of
  earlier WASM records.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live and the submission contract
   address is known (required for initialization).
2. Configure the backend draft service with the deployed contract address
   before enabling the proposal draft UI.
3. The backend service must handle `StaleRevision` as a retriable conflict, not
   a fatal error; it must reload the latest `revision` before retrying.
4. Confirm that the question-schema contract (`CAPSTONE-035`) has published at
   least one schema version before allowing authors to open drafts, so
   `schema_version` references are valid.
5. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rollback to a prior WASM: safe as long as the `DraftRecord` XDR schema has
  not changed. If fields were added, test with the upgrade-compatibility suite
  before rollback.
- If a rollback removes `mark_submitted`, drafts already in `Submitted` state
  remain terminal and are unaffected; in-flight saves on `Open` drafts will
  continue to work if `save_draft` survives the rollback.
- The backend draft service must be rolled back in concert with the contract to
  avoid sending `expected_revision` values in a schema the contract no longer
  understands.

## Corresponding CAPSTONE-033 work in other repositories

- **chainVerse-backend**: draft persistence service — stores draft content
  keyed by `(author, draft_id)`, computes and verifies `content_hash` on every
  save, validates draft fields against the published question schema
  (`schema_version`), resolves call linkage and eligibility, handles
  `StaleRevision` retries, computes preflight status before submission.
- **chainVerse-frontend**: proposal draft UI — structured form capturing all
  required fields (title, abstract, problem, objectives, methods, expected
  outputs, timeline, risks, resources, preferred roles, call linkage); displays
  "Saved" feedback sourced from `last_saved_at`; renders validation errors
  returned by the backend; surfaces supervisor search results from CAPSTONE-030
  within the preferred-roles section.
