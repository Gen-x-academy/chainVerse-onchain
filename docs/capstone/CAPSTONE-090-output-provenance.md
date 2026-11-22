# CAPSTONE-090: Project Output Provenance

**Track:** Publishing | **Component:** on-chain | **Issue:** #1438 | **Crate:** `contracts/capstone-output-provenance`

## Scope

Records a bounded, versioned integrity commitment for each capstone output. The contract tracks commitments to source datasets, methods, tools, commits, transformations, authorship and licenses; it never stores the underlying material. A record moves `Recorded -> Verified -> Sealed`, or `Recorded -> Rejected`, and can be moved to `Expired` after its retention window. Sealed records are immutable.

## On-chain stores vs off-chain content

| On-chain store | Purpose |
| --- | --- |
| `TenantAdmin(tenant_id)` | One-time tenant bootstrap marker. |
| `Role(tenant_id, address, role)` | Stored role membership; deny by default. |
| `Request(tenant_id, project_id, request_id)` | Single-use request fingerprint. |
| `Intent(tenant_id, project_id, intent_id)` | Single-use transition fingerprint and version. |
| `Record(tenant_id, project_id, request_id)` | `ProvenanceRecord` with bounded hash vectors and state. |
| `ByOutput(tenant_id, project_id, output_id, version)` | Public lookup to the request id. |

The abstract, dataset contents, method prose, tool configuration, commit diffs, author identities, license text and transformation logs stay off-chain in the repository/file store. Only 32-byte commitments, coarse codes, versions, timestamps, states and public identifiers are written to the ledger. `verification_links` is a bounded vector of 32-byte commitments, not URLs.

## Authorization

Every mutating entry point calls `require_auth` on the supplied actor and then checks a role stored under the same `tenant_id`. Uninitialized tenants return `WrongTenant`; unknown actors return `NotAuthorized`. Roles are `Administrator`, `Operator`, `ResearchLead`, `Verifier` and `Curator`. `record_provenance` requires `ResearchLead`, `attest_provenance` requires `Verifier`, `seal_provenance` requires `Curator`, and `expire_provenance` requires `Operator`. Only an `Administrator` can grant roles. No role means no permission.

## Idempotency

`request_id` is single-use per `(tenant_id, project_id)` and is bound by SHA-256 to the full payload, actor, output id and version. A byte-identical retry returns the original `request_id` and emits no second event; a reused id with different content returns `RequestAlreadyUsed`. `intent_id` is single-use per scope and bound to actor, target, action tag, payload hash and `expected_version`. A replay returns success without state change; a mismatched replay returns `IntentAlreadyUsed`. Every transition checks `expected_version`, so stale approvals fail with `StaleVersion`. Arithmetic uses checked addition for expiry and vector totals; storage TTLs are extended on every write and read.

## Errors

`ContractError` is append-only. Current codes: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAuthorized=3`, `WrongTenant=4`, `RequestAlreadyUsed=5`, `IntentAlreadyUsed=6`, `StaleVersion=7`, `RecordNotFound=8`, `InvalidState=9`, `TooManyHashes=10`, `InvalidValidity=11`, `Overflow=12`, `AlreadyExpired=13`, `AlreadyHasRole=14`, `NotExpired=15`. New codes must be appended, never reordered or reused.

## Events

`CAPTNT`, `CAPROL`, `PROVNEW`, `PROVATT`, `PROVSEA`, `PROVEXP` use `symbol_short!` topics. Data contains only tenant/project/output/request/intent ids, versions, enums and timestamps. No addresses, descriptions, URLs, names or free text are emitted. `PROVATT` carries the `Verdict` enum only.

## Privacy boundaries

Private research data, personal author details, embargoed datasets, proprietary tooling and license negotiations remain off-chain. The ledger sees unlinkable-looking 32-byte commitments and public codes. Deployments must use high-entropy commitments rather than hashes of low-entropy text, and indexers must treat every emitted hash as public forever.

## Dependencies

Uses `soroban-sdk` 22.0.11 only. Conceptually depends on `capstone-identifiers` for the public `project_id`, on the tenant role registry pattern for `tenant_id` membership, and on the off-chain artifact store for the preimages of every hash. Consumed by IP terms, publication approvals and showcase pages as an integrity reference.

## Rollout and rollback

Deploy, initialize each tenant with a dedicated administrator, grant the minimum roles, then backfill commitments from the off-chain store. Keep reads available while consumers migrate. Rollback is a consumer-side stop: retire new `PROVNEW` writes and keep existing records readable until their TTL lapses; never delete or rewrite records. Because error codes and event schemas are append-only, rollback must not require remapping codes.

## Other-repository work

The backend owns artifact hashing, dataset/method/tool metadata, role administration, retention scheduling and the bounded-hash assembly sent to `record_provenance`. The frontend provides the provenance form and read-only verification view. The docs repository maps the code values for licenses and relation types. Indexers consume the events idempotently keyed by `(tenant_id, project_id, request_id|intent_id)`.
