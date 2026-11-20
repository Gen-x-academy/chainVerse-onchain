# Capstone Research Theme Subscriptions Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-031] Add research theme subscriptions
- Related issues: GitHub issue #1379 — Discovery track, shared work item CAPSTONE-031
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (subscription matching engine, alert dispatch, consent store), chainVerse-frontend (subscription management UI, alert inbox)

## Scope

This contract (`contracts/capstone-research-theme-subscriptions`) implements
the on-chain primitive required by **CAPSTONE-031 — Add research theme
subscriptions**.

The goal is to let users subscribe to saved searches or research themes and
receive consent-aware alerts for newly matching opportunities, without storing
query content or personal targeting data on-chain.

**What the contract stores on-chain:**

- A subscription record per `(subscriber, subscription_id)` containing: a
  `query_hash` (`BytesN<32>`) commitment over the full off-chain saved-search
  or theme query document, a `consent_ref` (`BytesN<32>`) referencing the
  subscriber's current alert-delivery consent record, an `active` flag, and
  the created-at and last-matched-at ledger timestamps.
- A subscription count per `subscriber` (`u32`) used to enforce a per-user
  subscription cap (`MAX_SUBSCRIPTIONS = 50`).
- An admin address for rate-limit and consent-gate configuration.

**What stays off-chain:**

- Query content: keyword lists, filter values, topic taxonomies, and
  preference weights. Only the `query_hash` commitment is stored.
- Consent preference details: communication channel, frequency, language, and
  opt-out metadata. Only the `consent_ref` opaque reference is stored.
- Alert payloads: the full opportunity details surfaced by a matching alert.
  The contract records only the fact that a match event occurred.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `subscribe(subscriber, subscription_id, query_hash, consent_ref)` | subscriber | Creates a new subscription. Returns `SubscriptionLimitReached` if the subscriber already holds `MAX_SUBSCRIPTIONS` active entries. |
| `update_query(subscriber, subscription_id, query_hash)` | subscriber | Replaces the query commitment. The `subscription_id` must exist and belong to the caller. |
| `update_consent(subscriber, subscription_id, consent_ref)` | subscriber | Replaces the consent reference on an existing subscription. |
| `deactivate(subscriber, subscription_id)` | subscriber | Sets `active = false`. Deactivated subscriptions are not evaluated by the matching engine. |
| `reactivate(subscriber, subscription_id)` | subscriber | Sets `active = true`. Returns `SubscriptionLimitReached` if the user already has `MAX_SUBSCRIPTIONS` active entries. |
| `delete_subscription(subscriber, subscription_id)` | subscriber | Removes the subscription record and decrements the subscription count. |
| `record_match(subscriber, subscription_id, opportunity_ref)` | matching engine (authorized) | Records that a match was found; updates `last_matched_at` and emits the match event. `opportunity_ref` is a `BytesN<32>` reference to the matched opportunity. |
| `get_subscription(subscriber, subscription_id)` | any | Returns `SubscriptionRecord` or `None`. |
| `subscription_count(subscriber)` | any | Returns the active subscription count for a subscriber. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `SubscriptionNotFound` | 1 | No subscription exists for `(subscriber, subscription_id)`. |
| `SubscriptionAlreadyExists` | 2 | A subscription already exists at this `(subscriber, subscription_id)`. |
| `SubscriptionLimitReached` | 3 | Subscriber already holds the maximum number of active subscriptions. |
| `SubscriptionInactive` | 4 | `record_match` attempted on a deactivated subscription. |
| `Unauthorized` | 5 | Caller is not the subscriber or the authorized matching engine. |
| `InvalidQueryHash` | 6 | `query_hash` is the zero hash. |
| `InvalidConsentRef` | 7 | `consent_ref` is the zero hash. |
| `InvalidOpportunityRef` | 8 | `opportunity_ref` is the zero hash. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPSTHSUB` | `(subscriber, subscription_id, query_hash)` | Subscription created. |
| `CAPSTHUPQ` | `(subscriber, subscription_id, query_hash)` | Query commitment updated. |
| `CAPSTHUPC` | `(subscriber, subscription_id)` | Consent reference updated. |
| `CAPSTHDEA` | `(subscriber, subscription_id)` | Subscription deactivated. |
| `CAPSTHREC` | `(subscriber, subscription_id)` | Subscription reactivated. |
| `CAPSTHDEL` | `(subscriber, subscription_id)` | Subscription deleted. |
| `CAPSTHMAT` | `(subscriber, subscription_id, opportunity_ref)` | Match recorded. |

### Storage layout

```
Persistent:
  Admin                                                               → Address
  Matcher                                                             → Address
  Subscription(subscriber: Address, subscription_id: BytesN<32>)     → SubscriptionRecord
  Count(subscriber: Address)                                          → u32
```

`SubscriptionRecord` fields:
- `subscriber: Address`
- `subscription_id: BytesN<32>`
- `query_hash: BytesN<32>` — commitment to the full off-chain query document
- `consent_ref: BytesN<32>` — opaque reference to the subscriber's consent record
- `active: bool`
- `created_at: u64`
- `last_matched_at: Option<u64>`

TTL: persistent, bumped to `[3_110_400, 6_220_800]` ledger ticks on every
write. The off-chain matching engine must periodically extend entries for
active subscriptions it manages.

## Privacy

The contract never stores query content, keyword lists, or personal targeting
preferences. `query_hash` is a caller-supplied `BytesN<32>` commitment whose
preimage remains exclusively with the subscriber and the matching engine.

`consent_ref` is an opaque reference; the full consent record (channel,
frequency, opt-out timestamp) lives in the off-chain consent store.

The `CAPSTHMAT` event carries only the `subscription_id` and an opaque
`opportunity_ref` — not the alert text, matched terms, or subscriber identity
beyond their address.

## Authorization

- `subscribe`, `update_query`, `update_consent`, `deactivate`, `reactivate`,
  and `delete_subscription` call `subscriber.require_auth()` and verify the
  stored record's `subscriber` matches the caller.
- `record_match` calls `matcher.require_auth()` where `matcher` is the admin-
  configured authorized matching engine address. No subscriber or other caller
  may invoke `record_match`.
- Admin-only entry points (`set_matcher`) call `admin.require_auth()`.
- Authorization is deny-by-default: no mutating entry point has an open-access
  path.

## Idempotency and replay resistance

`subscribe` requires a unique `(subscriber, subscription_id)` pair;
`SubscriptionAlreadyExists` on replay. `subscription_id` should be a
client-generated `BytesN<32>` derived from the query content hash to ensure
stability across retries.

`deactivate` is idempotent: calling on an already-inactive subscription
produces no change and no error.

`delete_subscription` removes the record and decrements the count atomically.
A replayed delete call returns `SubscriptionNotFound` — no double-decrement is
possible.

`record_match` on an inactive subscription returns `SubscriptionInactive`
rather than silently updating `last_matched_at`, preventing the matching
engine from inadvertently re-activating deactivated subscriptions.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Query content | `query_hash` commitment | Query storage, evaluation, keyword indexing |
| Consent preferences | `consent_ref` reference | Consent store, channel routing, opt-out enforcement |
| Alert delivery | `CAPSTHMAT` event | Notification dispatch (email, in-app, push) |
| Opportunity matching | `record_match` entry point | Matching engine, opportunity scoring, deduplication |
| Alert deduplication | — | Off-chain dedup by `(subscription_id, opportunity_ref)` within a time window |

## Dependencies

- **CAPSTONE-030** (supervisor directory search): the matching engine reads
  newly registered or updated supervisor entries to evaluate subscriptions
  against `filters_hash`-committed attributes.
- **capstone-identifiers**: `subscription_id` values should be stable
  client-generated identifiers consistent with platform conventions.
- **Consent management service** (off-chain): `consent_ref` must resolve to a
  live consent record before the matching engine dispatches alerts. The
  contract does not validate consent liveness; that is the matching engine's
  responsibility.
- **CAPSTONE-032** (scraping protection): subscription listing and
  `get_subscription` reads are subject to the rate limits enforced by that
  primitive to prevent enumeration of subscriber intent.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `subscribe` / `update_query` / `deactivate` rejected
  for non-subscriber; `record_match` rejected for non-matching-engine; cross-
  subscriber mutation (subscriber A deleting subscriber B's entry) rejected
  with `Unauthorized`.
- **Replay resistance**: double `subscribe` same `(subscriber, subscription_id)`
  → `SubscriptionAlreadyExists`; double `delete_subscription` → `SubscriptionNotFound`.
- **Subscription cap**: create `MAX_SUBSCRIPTIONS` entries → succeeds; create
  one more → `SubscriptionLimitReached`; delete one → new `subscribe` → succeeds.
- **Inactive guard**: deactivate a subscription → `record_match` →
  `SubscriptionInactive`; reactivate → `record_match` → succeeds.
- **Idempotency**: `deactivate` twice → second call is a no-op.
- **Count consistency**: create N, delete M, assert count = N - M at each
  step.
- **Zero hash guards**: `query_hash = [0u8; 32]` → `InvalidQueryHash`;
  `consent_ref = [0u8; 32]` → `InvalidConsentRef`; `opportunity_ref = [0u8; 32]`
  → `InvalidOpportunityRef`.
- **Event schemas**: assert topic symbol and payload types for all seven events.
- **Error codes**: every `ContractError` variant has at least one test that
  triggers it.
- **Storage bounds**: confirm that `Count(subscriber)` is bounded by
  `MAX_SUBSCRIPTIONS` and that the subscription map has no unbounded growth
  path.
- **Resource budget**: smoke-test `subscribe` and `record_match` within default
  Soroban instruction and memory limits.
- **Upgrade compatibility**: `SubscriptionRecord` additions must not break reads
  of earlier WASM records.

## Rollout considerations

1. Deploy after `capstone-identifiers` is live and the admin and matcher
   addresses are known.
2. Configure the matching engine address via `set_matcher` before enabling
   the subscription UI.
3. Seed the matching engine with `CAPSTHSUB` events from the initial deployment
   window to pick up any subscriptions created before the engine was live.
4. Align the consent reference format with the off-chain consent store before
   opening subscriptions to end users.
5. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rolling back removes all entry points; coordinate the matching engine to
  pause alert dispatch during rollback to avoid orphaned `record_match` calls.
- Persistent subscription records survive WASM rollback if the
  `SubscriptionRecord` XDR schema is unchanged.
- The `Count` entries are separate persistent keys; verify consistency after
  rollback by comparing count values against the number of surviving
  subscription records.

## Corresponding CAPSTONE-031 work in other repositories

- **chainVerse-backend**: matching engine — evaluates active subscriptions
  against newly registered or updated opportunities and supervisor entries,
  computes `query_hash` preimage matches, enforces consent gating via
  `consent_ref`, calls `record_match` for each hit, deduplicates alerts within
  a configurable time window, dispatches notifications through the consent-
  aware alert delivery pipeline.
- **chainVerse-frontend**: subscription management UI — allows users to create,
  edit, deactivate, and delete saved searches or theme subscriptions; displays
  matched alert history sourced from `CAPSTHMAT` events; respects notification
  preference settings.
