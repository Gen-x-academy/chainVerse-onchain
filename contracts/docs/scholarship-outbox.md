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

## Signed sponsor webhooks (#1141)

A sponsor wants to know when an award is reserved or paid without polling
the chain. That means an HTTP push, which means the chain is no longer
involved, and the honest question is what the contract can still guarantee
once it isn't.

### The trust boundary, stated plainly

**The chain never sends an HTTP request and never verifies an off-chain
signature.** Soroban has no sockets, and a webhook signature is produced by
a key the chain does not hold. So the on-chain half is deliberately limited
to what a contract *can* enforce:

| Property | Enforced |
| --- | --- |
| Exact authorization | Contract — `register_endpoint` needs the operator's **and** the owner's signature |
| "Selected events only" | Contract — `record_delivery` re-checks the subscription |
| Timestamp | Contract — `recorded_at` from the ledger, `expires_at` in the signed preimage |
| Ordering, gap-free | Contract — a sequence must be strictly the next one |
| Retryable | Contract — a retry is a new sequence and an incrementing `attempt` |
| Replay resistance, on-chain | Contract — sequence + expiry, both refused once stale |
| Replay resistance, at the sponsor | Signed preimage + the sponsor's own cursor |
| **Signature authenticity** | **Off-chain** — the sponsor's HTTP handler |
| **Did it actually arrive** | **Off-chain** — the relay's claim is not receipt |

The last two rows are the ones a reader is most likely to assume are
covered. They are not, and no amount of on-chain code makes them so. A
sponsor **must** verify the HMAC in its handler and **must** track the
highest sequence and `expires_at` it has accepted. What this contract
provides is the means to do both correctly: a canonical byte string to
verify against, and a cursor that makes a replay visible.

### Enrollment is bilateral

`register_endpoint` requires the operator's auth **and** the owner's.
Neither the platform alone nor a sponsor alone can enroll an endpoint. A
compromised platform key therefore cannot silently start shipping one
sponsor's award events to an attacker's URL, and a sponsor cannot be
enrolled without its key.

Endpoints are identified by `sha256(url)` and authenticated by
`sha256(secret)`. **Neither the URL nor the secret is ever stored.** A
webhook URL is frequently a bearer capability, and the secret is the thing
an attacker wants; the chain holds only commitments, and the relay and
sponsor hold the values.

### Secret rotation is a preimage field, not a revocation list

`rotate_secret` bumps `secret_epoch`, which is part of the signed preimage.
A signature captured under the old secret stops verifying the moment the
epoch is bumped — the sponsor recomputes the preimage from the endpoint it
just read on-chain and gets a different digest. **Leaking a secret is a
one-transaction fix**, and there is no revocation list to distribute and
nothing for a sponsor to remember to invalidate. Rotation works on a live
endpoint, since rotating a working secret is the normal case.

### Two network-bound values the caller does not supply

`delivery_payload` takes the network id and the contract address from the
environment, not as parameters. A caller-supplied network id would put the
cross-network defence in the caller's hands; deriving both means a testnet
signature cannot be minted on mainnet at all, and a signature for one
deployment is not valid against another. `delivery_payload` is
unauthenticated and read-only on purpose: a sponsor verifying an incoming
delivery recomputes the expected digest from the chain alone, without
holding the secret. That is what makes the secret prove *possession*
instead of being the only source of truth.

`webhook_signing.rs` is a separate domain from the library's
`shared::signing` envelope — different tag, different layout. Reusing the
library envelope would have made a library signature a valid webhook
signature; `cross_domain_signatures_differ` pins that separation down with a
test.

### Selection is enforced, not trusted

`deliverable` reports whether an endpoint should receive an event, and
`record_delivery` re-checks the subscription rather than trusting the
relay's filter. An applicant submission must not reach a sponsor that never
asked for one, and if the only thing standing between them was the relay's
own good behaviour, that would be a one-line bug away from a disclosure.

The single exception is `DeliveryOutcome::Skipped`: a relay recording that
it deliberately did *not* send an out-of-subscription event. That outcome
is the one allowed to name an unsubscribed event, because otherwise the
decision would be invisible and the sponsor could not tell "nothing
happened" from "you were filtered out". It still advances the sequence, so
the gap-free ordering is preserved. Anything claiming to have been sent
must be subscribed.

### Relay claims vs. arrival

`record_delivery` records that the relay *tried*. `acknowledge_delivery`
records that something *arrived*, and only the endpoint's owner can call it.
The distinction is the point: a relay is not trusted to report its own
success. Acknowledgements are strictly increasing, so a replayed delivery
presented twice is refused with `ReplayDetected`.

### Bounds

| Bound | Value | Consequence past it |
| --- | --- | --- |
| Enrolled endpoints | 100 | `TooManyEndpoints` |
| Topics per endpoint | 16 | `TooManyTopics` |
| Delivery records per endpoint | 256 | oldest evicted, `delivery_history_floor` advances |
| Signature window | 24 h | `InvalidExpiry` |

The attempt counter per `(endpoint, event)` is a **separate storage key**,
so it survives history eviction: after the oldest record is dropped, a
sponsor can still see that an event was attempted 257 times. Eviction only
loses the per-attempt detail, never the retry count.

Endpoint and delivery state get the longer retention window (~10 weeks,
extended from ~5 weeks) because both are read on every relay cycle and by
every sponsor. A delivery record outliving its event is expected: the
history is the audit trail, and it stays readable after the event it
describes has been pruned.

## Operational impact

- **The relay is now a two-phase actor per endpoint.** Read
  `delivery_payload`, sign, POST, then `record_delivery`. A crash between
  the POST and the record leaves the sponsor with a delivery the chain does
  not know about; the next cycle re-signs the same sequence, the sponsor's
  replay check rejects it, and the relay records the retry. The
  acknowledgement, not the record, is what tells you it landed.
- **Sequence exhaustion is not a concern**, but `expires_at` churn is: the
  relay must re-read the endpoint each cycle, because a rotation changes
  the expected preimage underneath it. Signatures signed against a cached
  epoch fail verification, which is the intended behaviour but looks like a
  bug if the relay caches too long.
- **Rotating a secret does not pause delivery.** Signatures produced under
  the old epoch stop verifying immediately, so a rotation should be
  coordinated with the relay — but it does not have to be, because a
  failed delivery is a `Failed` record and a retry, not a lost event.
- **History is a window, not a ledger.** Past 256 records per endpoint the
  detail is gone. For a longer audit trail, export on-chain; do not expect
  to page back indefinitely.

## Settlement reconciliation (#1142)

A backend has an *opinion*: a payment or award intent, and an amount it
believes it is settling. The chain has a *fact*: a transaction that settled,
in a ledger, for an amount. This section reconciles the two, and the guiding
rule is that **the opinion never wins**.

### Finality is explicit, per intent, and zero is not allowed

A Stellar transaction reported as successful is not final. The ledger
containing it can still be rolled back, a fee-bump can change the outcome,
and a later close can settle it differently. Reconciling on "the backend
said it succeeded" is the bug this contract exists to prevent.

So each intent stores its own `required_finality`, and reconciliation
requires `current_ledger - observed_ledger >= required_finality`. It is
per-intent rather than a global constant because the right answer depends on
the operation: a scholarship disbursement and a rounding adjustment do not
deserve the same confidence.

Two bounds matter:

- **`required_finality == 0` is refused.** "Final immediately" is exactly
  the failure mode being guarded against, so it must not be expressible.
- **`required_finality > MAX_FINALITY` (100 ledgers) is refused.** Otherwise
  a caller can pass a threshold that makes an intent permanently
  unreconcilable. The failure mode of an unbounded window is *silence*,
  which is worse than a late answer.

`observe_settlement` also refuses a transaction claimed to be in ledger zero
or in a ledger that has not happened yet, since either would let the
finality check pass for a transaction that does not exist.

### Reprocessing is a complete no-op

Once an intent is decided, `reconcile_settlement` returns its state and
writes **nothing at all** — not the state, not the amount, not the timestamp,
not `attempts`. The tests assert whole-record equality rather than
spot-checking fields, because a partial guarantee here is not a guarantee.

That is what makes a backend retry loop, a duplicated webhook, or a
re-delivered event harmless. `attempts` counts *adjudications* rather than
calls, so a backend spinning while an intent is still pending is visible in
the record, while post-settlement retries leave no trace at all.

### A mismatch is a fact, not a phase

`Mismatched` is **never cleared by re-running**. The observed transaction is
immutable, so the disagreement is too; only `resolve_settlement` moves it,
with the operator's reason committed by hash.

This is the property that makes the alert trustworthy. If a re-run with the
"correct" amount could clear a mismatch, the alert would be suppressible by
the very backend that caused it. The same reasoning makes the second
observation immutable:

> `observe_settlement` is idempotent for the same transaction and
> **refuses a different one**. If a second observation could replace the
> first, a backend could swap in a transaction that happens to agree with its
> intent and make a real mismatch vanish.

### The UI cannot render a pending intent as settled

`settlement_status` returns `settled_amount` **only** when the state is
`Reconciled`, and zero otherwise. A pending or disputed intent has no
settled figure to render, so "not yet settled" cannot be shown as money the
sponsor received by forgetting a branch. `observed_amount` stays available
for an operator reconciling by hand — the two sides are both needed — but it
is a different field with a different name.

`settled_amount` goes further and *refuses to answer* for an unreconciled
intent (`InvalidState`), so a caller wanting a number has to handle the error
rather than receive a zero it might display as "nothing was paid".

### Mismatches alert

Two states page somebody: `Mismatched` (the chain disagrees) and `Expired`
(the transaction never appeared). Both are "the backend and the chain
disagree", and neither resolves on its own.

`alert_count()` is a running total that only rises on an alert and only falls
in the sense that a monitor can compare it against resolved intents. It exists
so alerting does not depend on someone tailing every reconciliation topic, and
so a rising number is unambiguous.

`expire_settlement` is separate from a mismatch on purpose: "no transaction"
is usually a backend that failed to submit, while "the wrong amount" is a
real dispute, and they need different responses.

### Bounds

| Bound | Value | Consequence past it |
| --- | --- | --- |
| Tracked intents | 5,000 | `TooManyIntents` |
| Finality threshold | 1–100 ledgers | `InvalidFinality` |

`archive_settlement` is the release valve, and it accepts **only**
`Reconciled` intents. It is refused for `Pending`, `Mismatched`, and
`Expired` so the valve cannot double as a way to erase an open dispute in
order to free a slot. The archival itself emits an event, so an auditor can
see that an intent existed and was removed even though it no longer does.
Past 5,000 unsettled intents, the correct response is to fix the backlog, not
to archive the disputes.

## Settlement: operational impact

- **Three phases per intent, and the backend drives all three.** `open`, then
  `observe` with the transaction hash and ledger, then `reconcile` with the
  amount once the finality window has passed. `reconcile` before that returns
  `NotFinal` — the backend must retry on a later ledger, not immediately.
- **The backend must not treat a successful submission as a settlement.** It
  is a `Pending` intent until `reconcile_settlement` returns `Reconciled`.
  This is the operational change most likely to surface existing bugs, and it
  is the point of the issue.
- **Alert on `alert_count()`, and expect it to be non-zero occasionally.** A
  mismatch is the system working. An `alert_count()` that never moves across a
  busy period is more suspicious than one that does.
- **A mismatch cannot be auto-cleared.** Any automation that retries
  `reconcile` to "fix" a mismatch will keep getting `Mismatched` back, which
  is correct. Clearing one is a human decision via `resolve_settlement`, and
  it is recorded.
- **Reconciliation is per-intent and idempotent, so replaying a whole
  settlement batch is safe.** That is deliberate: it makes a
  re-drive-after-deploy a non-event rather than a source of duplicate alerts.
