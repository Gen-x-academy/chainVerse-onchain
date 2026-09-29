# scholarship-idempotency

Contract: `scholarship-idempotency`  
Version: 1  
Audited against: `Gen-x-academy/chainVerse-onchain` `origin/main` commit `82c681a1fbdf5984b4b6adaee50c053af21bbe09`

---

## Purpose

Provides safe retry semantics for submissions, decisions, acceptance,
milestones, payouts, refunds, and notifications by binding an idempotency key
to a `(actor, operation)` pair and enforcing that concurrent or duplicate
retries produce exactly one outcome.

---

## Key lifecycle

```
create_key(actor, operation, key, ttl)
       │
       ▼ KeyStatus::Pending  ──────────────── duplicate: KeyInFlight
       │
       ├── complete_success(actor, operation, key, result_hash)
       │          │
       │          ▼ KeyStatus::Succeeded  ── further writes: KeyAlreadyCompleted
       │
       └── complete_failure(actor, operation, key, result_hash)
                  │
                  ▼ KeyStatus::Failed  ──── further writes: KeyAlreadyCompleted
```

A `Pending` key blocks concurrent execution — callers must wait and retry.  
`Succeeded` and `Failed` are terminal and immutable.  
A completed key blocks a new `create_key` with the same bytes → `KeyAlreadyCompleted`.

---

## Actor binding

The storage key is `(actor, operation_symbol, key_bytes)`. A key created by
actor A for operation X cannot be seen, completed, or interfered with by actor
B or under operation Y, even if they use identical `key_bytes`. This is the
structural cross-actor isolation.

---

## Operations

| Variant | Symbol stored |
|---|---|
| Submission | `"submission"` |
| Decision | `"decision"` |
| Acceptance | `"acceptance"` |
| Milestone | `"milestone"` |
| Payout | `"payout"` |
| Refund | `"refund"` |
| Notification | `"notification"` |

---

## Storage

| Key | Storage tier | TTL |
|---|---|---|
| `Admin` | Instance | Contract lifetime |
| `Record(actor, op, key)` | Persistent | `KEY_MIN_TTL` / `KEY_MAX_TTL` (6 h / 90 d) |
| `PendingCount(actor, op)` | Persistent | Same as above |

Records are extended at every write. After the TTL the ledger may evict the
record; a `get_record` on an evicted key returns `KeyNotFound` (same as
never-created).

---

## Bounds

- `MAX_KEY_TTL_SECONDS = 7 776 000` (90 days) — hard ceiling on retention.
- `DEFAULT_KEY_TTL_SECONDS = 86 400` (24 hours) — used when caller passes `0`.
- `MAX_PENDING_PER_ACTOR_OP = 32` — caps concurrent in-flight keys per
  (actor, operation) pair to prevent unbounded `PendingCount` growth.
- All arithmetic is checked.

---

## Concurrent retry guarantee

Soroban invocations are serialized by the ledger. Two workers racing to call
`complete_success` on the same key will have their calls sequenced: the first
write succeeds, the second reads `KeyAlreadyCompleted`. The record always
reflects exactly one outcome.

---

## Events

| Symbol | Payload | When |
|---|---|---|
| `IDMPNEW` | `(actor, op, key)` | Key created (Pending) |
| `IDMPOK` | `(actor, op, key)` | Key completed as Succeeded |
| `IDMPFAIL` | `(actor, op, key)` | Key completed as Failed |

---

## Privacy

Only `Address`, `Symbol` (operation name), `BytesN<32>` key, and numeric
metadata are stored. No payload, answer, or private data is ever written.
Callers must derive the key off-chain using a secure hash of their own content.

---

## Ownership

The contract admin can transfer admin via `transfer_admin`. No other admin-only
functions exist in v1 — all key operations are actor-self-service.

---

## Migration

- New `Operation` variants: add the `as_symbol` mapping; no existing keys
  are affected.
- Key retention policy changes: update `MAX_KEY_TTL_SECONDS` and rebuild.
  Existing keys with shorter TTLs are unaffected until they naturally expire.

---

## Operational impact

- `create_key`: 2 persistent reads + 2 persistent writes (record + count).
- `complete_success` / `complete_failure`: 1 persistent read + 2 persistent
  writes (record + count decrement).
- `is_in_flight` / `is_succeeded`: 1 persistent read; no auth, suitable for
  on-chain guards.
