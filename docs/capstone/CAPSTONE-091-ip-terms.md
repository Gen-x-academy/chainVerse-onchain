# CAPSTONE-091: Intellectual Property Terms

**Track:** Publishing | **Component:** on-chain | **Issue:** #1439 | **Crate:** `contracts/capstone-ip-terms`

## Scope

Manages a versioned, privacy-minimized commitment to intellectual-property terms. Terms move `Draft -> Accepted -> Active -> Superseded`, or to `Expired` after the retention window. The contract records coarse code values for ownership, license, sponsor rights, commercialization and reuse, an embargo timestamp, participant acceptance count and a commitment to the private agreement.

## On-chain stores vs off-chain content

| On-chain store | Purpose |
| --- | --- |
| `TenantAdmin(tenant_id)` | One-time tenant bootstrap marker. |
| `Role(tenant_id, address, role)` | Stored role membership; deny by default. |
| `Request(tenant_id, project_id, request_id)` | Single-use proposal fingerprint. |
| `Intent(tenant_id, project_id, intent_id)` | Single-use transition fingerprint and version. |
| `Terms(tenant_id, project_id, terms_id)` | `IpTerms` with codes, counts, state and expiry. |
| `Acceptance(tenant_id, project_id, terms_id, actor)` | Prevents duplicate participant acceptance. |
| `Latest(tenant_id, project_id)` | Latest terms id for a project. |

The signed agreement, sponsor schedules, royalty formulas, participant names, legal text and embargo justification stay off-chain. The ledger stores only the `terms_hash` commitment, bounded code values, `embargo_until`, counters, timestamps and states.

## Authorization

All mutations call `require_auth` and check a role stored under the caller's `tenant_id`. Roles are `Administrator`, `Operator`, `IpManager`, `Participant` and `Auditor`. Proposal, embargo updates and supersession require `IpManager`; acceptance requires `Participant`; activation requires `Administrator`; expiry requires `Operator`. Uninitialized tenants return `WrongTenant`, and a caller without the stored role returns `NotAuthorized`.

## Idempotency

`request_id` is single-use per tenant/project and is bound to every proposal field through a composite payload hash. `intent_id` is single-use and bound to actor, target terms, action tag, payload and `expected_version`. Retries of accept, activate, embargo, supersede or expire return the stored outcome without a second event; conflicting reuse returns `IntentAlreadyUsed`. Acceptance is additionally keyed by actor, so one participant cannot count twice. `expected_version` rejects stale decisions with `StaleVersion`. Expiry arithmetic and acceptance increments use checked operations; TTLs are extended on reads and writes.

## Errors

`ContractError` is append-only. Current codes: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAuthorized=3`, `WrongTenant=4`, `RequestAlreadyUsed=5`, `IntentAlreadyUsed=6`, `StaleVersion=7`, `TermsNotFound=8`, `InvalidState=9`, `InvalidPolicy=10`, `Overflow=11`, `NotExpired=12`, `AlreadyAccepted=13`, `AcceptanceLimit=14`, `AlreadyHasRole=15`, `InvalidCode=16`, `AlreadyExpired=17`. Append new codes only.

## Events

`CAPTNT`, `CAPROL`, `IPNEW`, `IPACPT`, `IPACT`, `IPEMB`, `IPSUP`, `IPEXP` are `symbol_short!` events. Payloads are limited to ids, versions, code values, counts, timestamps and hashes. No legal text, participant identity, sponsor identity or free-form note is emitted.

## Privacy boundaries

The agreement preimage is private and must remain in the document store. Code values are deliberately coarse and must not encode confidential terms. Embargo timestamps are public and should be set to the minimum necessary value. High-entropy commitments are required; indexing services must treat every hash as permanently public.

## Dependencies

Uses `soroban-sdk` 22.0.11. Conceptually depends on `capstone-identifiers` for `project_id`, on the tenant role registry pattern, on output provenance for licensing integrity, and on the off-chain legal document store. Consumed by publication approvals and showcase pages as a public licensing state reference.

## Rollout and rollback

Deploy, initialize tenants, grant `IpManager` and participant roles, then backfill commitments for existing agreements. Start with conservative acceptance thresholds. Rollback stops new proposals and leaves active terms readable; supersession and expiry remain available so no project is stranded. Never rewrite existing terms or error codes.

## Other-repository work

The backend owns agreement storage, code mapping, participant acceptance collection, embargo scheduling and role administration. The frontend renders the terms summary, acceptance queue and license display. Legal operations owns the code registry in the docs repository. Indexers key idempotent consumers by `(tenant_id, project_id, terms_id|intent_id)`.
