# Indexer Projection Schema

- Status: Implemented (scoped to `royalty-splits` — see Scope)
- Owner: E-Library On-chain / Indexer working group
- Related issue: #1019 (define deterministic projection schemas)

## Scope

#1019 asks for a schema mapping "each versioned event to idempotent
database mutations and checkpoint behavior" across the E-Library domain's
catalog, loan, hold, and finance UIs. That domain spans many existing
contracts (`library_licensing`, `library-fines`, `library-rights`, and
others) whose events were not documented with this level of rigor before
now. Rather than retrofit a schema onto all of them speculatively in one
pass, this document defines the *pattern* every projection in this
codebase should follow, and applies it concretely and completely to the
one contract added alongside it in this same PR: `royalty-splits`
(#993). Extending this same pattern to the existing catalog/loan/hold
contracts' events is scoped follow-up work, listed at the end.

## The pattern

Every projection table in this codebase should follow four rules:

1. **Composite idempotency key.** Every mutation a projector applies is
   keyed by `(ledger_sequence, tx_index, event_index)` (or an equivalent
   unique per-event coordinate the RPC/indexing pipeline provides) in
   addition to whatever business key the row itself has. Re-applying the
   same event (a retry, an at-least-once delivery redelivery, or a
   reorg-like re-fetch of the same ledger range) must be a no-op the
   second time — `INSERT ... ON CONFLICT (idempotency_key) DO NOTHING` or
   equivalent, never a blind `INSERT`/`UPDATE`.
2. **Replay-from-genesis determinism.** A projection's state after
   processing events `[0..N]` must be identical whether those events were
   applied one at a time as they arrived, or all at once in a single
   replay from ledger 0. This means: never use wall-clock "now" in a
   projection (use the event's own ledger timestamp), never depend on
   processing order across *different* topics arriving out of order
   (each row's fields should only ever be written from one event topic),
   and never accumulate via a non-commutative operation without the
   idempotency key protecting against double-application.
3. **Checkpointing.** The indexer persists the last successfully
   processed `(ledger_sequence, tx_index, event_index)` per contract
   (or globally) in the same transaction as the row mutations it protects
   — so a crash between "wrote projection rows" and "advanced checkpoint"
   is recoverable by re-processing from the last checkpoint, which rule 1
   makes safe.
4. **Explicit privacy exclusions.** A projection table only ever contains
   fields already public on-chain (see each contract's own "Privacy"
   section in `contracts/docs/`). A projector must never join against or
   store off-chain PII to "enrich" a row — enrichment, if needed, happens
   in a separate, access-controlled system, not in the public projection
   this schema describes.

## `royalty-splits` projections

### `royalty_manifests`

Projected from the `SPLITNEW` event (`(manifest_id: BytesN<32>)`), joined
with a `get_manifest` contract read to populate `recipients`/`bps` (the
event itself only carries the id — see Notes).

| Column | Type | Source |
|---|---|---|
| `manifest_id` (PK) | `bytea` | event topic payload |
| `recipients` | `jsonb` (array of addresses) | `get_manifest(manifest_id).recipients` |
| `bps` | `jsonb` (array of integers) | `get_manifest(manifest_id).bps` |
| `created_at` | `timestamptz` | `get_manifest(manifest_id).created_at`, converted from the ledger-time `u64` |
| `first_seen_ledger` | `bigint` | the event's ledger sequence |

Idempotency: `manifest_id` is already the natural key, and the contract
itself guarantees a `manifest_id` is only ever created once
(`ManifestAlreadyExists` otherwise) — so this table's upsert is
naturally idempotent even without the composite event key, though the
projector should still record it for consistency with every other table.

### `royalty_distributions`

Projected from the `SPLITDST` event
(`(manifest_id: BytesN<32>, asset: Symbol, amount: i128)`).

| Column | Type | Source |
|---|---|---|
| `event_key` (PK) | composite `(ledger_sequence, tx_index, event_index)` | event coordinate |
| `manifest_id` | `bytea` | event payload |
| `asset` | `text` | event payload |
| `amount` | `numeric` | event payload |
| `ledger_sequence` | `bigint` | event coordinate |
| `occurred_at` | `timestamptz` | the ledger's close time |

This table is append-only — one row per `distribute()` call, ever. It is
the audit trail for "how much of what asset was distributed under which
manifest, and when," matching the on-chain `AssetLedger.total_distributed`
counter (which is a *sum*, not a *log* — this table is the log).

### `royalty_balances`

Projected from the same `SPLITDST` events (to credit) and `SPLITWD`
events (to debit) — this is the one table in this schema that is *not*
simply append-only, since it mirrors the contract's own mutable
`Balance(recipient, asset)` storage.

| Column | Type | Source |
|---|---|---|
| `recipient` (PK part 1) | `text` (address) | derived: for `SPLITDST`, look up `get_manifest(manifest_id).recipients`/`bps` and recompute each recipient's share using the *same* deterministic-rounding rule as the contract (see `royalty-splits.md`) |
| `asset` (PK part 2) | `text` | event payload |
| `balance` | `numeric` | running total: `+= share` on `SPLITDST`, `-= amount` on `SPLITWD` |
| `last_event_key` | composite | last event's coordinate, for idempotency (rule 1) |

**Notes / an explicit gap this schema surfaces:** `SPLITDST`'s payload is
`(manifest_id, asset, amount)` — it does not carry the per-recipient
share breakdown the contract computed internally. A projector can
recompute those shares deterministically (rule 2 guarantees this is safe
— the rounding rule is pure and documented), but this requires the
projector to embed the *same* rounding logic as the contract, which is a
duplication risk if the two ever drift. **Recommended follow-up:** either
have `distribute()` emit one event per recipient share (trading a larger
event payload for no recomputation), or accept the duplication and add a
contract-conformance test in the indexer that replays a fixed set of
distributions and asserts its computed balances match `get_balance()` via
a live contract call, catching drift immediately. This document takes no
position on which; it exists to make the tradeoff visible, per #1019's
own scope.

### `royalty_withdrawals`

Projected from the `SPLITWD` event
(`(recipient: Address, asset: Symbol, amount: i128)`). Append-only, same
shape as `royalty_distributions`.

## Follow-up: extending this pattern

The existing `library_licensing`/`library-fines`/`library-rights`
contracts' events (documented in `contracts/docs/events.md`) are not yet
covered by a schema this rigorous. Applying rules 1–4 above to each of
their events is the recommended next step for fully closing #1019 across
the whole E-Library domain — this document's job was to establish and
prove out the pattern on one contract end-to-end, not to retrofit every
existing one in the same pass.
