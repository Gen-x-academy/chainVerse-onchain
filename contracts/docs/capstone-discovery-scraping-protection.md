# Capstone Discovery Scraping Protection Contract

- Status: Specified — pending implementation
- Owner: Capstone On-chain working group
- Tracking: [CAPSTONE-032] Protect public discovery from scraping
- Related issues: GitHub issue #1380 — Discovery track, shared work item CAPSTONE-032
- Audit baseline: commit `5b62b369c1dbf51e6ebf9fbfc85192f3cc0f72fb`, branch `main`, 2026-11-18
- Related repositories: chainVerse-backend (rate-limit enforcement middleware, bot detection service, cache policy enforcement), chainVerse-frontend (safe public projection renderer, rate-limit backoff UI)

## Scope

This contract (`contracts/capstone-discovery-scraping-protection`) implements
the on-chain primitive required by **CAPSTONE-032 — Protect public discovery
from scraping**.

The goal is to apply rate limits, enumeration resistance, safe public
projections, caching policy, and bot controls without harming normal browsing.
The contract provides the on-chain commitment and audit layer; the enforcement
middleware runs off-chain but records its policy version and violation signals
on-chain so policy changes and abuse events are auditable.

**What the contract stores on-chain:**

- A versioned policy record per `policy_version` containing: a `policy_hash`
  (`BytesN<32>`) commitment over the full off-chain policy document (rate-limit
  thresholds, cache TTLs, enumeration-resistance parameters, bot-signal
  weights), the published-by admin address, and the published-at timestamp.
- A `latest_policy_version` pointer.
- A bounded violation log: each `ViolationEntry` stores a `caller_commitment`
  (`BytesN<32>` = `sha256(caller_address)`), the `violation_type` symbol, the
  `policy_version` in effect at violation time, and the ledger timestamp.
  Violations older than `VIOLATION_TTL` (~7 days) are expired from temporary
  storage automatically.
- An allowlist of off-chain enforcement agents authorized to record violations.

**What stays off-chain:**

- Rate-limit counters and sliding-window state: computed and enforced by the
  backend middleware; not stored on-chain to avoid write amplification.
- Bot-signal scores, session fingerprints, and IP metadata: privacy-sensitive
  and unsuitable for public ledger storage. Only the violation outcome is
  committed.
- Cache content and CDN configuration: the `policy_hash` commits to the
  cache TTL and cache-control policy document; actual cache infrastructure is
  off-chain.
- Full caller identity beyond the hashed commitment in violation records.

### Entry points

| Function | Caller | Description |
|---|---|---|
| `publish_policy(admin, version, policy_hash)` | admin | Publishes a new versioned policy commitment. `version` must be strictly greater than the current latest. |
| `record_violation(agent, caller_commitment, violation_type, policy_version)` | authorized agent | Records a scraping or enumeration violation event. `policy_version` must match the current latest. |
| `add_agent(admin, agent)` | admin | Adds an address to the enforcement agent allowlist. |
| `remove_agent(admin, agent)` | admin | Removes an address from the enforcement agent allowlist. |
| `get_policy(version)` | any | Returns `PolicyRecord` for the given version or `None`. |
| `latest_policy_version()` | any | Returns the current latest policy version number or `None`. |
| `get_violation_count(caller_commitment)` | any | Returns the number of non-expired violation records for a caller commitment. |

### Error codes

| Code | Value | Meaning |
|---|---|---|
| `InvalidVersion` | 1 | `version` is 0 or not strictly greater than the current latest. |
| `VersionAlreadyPublished` | 2 | A policy record already exists at this version. |
| `Unauthorized` | 3 | Caller is not the admin (for admin-only calls) or not an authorized agent (for `record_violation`). |
| `PolicyVersionMismatch` | 4 | `record_violation` supplied a `policy_version` that does not match the current latest. |
| `InvalidPolicyHash` | 5 | `policy_hash` is the zero hash. |
| `InvalidCallerCommitment` | 6 | `caller_commitment` is the zero hash. |
| `AgentNotFound` | 7 | `remove_agent` called for an address not on the allowlist. |

### Events

| Topic symbol | Payload | Trigger |
|---|---|---|
| `CAPSCPPUB` | `(version, admin, policy_hash)` | Policy version published. |
| `CAPSCPVIO` | `(caller_commitment, violation_type, policy_version)` | Violation recorded. |
| `CAPSCPAGA` | `(agent)` | Enforcement agent added. |
| `CAPSCPAGR` | `(agent)` | Enforcement agent removed. |

### Storage layout

```
Persistent:
  Admin                                     → Address
  Policy(version: u32)                      → PolicyRecord
  Latest                                    → u32
  Agent(agent: Address)                     → bool

Temporary (TTL: 120_960 ledger ticks ≈ 7 days):
  Violation(seq: u64)                       → ViolationEntry
  ViolationCount(caller_commitment: BytesN<32>)  → u32
```

`PolicyRecord` fields:
- `version: u32`
- `policy_hash: BytesN<32>` — commitment to the full off-chain policy document
- `published_by: Address`
- `published_at: u64`

`ViolationEntry` fields:
- `caller_commitment: BytesN<32>` — `sha256(caller_address)` or agent-supplied opaque commitment
- `violation_type: Symbol` — e.g. `RateExceeded`, `EnumerationDetected`, `BotSignal`
- `policy_version: u32`
- `recorded_at: u64`

Violation records use a short temporary TTL (`[120_960, 120_960]` ≈ 7 days).
Policy records use a long persistent TTL (`[3_110_400, 6_220_800]`).

## Privacy

Violation records use `sha256(caller_address)` commitments, not raw addresses.
This prevents public indexers from trivially mapping violations to identities
while still allowing authorized enforcement agents to correlate records with
their own caller-identity index.

The contract never stores session fingerprints, IP addresses, bot-signal
scores, or rate-limit counter values.

The `CAPSCPVIO` event emits only the caller commitment hash, violation type,
and policy version.

## Authorization

- `publish_policy` calls `admin.require_auth()`.
- `record_violation` calls `agent.require_auth()` and verifies the agent is on
  the admin-managed allowlist.
- `add_agent` / `remove_agent` call `admin.require_auth()`.
- `get_policy`, `latest_policy_version`, and `get_violation_count` are
  read-only and require no authentication.
- Authorization is deny-by-default: no mutating entry point has an open-access
  path.

## Idempotency and replay resistance

`publish_policy`: each version is written exactly once; `VersionAlreadyPublished`
on replay. Strictly monotonic version numbers mean a replayed old-version
transaction always fails.

`record_violation`: uses a monotonically incrementing sequence number as the
storage key, so each invocation creates a distinct entry. The enforcement agent
is responsible for deduplicating at its own layer before calling in.

`add_agent` is idempotent: adding an already-present agent is a no-op.
`remove_agent` returns `AgentNotFound` if the address is not on the list,
preventing confusion over whether the removal succeeded.

## Off-chain boundaries

| Boundary | On-chain primitive | Off-chain responsibility |
|---|---|---|
| Rate-limit enforcement | `policy_hash` commitment + `record_violation` | Sliding-window counters, request throttling, 429 response handling |
| Enumeration resistance | `policy_hash` commitment | Opaque cursor tokens, result shuffling, field projection stripping |
| Bot controls | `record_violation` (BotSignal type) | Bot-signal scoring, CAPTCHA gating, session fingerprinting |
| Cache policy | `policy_hash` commitment | CDN TTL configuration, cache-control headers, stale-while-revalidate |
| Public projections | `policy_hash` commitment | Field allowlist enforcement, sensitive field stripping in API responses |
| Caller identity correlation | `caller_commitment` hash | Agent-side mapping from commitment to session or account |

## Dependencies

- **CAPSTONE-030** (supervisor directory search): the scraping protection
  policy governs read access to the supervisor registry and search endpoints.
- **CAPSTONE-031** (research theme subscriptions): subscription listing
  endpoints are covered by this policy to prevent enumeration of subscriber
  intent.
- **capstone-identifiers**: the enforcement agent must be a registered platform
  identity before being added to the allowlist.

## Contract tests (required)

All tests must be deterministic and pass in a sandboxed Soroban test
environment with no network calls.

- **Authorization tree**: `publish_policy` rejected for non-admin;
  `record_violation` rejected for non-agent; `add_agent` / `remove_agent`
  rejected for non-admin.
- **Replay resistance**: double `publish_policy` same version →
  `VersionAlreadyPublished`; `record_violation` with stale `policy_version` →
  `PolicyVersionMismatch`.
- **Strict monotonicity**: versions 1 → 2 → 3; version 2 again → rejected.
- **Violation TTL**: assert violation records use the short temporary TTL;
  assert policy records use the long persistent TTL.
- **Count tracking**: record 3 violations for the same `caller_commitment`;
  assert `get_violation_count` returns 3.
- **Agent lifecycle**: add agent → `record_violation` → succeeds; remove
  agent → `record_violation` → `Unauthorized`.
- **Idempotency**: `add_agent` twice → no error on second call.
- **Zero hash guards**: `policy_hash = [0u8; 32]` → `InvalidPolicyHash`;
  `caller_commitment = [0u8; 32]` → `InvalidCallerCommitment`.
- **Event schemas**: assert topic symbol and payload types for all four events.
- **Error codes**: every `ContractError` variant has at least one test.
- **Resource budget**: smoke-test `publish_policy` and `record_violation`
  within default Soroban instruction and memory limits.
- **Upgrade compatibility**: `PolicyRecord` and `ViolationEntry` additions
  must not break reads of earlier WASM records.

## Rollout considerations

1. Deploy after the enforcement middleware is ready and the admin address is
   known.
2. Publish the first policy version via `publish_policy` before adding
   enforcement agents, so agents always have a current policy version to
   reference.
3. Add enforcement agent addresses via `add_agent` before enabling middleware
   recording.
4. Configure the backend middleware to subscribe to `CAPSCPPUB` events and
   reload its in-process policy document when a new version is published.
5. Align the violation TTL (7 days) with the off-chain abuse review SLA.
6. No existing on-chain state to migrate — this is a new contract.

## Rollback considerations

- Rolling back removes the `record_violation` entry point; coordinate the
  enforcement middleware to pause violation recording during rollback.
- Persistent policy records survive WASM rollback if the `PolicyRecord` XDR
  schema is unchanged.
- Temporary violation records expire within 7 days even if rollback cannot
  remove them explicitly.
- The agent allowlist persists through rollback.

## Corresponding CAPSTONE-032 work in other repositories

- **chainVerse-backend**: enforcement middleware — applies rate limits and
  enumeration-resistance controls at the API gateway layer; evaluates bot
  signals; enforces field-projection allowlists; computes `caller_commitment`
  from session identity; calls `record_violation` on confirmed abuse events;
  subscribes to `CAPSCPPUB` events to reload policy on version change; manages
  cache-control header injection per the policy document.
- **chainVerse-frontend**: safe public projection renderer — displays only
  permitted fields from discovery API responses; implements graceful backoff
  and user-facing messaging when a 429 rate-limit response is returned; does
  not expose raw supervisor identifiers in URLs to resist enumeration.
