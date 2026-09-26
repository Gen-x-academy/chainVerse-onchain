# Scholarship Outbox Contract

- Status: Implemented
- Owner: Scholarships On-chain working group
- Related issue: #1140 (transactional scholarship domain events), #1056
  (platform boundaries — see
  `docs/adr/0002-scholarships-on-chain-boundaries.md`)

## Scope

This contract (`contracts/scholarship-outbox`) is the durable,
transactional half of the platform's off-chain event boundary.

ADR 0002 puts orchestration off-chain and forbids the five scholarship
contracts from calling each other. That leaves an awkward failure mode:
the state write and the "tell the outside world" step are two different
systems, so a crash between them either loses an event or emits one for a
write that rolled back. Consumers — a sponsor dashboard, a finance
reconciler, an analytics indexer — then see state and history disagree,
with no way to tell which one is lying.

This contract closes that gap. It stores event *entries*: a topic, a
schema version, a correlation id, a payload commitment, and the address of
the domain contract whose state changed.

### How atomicity is actually obtained

The five contracts cannot call this one, and this contract does not call
them. Atomicity comes from the transaction envelope instead: the
orchestrator submits **one** Stellar transaction containing an invocation
of the domain contract *and* an `enqueue` invocation here. Stellar
transactions are atomic, so both land or neither does.

The alternative — having each of the five contracts call the outbox
directly — was rejected. It would add five cross-contract edges to a
platform whose defining constraint is that it has none, and it would put
the atomicity guarantee inside each contract rather than in the one place
that can see the whole transaction.

## Event schema

| Field | Meaning |
| --- | --- |
| `id` | Monotonic, gapless, starts at 1. Never 0. |
| `topic` | Coarse event name, e.g. `award_reserved`. A `Symbol`, so length-bounded by the SDK. |
| `schema_version` | Consumer-facing payload version. `0` is refused. |
| `correlation_id` | The domain object that caused the event, so a consumer can group a program's events without parsing payloads. |
| `payload_hash` | `sha256` of the off-chain payload. **The payload is never stored.** |
| `source` | Address of the domain contract whose state changed. Recorded for provenance, not used for authorization. |
| `created_at`, `published_at` | Ledger timestamps. `published_at == 0` means undelivered. |

Consumers branch on `schema_version` rather than inferring a shape. Adding
a field is a new version, not a silent change: an immutable
`dedup_key` → `event_id` mapping outlives the event it points at, so a
consumer can still tell "already seen this" after the event itself has
been pruned.

## Deduplication

Two independent mechanisms, because the two sides fail differently:

- **Producers.** `enqueue` takes a caller-chosen `dedup_key`. Re-enqueueing
  the same key returns the original id and writes nothing — no new id, no
  pending-count change, and no failure even when the backlog is full. An
  orchestrator retrying after an ambiguous failure cannot double-emit.
  The key wins over every other field, so a retry that changed a topic or
  version still deduplicates.
- **Consumers.** Each consumer has a cursor: the highest event id it has
  acknowledged. The cursor only moves forward, and only to an id that
  exists. A redelivered batch re-acknowledges the head and is a no-op, so
  a retrying consumer is never wedged.

The cursor's two restrictions are load-bearing. A cursor that could go
backwards would let a consumer reprocess a batch it had already acked; one
that could jump to a non-existent id would let a consumer park beyond the
backlog and silently skip everything.

## Privacy

The outbox stores no applicant data. `OutboxEvent` has no field a name, an
email, or a free-text answer could be smuggled into — an id, a topic, a
version, two 32-byte ids, a hash, an address, and two timestamps. Form
answers, applicant names, and award documents stay off-chain; the chain
holds only the hash a consumer uses to confirm it received the right
bytes. `source` is a contract address, not a person.

This makes ADR 0002's privacy claim checkable rather than aspirational: a
reviewer can read the struct and confirm there is nowhere for PII to go.

## Ownership

A single operator (`initialize`) is the only account that can `enqueue`,
`mark_published`, or `prune`. That is deliberate rather than a shortcut:
the operator is the only party that can place a domain-contract invocation
and an `enqueue` invocation in the same transaction, so a per-publisher
role registry would gate a capability the contract cannot verify anyway.
The trade-off is that this contract is single-tenant; a multi-tenant
deployment would need a per-`source` registry, which is a non-goal here.

Consumers need no role at all — a cursor is keyed by the consumer's own
address and guarded by that address's `require_auth`.

## Migration

New contract, no prior on-chain state, so nothing to migrate *into*.

Migrating *off* a deployment means pointing the relay at the new address
and letting the old one age out. Two properties make that safe:

- The `dedup_key` → `event_id` mapping has the same TTL as the event, so a
  relay that fails over mid-window and retries an enqueue against the new
  contract will double-emit — **dedup keys are per-deployment.** Scope them
  with a deployment id if a failover is expected.
- Cursors are per-consumer and per-deployment for the same reason.

## Operational impact

- **The backlog is the health signal.** `pending_count()` is how far the
  relay has fallen behind. It only grows if the relay stops, and it is
  bounded: at `MAX_PENDING_EVENTS` (10,000) `enqueue` starts returning
  `BacklogFull` rather than growing without limit. **That is a deliberate
  fail-loud choice** — once the backlog is full, the orchestrator's
  transactions start failing, which is visible, rather than the chain
  quietly absorbing an unbounded queue. Alert on a rising
  `pending_count()`, well before the cap.
- **`mark_published` is the relay's, and only the relay's.** It clears an
  entry from the backlog and is refused a second time (`AlreadyPublished`)
  rather than being idempotent, because a silent no-op would let a buggy
  relay decrement the pending count twice and report a healthy backlog
  while entries pile up.
- **Pruning is incremental and never drops undelivered events.**
  `prune(horizon, limit)` starts from a stored floor rather than from 1,
  so repeated calls do not rescan history, and it is capped at 200 entries
  per call so it cannot be used to burn unbounded gas. It stops at the
  first undelivered entry rather than skipping past it, so the floor never
  overtakes a pending event. A full `limit` back means there is more to do.
  Call it on a schedule, not in a user-facing path.
- **A future `horizon` is refused** (`InvalidRetentionHorizon`): pruning to
  a time consumers have not reached yet would delete events they have not
  had a chance to read.
- **Nothing here is a delivery guarantee.** The chain can prove an event
  was enqueued and that a relay claimed to publish it. It cannot prove the
  consumer received or processed it — that is the consumer cursor's job,
  and it is the consumer's `require_auth`, not the relay's claim, that
  establishes it. Any consumer that needs end-to-end delivery guarantees
  must track its own cursor and treat its own position as the truth.
- **Cross-contract reads are still the orchestrator's job.** This contract
  records `source` for provenance but never verifies that the named
  contract's state actually changed. An event's existence is proof that
  the operator enqueued it in a transaction; correlating it against a
  domain contract's state is the orchestrator's responsibility, exactly as
  ADR 0002 describes.
