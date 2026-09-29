# scholarship-rate-limits

Contract: `scholarship-rate-limits`  
Version: 1  
Audited against: `Gen-x-academy/chainVerse-onchain` `origin/main` commit `82c681a1fbdf5984b4b6adaee50c053af21bbe09`

---

## Purpose

Protects public discovery, applications, uploads, invitations, messaging, and
payout actions with risk-based rolling-window rate limits keyed to stable actor
identities (`Address`), with explicit service-level authorization required for
High and Critical tier actions.

---

## Actions and default limits

| Action | Risk tier | Default window | Default max |
|---|---|---|---|
| Discovery | Low | 1 hour | 300 |
| Application | Medium | 24 hours | 10 |
| Upload | Medium | 24 hours | 20 |
| Invitation | Medium | 24 hours | 50 |
| Messaging | Medium | 1 hour | 100 |
| Payout | High | 24 hours | 5 |

All defaults are overridable by the admin via `set_action_config`.

---

## Rate-limit algorithm

1. Compute `window_id = floor(ledger_timestamp / window_seconds)`.
2. Read bucket `(actor, action, window_id)` from persistent storage.
3. If `count >= max_count`, emit `RATELIM` event with `window_reset` timestamp
   and return `RateLimitExceeded`. The `window_reset` value lets the caller
   compute the earliest retry instant without polling.
4. Otherwise increment and return `remaining`.

Windows roll automatically: each new `window_id` starts a fresh bucket. Old
buckets expire from ledger storage when their TTL elapses (~7 days), so no
explicit pruning is needed.

---

## High/Critical tier bypass

Actions with `RiskTier::High` or `RiskTier::Critical` require the actor to hold
a service-level authorization (`ServiceAuth`) granted by the admin. Without it
the call is rejected with `ServiceAuthRequired` before any bucket is read or
written. This makes bypasses explicit and auditable.

---

## Draft safety

`check_and_record` is the single gate point: it is called _before_ any
state-mutating operation in the invoking contract. A failed check (limit
exceeded or no service auth) returns an error without writing state, so the
caller's draft is never modified by a rejected rate-limit check.

---

## Stable identities

Limits are keyed by `Address` (Stellar public key). An actor cannot reset their
window by rotating an ephemeral identifier; the `Address` is the only identity
accepted.

---

## Storage

| Key | Storage tier | TTL |
|---|---|---|
| `Admin` | Instance | Contract lifetime |
| `ActionConfig(action)` | Instance | Contract lifetime |
| `BucketCount` | Instance | Contract lifetime |
| `ServiceAuth(address)` | Persistent | ~1–2 years |
| `Bucket(actor, action, window_id)` | Persistent | ~1 day / ~7 days |

---

## Bounds

- `MAX_BUCKETS = 500 000` total rate-limit buckets across all actors.
- Buckets are self-expiring (short TTL); the global count guards pathological
  creation before expiry kicks in.

---

## Events

| Symbol | Payload | When |
|---|---|---|
| `CFGSET` | `(action, tier, window, max)` | Config updated |
| `SVAUTGRT` | `(service)` | Service auth granted |
| `SVAUTRVK` | `(service)` | Service auth revoked |
| `SVAUREQ` | `(actor, action, window_reset)` | Service auth required (rejected) |
| `RATELIM` | `(actor, action, window_reset, used, max)` | Limit exceeded (rejected) |
| `RATEOK` | `(actor, action, used, remaining)` | Invocation recorded |

---

## Privacy

Only `Address` and numeric counters are stored. No action payloads, content,
or PII are written to this contract.

---

## Ownership

Admin controls tier config and service-auth grants. Per-action config is
updateable at any time; existing buckets for the old window are not purged (they
expire naturally). Config changes take effect for the _next_ bucket write.

---

## Migration

- New `Action` variants: add a config entry via `set_action_config`; no
  existing bucket keys are affected.
- Changing `window_seconds`: takes effect on the next `window_id` boundary.
  In-flight windows complete under the old config.

---

## Operational impact

- `check_and_record`: 1 instance read (config) + 1 persistent read (bucket) +
  1 persistent write. Suitable for on-chain hot paths.
- `current_usage`: 1 instance read + 1 persistent read; read-only, no auth.
