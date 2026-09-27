# scholarship-payment-anomaly

Soroban smart contract for detecting and safely pausing suspicious scholarship
disbursements — including unusual destinations, rapid wallet changes, repeated
failures, duplicate ledger references, and suspicious splits — with audited
human resolution.

Audited against **Gen-x-academy/chainVerse-onchain origin/main commit 82c681a1**.

---

## Purpose

Scholarship disbursements are high-value, irreversible transactions. Automated
anomaly detection reduces the window of exposure for fraud or misdirection.
This contract provides:

- An on-chain audit trail of every anomaly alert, with evidence references.
- A safe-pause mechanism: disbursement contracts check `is_payment_paused`
  before executing, giving the resolver time to investigate.
- Audited resolution: every clear or confirm action records who acted, when,
  and why — via both on-chain events and structured storage.
- Global counters (`total_alerts`, `total_paused_amount`) for operational monitoring.

---

## Safe-pause design

This contract **never holds funds**. It is a coordination layer. The pause
mechanism works as follows:

1. Detector raises an alert with `pause_payment = true`.
2. The disbursement contract (or off-chain service) calls
   `is_payment_paused(payment_ref)` before executing.
3. If `true`, the disbursement is halted pending resolution.
4. Resolver calls `resolve_alert(..., Cleared, notes)` — `is_payment_paused`
   returns `false` from that point.

The pause has an optional expiry (`pause_expires_at`). If the resolver does not
act within the window, `is_payment_paused` automatically returns `false` when
queried after expiry. The default window is 72 hours.

**The disbursement contract or off-chain service is responsible for enforcing
the pause.** This contract cannot block on-chain token transfers directly.

---

## Ownership

| Role | Stored as | Responsibilities |
|---|---|---|
| `admin` | instance storage | Governance: pause contract, rotate detector/resolver, upgrade |
| `detector` | instance storage | Trusted service account that raises anomaly alerts. Should be automated monitoring infrastructure. |
| `resolver` | instance storage | Human investigator who clears or confirms alerts. Every resolution is audited. |

---

## Anomaly categories

| Variant | Description |
|---|---|
| `UnusualDestination` | Destination wallet is newly registered, in a high-risk jurisdiction, or matches a watchlist |
| `RapidWalletChange` | The recipient's destination address changed recently before disbursement |
| `DuplicateLedgerReference` | Multiple disbursements reference the same ledger sequence number or transaction hash |
| `SuspiciousSplit` | Disbursement split into many sub-transactions in a structuring/smurfing pattern |
| `RepeatedFailure` | Repeated transaction failures before success, suggesting front-running or manipulation |
| `Other` | Catch-all |

---

## Alert lifecycle

```
Detector raises alert (pause_payment = true or false)
  → status: PausedPendingReview

  ┌─ Resolver acts ───────────────────────────────────────────────┐
  │  resolve_alert(..., Cleared, notes)                           │
  │    → status: Cleared, payment_paused = false                  │
  │    → total_paused_amount decremented                          │
  │                                                               │
  │  resolve_alert(..., Confirmed, notes)                         │
  │    → status: Confirmed, payment remains paused                │
  │    → total_paused_amount unchanged                            │
  └───────────────────────────────────────────────────────────────┘

  OR

  pause_expires_at reached (time-based expiry)
  → is_payment_paused returns false automatically
  → alert status remains PausedPendingReview (resolver should still act)
```

`Cleared` and `Confirmed` are **terminal states**. A resolved alert cannot
be re-resolved.

---

## Entry points

### Lifecycle

| Function | Caller | Description |
|---|---|---|
| `init(admin, detector, resolver)` | `admin` | One-time initialisation. |
| `upgrade(caller, new_wasm_hash)` | `admin` | WASM upgrade. |

### Alert management

| Function | Caller | Description |
|---|---|---|
| `raise_alert(caller, payment_ref, category, description, evidence_ref, amount, destination, pause_payment, pause_duration_secs, wallet_change_velocity, retry_count)` | `detector` | Raise an alert. `payment_ref` must be unique. `amount` must be ≥ 0. `pause_duration_secs = 0` uses the default 72 h window. |
| `resolve_alert(caller, payment_ref, resolution, notes)` | `resolver` | Resolve to `Cleared` or `Confirmed`. `notes` is mandatory and must reference the off-chain investigation report for `Confirmed`. |

### Queries

| Function | Returns |
|---|---|
| `is_payment_paused(payment_ref)` | `bool` — canonical pause state for disbursement contracts to check |
| `get_alert_summary(payment_ref)` | `AlertSummary` (compact public view) |
| `get_alert_full(payment_ref)` | `PaymentAlert` (full record — gate to authorised reviewers) |
| `total_alerts()` | `u64` |
| `total_paused_amount()` | `i128` |
| `paused()` | `bool` |

### Admin operations

| Function | Caller |
|---|---|
| `pause(caller)` | `admin` |
| `unpause(caller)` | `admin` |
| `set_detector(caller, new_detector)` | `admin` |
| `set_resolver(caller, new_resolver)` | `admin` |

---

## `is_payment_paused` logic

Returns `true` if **all** of the following hold:

1. An alert exists for the given `payment_ref`.
2. `alert.payment_paused == true`.
3. `alert.status == PausedPendingReview`.
4. `pause_expires_at == 0` OR `current_timestamp <= pause_expires_at`.

Returns `false` otherwise — including for unknown `payment_ref` values.

---

## Evidence requirements

| Alert type | Evidence must include |
|---|---|
| `UnusualDestination` | Source of watchlist match or jurisdiction data |
| `RapidWalletChange` | Timestamps and addresses of prior wallet(s) |
| `DuplicateLedgerReference` | Both transaction references |
| `SuspiciousSplit` | List of sub-transaction references and amounts |
| `RepeatedFailure` | Failure log references |

`evidence_ref` must be a content-addressed pointer (IPFS CID, SHA-256 hex of
an evidence bundle). Alerts without evidence references are stored but resolvers
should treat them as lower confidence.

---

## Storage layout

| Key | Class | TTL | Description |
|---|---|---|---|
| `DataKey::Admin` | instance | contract instance TTL | Admin address |
| `DataKey::Paused` | instance | contract instance TTL | Pause flag |
| `DataKey::Detector` | instance | contract instance TTL | Detector address |
| `DataKey::Resolver` | instance | contract instance TTL | Resolver address |
| `DataKey::Alert(String)` | persistent | ~1–2 years | Full `PaymentAlert` per payment reference |
| `DataKey::TotalAlerts` | persistent | ~1–2 years | Monotonic alert counter |
| `DataKey::TotalPausedAmount` | persistent | ~1–2 years | Running total of paused amounts |

---

## Events

| Topic | Data | Emitted by |
|---|---|---|
| `PA_INIT` | `(admin, detector, resolver)` | `init` |
| `PA_RAIS` | `(payment_ref, category, amount, destination, pause_payment, pause_expires_at, caller, timestamp)` | `raise_alert` |
| `PA_RESV` | `(payment_ref, resolution, caller, timestamp)` | `resolve_alert` |
| `PA_PAUS` | `caller` | `pause` |
| `PA_UPAU` | `caller` | `unpause` |
| `PA_SDET` | `(caller, new_detector)` | `set_detector` |
| `PA_SRES` | `(caller, new_resolver)` | `set_resolver` |
| `PA_UPGD` | `new_wasm_hash` | `upgrade` |

---

## Field size limits

| Field | Limit |
|---|---|
| `payment_ref` | 64 bytes |
| `description` | 512 bytes |
| `evidence_ref` | 256 bytes |
| `resolution_notes` | 1 024 bytes |
| `amount` | i128 ≥ 0 |

---

## Privacy design

1. **Destination addresses are stored for audit only.** The `destination` field
   in `PaymentAlert` records the recipient address so resolvers can investigate,
   but this field must not be used to build profiles or cross-reference
   individuals across alerts.

2. **Full records gated off-chain.** `get_alert_full` is a public read-only
   call on-chain (Soroban has no native read access control). The off-chain
   layer must restrict who can call this endpoint to authorised investigators.

3. **No applicant PII in string fields.** `description`, `evidence_ref`, and
   `resolution_notes` must not contain applicant names, government IDs, or
   sensitive personal data. Reference off-chain evidence by CID.

---

## Migration

Alert records are keyed by `payment_ref`. If the payment reference scheme changes:

1. New alerts use new reference format.
2. Old alerts remain accessible under old references.
3. `TotalAlerts` and `TotalPausedAmount` are cumulative across both schemes.

If `PaymentAlert` gains new fields via a contract upgrade, existing records will
surface default/zero values for new fields until explicitly updated.

**Breaking ABI changes** must be listed in `.github/scholarship-compat-override.json`.

---

## Operational runbook

### High-risk payment alert raised

1. Disbursement service calls `is_payment_paused(payment_ref)` — returns `true`.
2. Disbursement is halted. Ops team is notified via `PA_RAIS` event.
3. Resolver calls `get_alert_full(payment_ref)` and reviews the evidence bundle.
4. If legitimate: `resolve_alert(..., Cleared, "Investigation complete: <report-cid>")`.
5. If fraudulent: `resolve_alert(..., Confirmed, "Escalated per SOP-ANOMALY-001: <report-cid>")`.
   Disbursement remains blocked. Escalate to compliance.

### Pause window expiry

If a resolver cannot act within 72 hours (or the configured window):

- `is_payment_paused` returns `false` automatically.
- The alert status remains `PausedPendingReview`.
- Ops must re-raise a new alert or extend the investigation off-chain.
- **No automatic re-raise** — this prevents indefinite payment blocks from
  stale alerts.

---

## Operational impact

- **Contract pause** (`pause`): Blocks `raise_alert` and `resolve_alert`.
  Queries (`is_payment_paused`, `get_alert_summary`, etc.) still work.
  Use only for emergency maintenance.
- **Detector rotation**: New detector takes effect immediately. Alerts already
  raised by the old detector remain valid.
- **Resolver rotation**: New resolver takes effect immediately. Pending
  alerts can be resolved by the new resolver.

---

## Security properties

- `require_auth()` called before any storage access on all mutating calls.
- `amount < 0` returns `InvalidAmount` — no negative-amount exploits.
- `AlertAlreadyResolved` prevents double-resolution of terminal-state alerts.
- `resolve_alert` only accepts `Cleared` or `Confirmed` — other statuses
  return `Unauthorized`.
- `TotalPausedAmount` uses `checked_add`/`checked_sub` — overflow returns
  `ArithmeticOverflow`.
- `TotalAlerts` uses `saturating_add` — cannot overflow at realistic scale.
- Pause expiry is evaluated at query time — no background job needed.
