# CAPSTONE-093: Project Showcase Pages

**Track:** Publishing | **Component:** on-chain | **Issue:** #1441 | **Crate:** `contracts/capstone-showcase-pages`

## Scope

Publishes a bounded, versioned commitment to a project showcase page. The page covers approved abstracts, contributors, outputs, demonstrations, citations, licenses, funding acknowledgements and verification links. A page moves `Draft -> Published -> Updated -> Unpublished`, or to `Expired` after its retention window. All visible content lives off-chain; the chain holds only commitments and states.

## On-chain stores vs off-chain content

| On-chain store | Purpose |
| --- | --- |
| `TenantAdmin(tenant_id)` | One-time tenant bootstrap marker. |
| `Role(tenant_id, address, role)` | Stored role membership; deny by default. |
| `Request(tenant_id, project_id, request_id)` | Single-use page creation fingerprint. |
| `Intent(tenant_id, project_id, intent_id)` | Single-use publish/update/unpublish/expire fingerprint and version. |
| `Page(tenant_id, project_id, page_id)` | `ShowcasePage` with bounded verification commitments and state. |
| `Latest(tenant_id, project_id)` | Latest page id for a project. |

Abstracts, contributor profiles, demonstration media, citation text, license documents, funding statements and verification URLs stay in the off-chain content store. Only 32-byte commitments to each section, a bounded vector of 16 verification commitments, approval hash, versions, timestamps and states are on-chain.

## Authorization

Mutating calls use explicit `require_auth` and a stored role under the caller's `tenant_id`. Roles are `Administrator`, `Operator`, `Curator`, `Publisher` and `Auditor`. Creation and content updates require `Curator`; publish and unpublish require `Publisher`; expiry requires `Operator`; role grants require `Administrator`. Uninitialized tenants return `WrongTenant`; missing roles return `NotAuthorized`. Separation of curator and publisher prevents unilateral release of edited content.

## Idempotency

`request_id` is single-use and bound to every page field, actor and version through a composite hash. `intent_id` is single-use and bound to actor, page id, action tag, payload hash and `expected_version`. Retries return the stored outcome without a second event; conflicting reuse returns `IntentAlreadyUsed`. `expected_version` prevents stale updates with `StaleVersion`, and each successful update increments the version with checked arithmetic. Revisions are capped at 32; verification commitments at 16. TTLs are extended on reads and writes.

## Errors

`ContractError` is append-only. Current codes: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAuthorized=3`, `WrongTenant=4`, `RequestAlreadyUsed=5`, `IntentAlreadyUsed=6`, `StaleVersion=7`, `PageNotFound=8`, `InvalidState=9`, `TooManyLinks=10`, `InvalidValidity=11`, `Overflow=12`, `NotExpired=13`, `RevisionLimit=14`, `AlreadyExpired=15`, `AlreadyHasRole=16`. Append new codes only.

## Events

`CAPTNT`, `CAPROL`, `SHWNEW`, `SHWPUB`, `SHWUPD`, `SHWUNP`, `SHWEXP` are `symbol_short!` events. Payloads contain only ids, versions, hashes, revision counts and timestamps. No abstract text, contributor name, URL or funding wording is emitted.

## Privacy boundaries

The showcase renderer resolves the on-chain commitments to off-chain content under its own access rules. Public hashes are permanent; they must be high-entropy commitments, not hashes of short or guessable strings. Approval hash and verification commitments link the page to CAPSTONE-092 and the external verification store without exposing their contents. Tenants must not place personal data in code values because no such field exists in the contract.

## Dependencies

Uses `soroban-sdk` 22.0.11. Conceptually depends on `capstone-identifiers` for `project_id`, on publication approvals for the release commitment, on IP terms for license state, on output provenance for artifact integrity, and on the off-chain content store. Consumed by public directory indexers and the showcase frontend.

## Rollout and rollback

Deploy, initialize tenants, grant curator and publisher roles to separate principals, then enable publishing after the renderer resolves commitments correctly. Keep read endpoints available while caches warm. Rollback stops new publish/update calls and unpublishes pages through the normal state transition; pages remain readable for audit until TTL expiry. Do not remove records or reuse error codes.

## Other-repository work

The backend owns content authoring, commitment generation, approval lookup, role administration and the public renderer. The frontend provides the page editor and published view. The docs repository defines the canonical section layout. Indexers deduplicate by `(tenant_id, project_id, page_id|intent_id)` and treat `SHWPUB`/`SHWUPD` as cache invalidation signals only.
