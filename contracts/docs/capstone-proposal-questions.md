# Capstone Proposal Questions Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-035] Support configurable proposal questions
- Related issues: GitHub issue #1385 — Proposals track, shared work item CAPSTONE-035
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (question schema service, validation engine), chainVerse-frontend (dynamic form renderer)

## Scope

This contract (`contracts/capstone-proposal-questions`) implements the
on-chain primitive required by **CAPSTONE-035 — Support configurable proposal
questions**.

The goal is to allow call owners to define versioned typed questions,
validation, help text, ordering, conditional visibility, and safe rendering.

**What the contract stores on-chain:**

- An immutable, versioned schema record per `(call_id, version)` pair,
  containing a `schema_hash` (`BytesN<32>`) commitment over the full
  off-chain question schema document and bounded public metadata: the
  published-by address, published-at ledger timestamp, question count
  (`u8`, max 50), and the latest active version pointer.
- An owner record per `call_id` so only the original call owner can publish
  further versions.
- A `latest` pointer per `call_id` that always resolves to the most recently
  published version number.

**What stays off-chain:**

- All question definitions: label text, help text, placeholder text, option
  lists, validation rules (regex, word limits, numeric ranges), conditional
  visibility expressions, ordering, and safe-rendering metadata. The
  `schema_hash` is a SHA-256 commitment over the canonically serialized
  schema document.
- Form renderer configuration, localisation, and conditional logic
  evaluation are off-chain concerns.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `publish_schema(authority, call_id, version, schema_hash, question_count)` | call owner | Publishes version N of a question schema. Version must be strictly greater than the current latest. First publish also registers the caller as the call owner. |
| `get_schema(call_id, version)` | any | Returns `PublishedSchema` for the given version, or `None`. |
| `latest_version(call_id)` | any | Returns the current latest version number, or `None` if no schema has been published. |
| `get_owner(call_id)` | any | Returns the registered owner address. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `InvalidVersion` | 1 | `version` is 0 or not greater than the current latest. |
| `VersionAlreadyPublished` | 2 | A schema record already exists at this `(call_id, version)`. |
| `UnauthorizedOwner` | 3 | Caller is not the registered owner of this call. |
| `CallNotFound` | 4 | No schema has been published for this `call_id`. |
| `InvalidQuestionCount` | 5 | `question_count` is 0 or exceeds `MAX_QUESTIONS` (50). |
| `InvalidSchemaHash` | 6 | `schema_hash` is the zero hash. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPQSNEW` | `(call_id, version, authority, question_count)` | First schema published for a call. |
| `CAPQSUPD` | `(call_id, version, authority, question_count)` | Subsequent version published. |

### Storage layout

```
Persistent:
  Owner(call_id: BytesN<32>)              → Address
  Schema(call_id: BytesN<32>, ver: u32)   → PublishedSchema
  Latest(call_id: BytesN<32>)             → u32
```

`PublishedSchema` fields:
- `call_id: BytesN<32>`
- `version: u32`
- `published_by: Address`
- `published_at: u64`
- `schema_hash: BytesN<32>` — commitment to the full off-chain schema document
- `question_count: u8` — bounded count (1–50); informs renderers and validators

TTL: persistent entries are bumped to `[3_110_400, 6_220_800]` ledger ticks
on every write (consistent with `capstone-call-terms` and other capstone
contracts).

## Privacy

The contract never stores question text, help text, validation rules, option
lists, or conditional logic. `schema_hash` is a caller-supplied `BytesN<32>`
commitment whose preimage remains entirely off-chain.

Events carry only the call identifier, version, authority address, and
question count — no schema content.

## Authorization

`publish_schema` calls `authority.require_auth()`. On first publication for a
`call_id`, the caller is registered as the owner. All subsequent publications
for that `call_id` require the same owner address. Any other caller is
rejected with `UnauthorizedOwner`.

Authorization is deny-by-default: a caller who is not the registered owner
cannot publish a schema version, regardless of any other on-chain state.

There is no admin override path: ownership is set once and is permanent for
the lifetime of the contract deployment. If ownership must change (e.g. a
call is transferred), a new `call_id` must be used.

## Idempotency and replay resistance

Each `(call_id, version)` pair is written exactly once. A second transaction
with the same pair fails with `VersionAlreadyPublished` before modifying any
state. Version numbers are strictly monotonically increasing, so a replayed
old transaction always fails with `InvalidVersion` or `VersionAlreadyPublished`.

The `latest` pointer is updated atomically in the same transaction as the
schema write; there is no window where `latest` points to a version that does
not yet have a stored record.

## Off-chain schema document

The preimage behind `schema_hash` is a canonically serialized document that
the off-chain question schema service owns. It must include:

- Per-question: stable `question_id` (UUID or slug), `type` (short-text,
  long-text, single-choice, multi-choice, numeric, date, file-reference),
  `label`, `help_text`, `placeholder`, `required` flag, `order` integer,
  `validation` rules (type-specific — word limits, numeric ranges, allowed
  MIME types, max file size), `conditional_visibility` expression (references
  other `question_id`s and their values), and `safe_render_policy`
  (HTML-escaping level, allowed Markdown subset).
- Schema-level: `schema_version`, `call_id`, `published_at` (ISO 8601),
  `locale` default, and a `canonical_hash` self-reference.

The hash must be computed over the canonical UTF-8 JSON serialization with
keys sorted, no insignificant whitespace, and no BOM.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Question definitions | `schema_hash` commitment | Schema document storage, versioning, delivery to renderers |
| Validation rules | counted in `question_count` | Evaluation against applicant inputs |
| Conditional visibility | — | Expression evaluation in the form renderer |
| Safe rendering | — | HTML escaping, Markdown parsing, XSS prevention |
| Schema change notifications | `CAPQSUPD` event | Off-chain indexer triggers re-fetch and cache invalidation |

## Dependencies

- **capstone-call-terms**: `call_id` values are expected to refer to calls
  whose terms have been published by that contract. Question schema versions
  should be published only after call terms are finalised, so applicants see a
  stable combination of terms and questions.
- **capstone-identifiers**: `call_id` values should be valid public identifiers
  registered by that contract.
- **capstone-proposal-draft-revisions** (CAPSTONE-034): the draft content
  committed in that contract's `content_hash` must be structured according to
  the question schema version recorded in the draft. The off-chain autosave
  service must carry the `schema_version` in every draft payload.
- **Proposal submission contract** (future): must validate that the submitted
  `form_version` matches a published schema version for the target `call_id`
  before accepting a submission.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: publish as owner, then publish again as a different
  address and assert `UnauthorizedOwner`; assert the first publish registers
  the caller as owner.
- **Replay resistance**: publish version 1, replay the same transaction and
  assert `VersionAlreadyPublished`; attempt version 0 and assert
  `InvalidVersion`; attempt a version lower than latest and assert
  `InvalidVersion`.
- **Strict monotonicity**: publish versions 1 → 2 → 3 in order; assert
  `latest_version` returns 3 after each publish; attempt version 2 again after
  publishing 3 and assert `VersionAlreadyPublished`.
- **Storage bounds**: `question_count` = 0 → `InvalidQuestionCount`; = 51 →
  `InvalidQuestionCount`; = 50 → succeeds.
- **Zero hash guard**: supply `schema_hash = [0u8; 32]` and assert
  `InvalidSchemaHash`.
- **Event schemas**: assert topic and payload types for `CAPQSNEW` and
  `CAPQSUPD` events; assert `CAPQSNEW` fires on first publish and `CAPQSUPD`
  on subsequent versions.
- **TTL extension**: assert that persistent entries receive a TTL bump on
  every write.
- **Error codes**: every `ContractError` variant has at least one test.
- **Resource budget**: smoke-test that `publish_schema` completes within
  default Soroban instruction and memory budgets.
- **Upgrade compatibility**: the `PublishedSchema` type must be forward-
  compatible; test that adding a new optional field does not break reads of
  existing records.

## Rollout considerations

1. Deploy after `capstone-identifiers` and `capstone-call-terms` are live.
2. Configure the off-chain question schema service with the deployed contract
   address before allowing call owners to publish schemas.
3. Call owners must publish a schema before the draft autosave service
   (CAPSTONE-034) or proposal submission contract accepts drafts or
   submissions against that call.
4. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rolling back to a prior WASM is safe as long as the `PublishedSchema` XDR
  schema has not changed. If new optional fields were added, an old WASM
  reads them as defaults; validate with the upgrade-compatibility test suite.
- Schema records are immutable once written; a rollback does not remove
  already-published records. Off-chain renderers may cache schema documents
  keyed by `schema_hash`; a rollback does not invalidate those caches.
- The off-chain question schema service must be rolled back in concert with
  the contract if the `schema_hash` computation algorithm changes.

## Corresponding CAPSTONE-035 work in other repositories

- **chainVerse-backend**: question schema service — schema document storage,
  `schema_hash` computation (canonical JSON → SHA-256), schema change
  notifications via `CAPQSUPD` event indexer, versioned delivery API for
  renderers and validators.
- **chainVerse-frontend**: dynamic form renderer — fetches schema by
  `call_id` + `version`, evaluates conditional visibility expressions,
  applies safe-render policies, displays question ordering and help text.
