# CAPSTONE-092: Publication Approvals

**Track:** Publishing | **Component:** on-chain | **Issue:** #1440 | **Crate:** `contracts/capstone-publication-approvals`

## Scope

Coordinates author, supervisor, sponsor, ethics and organization approval before public release or external submission. A request moves `Requested -> Approved -> Released`, or to `Rejected` / `Expired`. Once released, the result is final; no further decisions are accepted.

## On-chain stores vs off-chain content

| On-chain store | Purpose |
| --- | --- |
| `TenantAdmin(tenant_id)` | One-time tenant bootstrap marker. |
| `Role(tenant_id, address, role)` | Stored role membership; deny by default. |
| `Request(tenant_id, project_id, request_id)` | Single-use request fingerprint. |
| `Intent(tenant_id, project_id, intent_id)` | Single-use decision/finalize fingerprint and version. |
| `Approval(tenant_id, project_id, request_id)` | `ApprovalRequest` with bounded role set, counts and state. |
| `Vote(tenant_id, project_id, request_id, actor)` | Prevents duplicate decisions by one address. |

Drafts, reviewer comments, ethics case files, sponsor negotiations, submission manuscripts and decision rationale stay off-chain. The ledger stores only the `target_hash`, `release_code`, bounded role set, counters, states, timestamps and decision commitments.

## Authorization

Mutating calls use `require_auth` plus stored roles under the caller's `tenant_id`. `Administrator` and `Operator` are configuration/lifecycle roles. Author, Supervisor, Sponsor, Ethics and Organization are approval roles. `request_approval` requires `Author`, `decide` requires one of the roles listed in the stored `required_roles` set, `finalize_approval` requires `Administrator`, and `expire_approval` requires `Operator`. Administrator is deliberately not accepted as a substitute for an approval role. Unknown tenants return `WrongTenant`; missing roles return `NotAuthorized`.

## Idempotency

`request_id` is single-use and bound to release code, target hash, bounded role set, validity and actor. `intent_id` is single-use and bound to actor, target request, decision enum, decision hash and `expected_version`. Replays return the stored outcome with no second event; conflicts return `IntentAlreadyUsed`. `Vote` prevents one address from deciding twice even if it holds multiple roles. `expected_version` rejects stale decisions with `StaleVersion`. Counts and expiry use checked arithmetic; TTLs are extended on every read and write.

## Errors

`ContractError` is append-only. Current codes: `NotInitialized=1`, `AlreadyInitialized=2`, `NotAuthorized=3`, `WrongTenant=4`, `RequestAlreadyUsed=5`, `IntentAlreadyUsed=6`, `StaleVersion=7`, `ApprovalNotFound=8`, `InvalidState=9`, `InvalidRoleSet=10`, `TooManyRoles=11`, `NotExpired=12`, `AlreadyVoted=13`, `Overflow=14`, `InvalidValidity=15`, `AlreadyHasRole=16`, `AlreadyExpired=17`. Append new codes only.

## Events

`CAPTNT`, `CAPROL`, `PUBNEW`, `PUBDEC`, `PUBFIN`, `PUBEXP` are `symbol_short!` events. Data carries only ids, versions, `ReleaseCode`, `Decision` and hashes. Reviewer addresses, comments and document contents are never emitted.

## Privacy boundaries

Reviewer identities are public only to the extent that the tenant grants a role to an address; the event stream does not reveal who voted. Decision hashes must commit to off-chain rationale without embedding it. The bounded role set is structural metadata, not personal data. Tenants should avoid address reuse across unrelated roles if linkage is a concern.

## Dependencies

Uses `soroban-sdk` 22.0.11. Conceptually depends on `capstone-identifiers` for `project_id`, on IP terms for the license state, on output provenance for the target hash, and on the off-chain review workspace. Consumed by showcase pages, which store only a release approval commitment.

## Rollout and rollback

Deploy, initialize tenants, grant approval roles to the minimum set, then enable release finalization in the backend. Keep the event consumer idempotent on `(tenant_id, project_id, request_id|intent_id)`. Rollback stops new requests and finalizations, leaves requests readable for audit, and lets pending requests expire; already released approvals are not reversed on-chain.

## Other-repository work

The backend owns the review UI, role assignment, document storage, decision rationale and release orchestration. The frontend renders the approval queue and release gating. The docs repository defines `ReleaseCode` semantics. Indexers must treat `PUBFIN` as the single release trigger and deduplicate by request id.
