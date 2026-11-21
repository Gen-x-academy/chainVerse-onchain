# CAPSTONE-084: Create Escrow Funding Intents

**Track:** Funding | **Component:** on-chain | **Issue:** #1432 | **Contract:** `contracts/capstone-escrow-intents`

## Scope

Create replay-safe escrow funding intents tied to an accepted charter, a payer, an amount, an asset,
an expiry, and a commitment to explicit release conditions. An intent is a public instruction
record; it does not move funds and does not store the charter or condition text.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service / payer wallet) |
| --- | --- |
| `EscrowIntent`: tenant, project, intent id/hash, payer address, charter hash, conditions hash, asset, amount, state, version, timestamps, request id/hash | Charter text, funding agreement, release-condition prose, payment instructions, payer display data |
| `RequestMarker`, cancel/expiry markers, role grants | Canonical request payloads, receipts, notifications |
| Bounded index of intent ids per project | Search and filtering views |
| Events with IDs, asset hash, amount, state enum | Payer UX and audit exports |

`intent_hash`, `charter_hash`, and `conditions_hash` are SHA-256 commitments to canonical off-chain
documents. The client must verify these hashes before asking the payer to sign.

## Authorization

- One-time `initialize` requires the administrator's `require_auth`.
- Tenant-scoped stored roles: `FinanceOfficer` creates/cancels intents, `Payer` authorizes creation,
  `Auditor` marks expiry.
- `create_intent` calls `require_auth` for both the officer and the payer and requires both stored
  roles in the same tenant. Either missing signature or role returns `Unauthorized`.
- All keys are `(tenant_id, project_id, intent_id/request_id, ...)`; cross-tenant requests fail the
  role check, and cross-tenant reads return `NotFound`. No admin can create or cancel on a payer's
  behalf.

## Idempotency and versioning

- `intent_id` is the replay key and doubles as the creation `request_id`. `expected_version` must be
  `0` for a new intent.
- A retry with the same `intent_id` and `intent_hash` returns the original intent and emits no new
  event. A reused id with a different hash or base version returns `RequestConflict`.
- `cancel_intent` and `mark_expired` use separate `request_id` values; retries are no-ops, and a
  different request against a terminal state returns `InvalidState`.
- Lifecycle: implicit `Pending -> Cancelled | Expired`. CAPSTONE-085 marks an intent as confirmed
  off-chain by storing a confirmation against the same `intent_id`; this contract never mutates that
  outcome.
- Bounds: `MAX_INTENTS_PER_PROJECT = 16`; version creation uses `checked_add`; all persistent keys
  extend TTL.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`WrongTenant=4`, `NotFound=5`, `InvalidAmount=6`, `InvalidExpiry=7`, `VersionConflict=8`,
`RequestConflict=9`, `InvalidState=10`, `CapacityExceeded=11`, `Overflow=12`, `Expired=13`,
`HashMismatch=14`, `AlreadyExists=15`. Append new codes only.

## Events

| Symbol | Data |
| --- | --- |
| `CBPROLG` / `CBPRVRK` | `(tenant_id, account, role)` |
| `CBEINT` | `(tenant_id, project_id, intent_id, amount, asset)` |
| `CBECAN` | `(tenant_id, project_id, intent_id, IntentState::Cancelled)` |
| `CBEEXP` | `(tenant_id, project_id, intent_id, IntentState::Expired)` |

Only IDs, hashes, amounts, asset identifiers, and enums are published.

## Privacy boundaries

The payer address is a public ledger account; payer identity data remains off-chain. Charter and
release-condition documents are private and are represented only by hashes. Amounts and asset
identifiers are public because confirmation depends on them. No bank account, memo, agreement text,
or note is stored in contract storage or events.

## Dependencies

- CAPSTONE-050/052 supply the accepted charter and amendment hash.
- CAPSTONE-082 supplies the plan-derived amount context; CAPSTONE-083 supplies reservation status.
- CAPSTONE-085 consumes `intent_id` and `intent_hash` for confirmation.
- `capstone-identifiers` supplies `project_id`; tenant roles are mirrored from the shared ACL.

## Rollout and rollback

Enable the intent flow behind the funding flag and create intents in parallel with the existing
off-chain payment queue for one cohort. Reconcile intent ids and hashes before making chain state
authoritative. Rollback stops new intent creation; existing pending intents can be cancelled or
expired through the contract, and no private content needs purging. TTL renewal keeps active intent
records available across upgrades.

## Other-repository work

- Backend: canonicalize/hash charter and conditions, generate intent ids, sync roles, and expose
  intent status to the payer flow.
- Frontend/payer wallet: display hash-verified funding intent and collect payer authorization.
- Indexer/ops: monitor intent expiry, `RequestConflict`, and the bounded per-project index.
