# Scholarship Core (Program Data Model + Lifecycle) Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1058 (data model), #1061 (lifecycle states), #1059
  (sponsor organizations — not yet implemented), #1060 (sponsor team
  membership/invitations — not yet implemented)

## Scope

This contract (`contracts/scholarship-core`) implements two of the
"Scholarships On-chain" epic's foundational behaviors:

- **#1058 — Model scholarship programs and immutable identifiers.**
  `create_program()` stores a `Program` record keyed by a caller-supplied
  `program_id: BytesN<32>` — that ID is both the storage key and the
  record's `id` field, and this contract has no rename/re-key operation,
  so it genuinely never changes after creation. `title` and `description`
  are length-bounded (`MAX_TITLE_LEN = 120`, `MAX_DESCRIPTION_LEN = 2000`)
  and validated before the record is ever persisted — an invalid program
  cannot be created, let alone published. `funding_model` is a small,
  closed enum (`FixedAward` / `MatchingFund` / `MilestoneBased`) rather
  than free text.
- **#1061 — Draft/published/paused/closed/archived lifecycle.**
  `transition_program()` only allows the pairs enumerated in
  `is_legal_transition`: `Draft→Published`, `Draft→Archived`,
  `Published→Paused`, `Published→Closed`, `Paused→Published`,
  `Paused→Closed`, `Closed→Archived`. Every other pair — including
  `Archived→` anything — is rejected with `InvalidTransition`, making
  `Archived` a true terminal state. Every successful transition updates
  the record's `last_transitioned_by`/`last_transitioned_at` fields *and*
  emits a `PROGTRAN` event carrying `(program_id, from, to, actor, at)` —
  see Operational impact for why the full history lives in events rather
  than an on-chain list.

Other contracts already built in this epic (`scholarship-applications`,
`scholarship-eligibility`, `scholarship-programs`) all currently treat a
program as an opaque `BytesN<32>` with their own narrow slice of
configuration (submission rules, eligibility rules, windows/budget). This
contract is that ID's actual owning record — a natural next step is having
those contracts validate against `scholarship-core`'s program existence and
status (e.g. `submit_application` should probably also check
`get_program_status(program_id) == Published`), but that cross-contract
wiring is not done in this pass.

**Not implemented here** (tracked separately): sponsor organizations with
verified profiles (#1059) and sponsor team membership/invitations (#1060).
Until those exist, `sponsor_id` is stored as an opaque, unvalidated
reference, and the program's actual authorization principal is `owner` (a
single `Address` set at creation) rather than a sponsor org's
least-privilege member roles.

## Privacy

`title`, `description`, and `currency` are the only free-form-ish fields,
and they describe the *program*, not any applicant — this contract holds
no applicant-identifying data at all. `sponsor_id` is opaque (no sponsor
contact/branding data lives here pending #1059).

## Ownership

The program's `owner` (set once, at creation, to the creating caller) is
the sole party who can transition its lifecycle. There is no admin
override in this contract — an admin exists only for contract-level
initialization bookkeeping, not for managing individual programs. This is
a deliberate simplification pending #1059/#1060: once sponsor
organizations and roles exist, ownership/transition authority should
likely move to "any sponsor-org member with the right role for this
program" rather than a single fixed address.

## Migration

New contract, no prior on-chain state.

## Operational impact

- Transition history is **not** stored as a growing on-chain list — only
  the *latest* transition's actor/time live on the `Program` record
  itself (bounded storage). The full sequence of transitions is
  reconstructable from the `PROGTRAN` events every `transition_program`
  call emits, which is the standard Soroban pattern for unbounded audit
  trails without unbounded contract storage. Any indexer/off-chain service
  that needs "every transition, in order" for an archived program should
  subscribe to and retain these events rather than expect the contract to
  serve them back.
- `owner` cannot currently be transferred/reassigned. If a program's
  original creator needs to hand off management, that requires a new
  contract function this pass didn't add.
- Storage/TTL/events follow the same conventions as the sibling
  scholarship contracts (persistent storage, ~1-year TTL bump on write).
