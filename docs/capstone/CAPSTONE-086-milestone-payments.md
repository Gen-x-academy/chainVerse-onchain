# CAPSTONE-086: Milestone Payments

**Track:** Funding | **Component:** on-chain | **Issue:** #1434 | **Crate:** `contracts/capstone-milestone-payments`

## Scope

Release a configured milestone amount only after a reviewer records deliverable acceptance and the challenge window has closed. Fees are computed on-ledger from a basis-point rate with deterministic truncation toward zero; the contract records a release receipt but never moves tokens itself.

Lifecycle: `Configured -> Accepted -> (Challenged -> Accepted | Cancelled) -> Released`.

## On-chain stores vs off-chain content

| Store | Key shape | Contents |
| --- | --- | --- |
| `Admin` | instance | platform bootstrap address |
| `RoleIndex` | `(tenant_id, project_id)` | bounded list of `(account, role)` grants |
| `RequestReceipt` | `(tenant_id, project_id, request_id)` | cached `result_id`, `op`, and pre-mutation `version` |
| `RequestIndex` | `(tenant_id, project_id)` | bounded request-id list |
| `Milestone` | `(tenant_id, project_id, milestone_id)` | amount, fee bps, acceptance/challenge hash, timestamps, state, version |
| `MilestoneIndex` | `(tenant_id, project_id)` | bounded milestone-id list |
| `Receipt` / `ReceiptIndex` | `receipt_id` / `(tenant_id, project_id)` | gross, fee, net, fee bps, release timestamp, version |

Everything is persisted with a 1-to-2-year TTL extension on every write.

| Off-chain | On-chain |
| --- | --- |
| Deliverable documents and review notes | `acceptance_hash` (32-byte commitment) |
| Challenge justification | `challenge_hash` / `resolution_hash` |
| Fee policy narrative and invoicing text | `fee_bps` and the computed amounts |
| Recipient identity and bank details | Nothing; escrow owns payout routing |

## Authorization

Deny by default. `initialize(admin)` stores the only global address and is one-time. `grant_role` requires that admin's `require_auth()` and writes a role scoped to `(tenant_id, project_id)`. Every domain entry point calls `require_auth()` on the caller and then checks a stored `Role` under the same scope. There is no global fallback for domain operations, so a platform admin, a reviewer from another tenant, or a finance officer from another project receives `Unauthorized`.

| Role | Allowed operations |
| --- | --- |
| `Coordinator` | configure milestone, resolve challenge |
| `Reviewer` | record acceptance, flag challenge |
| `Finance` | release payment |

## Idempotency and versioning

Every mutation takes `request_id` and `expected_version`. A receipt records `(result_id, op, version)`; the operation code prevents a request id from being reused across entry points. A replay with the same triple returns the cached result without writing a domain record or emitting an event. A replay with a different version or operation returns `RequestConflict`; a live write whose `expected_version` does not match the stored state version returns `StaleVersion`.

Release computes `fee = amount * fee_bps / 10000` with `checked_mul` and truncation, then `net = amount - fee` with `checked_sub`. Overflow returns `ArithmeticOverflow`; no silent wrapping or banker's rounding exists. The `receipt_id` is the release `request_id`, so retries return the same receipt.

## Errors

Append-only codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`, `NotFound=4`, `AlreadyExists=5`, `InvalidAmount=6`, `InvalidFee=7`, `InvalidWindow=8`, `InvalidState=9`, `StaleVersion=10`, `RequestConflict=11`, `ChallengeWindowOpen=12`, `ArithmeticOverflow=13`, `IndexFull=14`, `RoleLimitReached=15`, `RoleAlreadyGranted=16`.

## Events

| Topic | Data |
| --- | --- |
| `ROLEGRT` | `(tenant_id, project_id, role_code)` |
| `MSPCFG` | `(milestone_id, amount, fee_bps)` |
| `MSPACCP` | `(milestone_id, acceptance_hash, state_code)` |
| `MSPCHAL` | `(milestone_id, challenge_hash, state_code)` |
| `MSPCHRES` | `(milestone_id, resolution_hash, state_code)` |
| `MSPPAY` | `(milestone_id, receipt_id, gross, fee, net)` |

No event contains an address, string, memo, or free-form text.

## Privacy boundaries

Only 32-byte commitments, enum codes, amounts, and timestamps are stored. Reviewer identity is recorded in the scoped role index (public keys only) but is not emitted in domain events. Deliverable contents, dispute reasoning, and payout instructions remain in the off-chain evidence store.

## Dependencies and rollout

Depends on `capstone-identifiers` for stable project IDs, the tenant role source for initial grants, and the charter/pause contracts for eligibility checks performed by the caller. `escrow`, `escrow-vault`, and `payout-automation` consume `MSPPAY` through an idempotent adapter keyed by `(project_id, receipt_id)`.

Rollout: deploy, grant roles per tenant, configure conservative challenge windows, then enable the escrow adapter after consumers deduplicate receipts. Rollback: disable the escrow adapter first; stored milestones and receipts remain readable and no release can be duplicated. Contract version is `1`; additive struct fields require a new client build but preserve existing keys.

## Other repositories

- Backend: milestone configuration API, evidence hash store, challenge workflow, fee schedule mapping, and escrow orchestration.
- Frontend: acceptance form, challenge timer, release status view, and receipt download that resolves hashes off-chain.
- Auditing: indexer for `MSPPAY` and mismatch alerts when escrow has no matching receipt.

## Tests

Deterministic `Env::default()` tests cover role separation, cross-tenant denial, full lifecycle, challenge resolution, fee rounding and overflow, replay/conflict/version failures, bounded indices, event schemas and topics, timestamps, budget ceilings, one-time initialization, and a stable version constant.
