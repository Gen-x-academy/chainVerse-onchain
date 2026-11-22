# Capstone Integrity and Privacy

On-chain primitives for **CAPSTONE-102** (dispute resolution), **CAPSTONE-103**
(bidirectional blocking), **CAPSTONE-104** (evidence retention) and
**CAPSTONE-105** (consent ledger).

## Privacy boundary

Only opaque ids, addresses, bounded 32-byte commitments, u32 codes, counters
and timestamps are stored. Claim text, evidence files, safe-contact preferences
and consent notices stay off-chain; only hashes of them are committed here.
Events carry ids and codes, never private payloads.

## Authorization

`initialize(admin)` is one-time. The global admin only grants/revokes
tenant-scoped roles. Every capability write requires `caller.require_auth()` and
an explicitly granted role for that `tenant_id`; there is no implicit admin
fallback. Roles: 1 respondent, 2 custodian, 3 participant.

## Idempotency and lifecycle

Mutations bundle bounded fields in `CapstoneInput` (including `request_id`,
`input_hash` and `expected_version`) to stay within Soroban's 10-parameter ABI.
Replays return the stored version; conflicting inputs fail with
`RequestConflict`. Stale updates fail with `StaleVersion`. Error codes are
append-only.

## Capabilities

- **CAPSTONE-102:** disputes move Open → Responded → Evidence → Resolved →
  Appealed → Final → Remedied, with a bounded response/appeal window and an
  idempotent remedy step. Outcomes and reasons are hashes/codes only.
- **CAPSTONE-103:** `set_block` writes both directions of a participant pair so
  discovery, invitations, matching, messaging and collaboration are blocked
  regardless of initiator. Withdrawal removes both directions. Existing
  safety/financial records are not deleted.
- **CAPSTONE-104:** category retention policies bound evidence lifetimes;
  legal holds override expiry, disposal requires both expiry and no hold, and
  `log_evidence_access` records only a hash and a counter.
- **CAPSTONE-105:** consent is purpose-specific (matching, collaboration,
  provider access, integrity checks, analytics, publication, credentials),
  expirable and withdrawable. `has_consent` is deny-by-default.

## Off-chain boundaries / dependencies

The report store, notification service, evidence store, contact-preference
service and consent-notice publisher are off-chain dependencies. Their data
enters this contract only as salted 32-byte commitments; the contract never
calls them.

## Rollout and rollback

Deploy behind the standard upgrade authority. A fresh deployment starts with
an empty role table. To roll back, pause callers at the integration layer and
redeploy the previous WASM; no private content lives on-chain.

## Corresponding work

CAPSTONE-102..105 are shared work items implemented across the chainVerse
repositories; this crate provides only their on-chain commitment and state
primitive.

No contract tests were added in this change, per bounty scope.
