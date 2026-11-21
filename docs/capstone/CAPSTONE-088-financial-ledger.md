# CAPSTONE-088: Financial Ledger

**Track:** Funding | **Component:** on-chain | **Issue:** #1436 | **Crate:** `contracts/capstone-financial-ledger`

## Scope

Reconcile intents, reservations, holds, releases, refunds, reimbursements, fees, and external transactions inside a bounded reporting period. The contract records commitment hashes and amounts, computes deltas, marks actionable mismatches, and requires resolution before a period can close. It does not hold or move funds.

## On-chain stores vs off-chain content

| Store | Key shape | Contents |
| --- | --- | --- |
| `Admin` | instance | platform bootstrap address |
| `RoleIndex` | `(tenant_id, project_id)` | bounded role grants |
| `RequestReceipt` / `RequestIndex` | `(tenant_id, project_id, request_id)` | cached result id, operation code, version |
| `Period` / `PeriodIndex` | `(tenant_id, project_id, period_id)` | window, entry limit/count, state, attestation hash, version |
| `Entry` / `EntryIndex` | `(tenant_id, project_id, period_id, entry_id)` | kind, amount, reference hash, timestamp, counterpart, delta, state, version |
| `Match` | `(tenant_id, project_id, period_id, entry_id)` | delta, reason code, resolution hash, state, version |

All writes extend the 1-to-2-year TTL window.

| Off-chain | On-chain |
| --- | --- |
| Bank statements and external transaction text | `reference_hash` and `EntryKind::External` |
| Accounting notes and variance explanations | `reason_code` and `resolution_hash` |
| Close checklist and sign-off identities | `attestation_hash` and period state |
| Payer/payee names and account numbers | Nothing |

## Authorization

Deny by default. `initialize` is one-time; only the stored platform admin can grant roles. Every entry point calls `require_auth()` and checks the stored role scoped to `(tenant_id, project_id)`. No global fallback exists, so an auditor cannot record entries and a finance officer from another project cannot open or close periods.

| Role | Allowed operations |
| --- | --- |
| `Coordinator` | open and close periods |
| `Finance` | record entries, match entries, resolve mismatches |
| `Auditor` | off-chain read access to indexer output; no on-chain mutation role |

## Idempotency and versioning

All mutations take `request_id` and `expected_version`. Receipts cache `(result_id, op, version)`, so replaying an identical call returns the same period, entry, or match without emitting a second event. Reuse with a different operation or version yields `RequestConflict`; a live call with a stale expected version yields `StaleVersion`. Period version advances once per accepted entry, match, resolution, or close.

Matching uses `checked_sub` for the delta and `checked_abs` before comparing against a non-negative tolerance capped at 1,000,000. A mismatch sets `reason_code=1` and blocks close with `MismatchOpen` until `resolve_mismatch` records a resolution hash and returns the pair to `Matched`. A period with any `Pending` entry also cannot close.

## Errors

Append-only codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`, `NotFound=4`, `AlreadyExists=5`, `InvalidPeriod=6`, `InvalidEntry=7`, `InvalidState=8`, `StaleVersion=9`, `RequestConflict=10`, `ArithmeticOverflow=11`, `IndexFull=12`, `RoleLimitReached=13`, `RoleAlreadyGranted=14`, `MismatchOpen=15`, `InvalidTolerance=16`, `CounterpartNotFound=17`.

## Events

| Topic | Data |
| --- | --- |
| `ROLEGRT` | `(tenant_id, project_id, role_code)` |
| `LEDOPEN` | `(period_id, entry_limit, state_code)` |
| `LEDENT` | `(entry_id, kind_code, amount)` |
| `LEDMATCH` | `(entry_id, counterpart_id, delta, state_code)` |
| `LEDRES` | `(entry_id, resolution_hash, state_code)` |
| `LEDCLOSE` | `(period_id, entry_count, state_code)` |

Only IDs, enum codes, hashes, and signed amounts are emitted.

## Privacy boundaries

The ledger is a public commitment and reconciliation surface, not a financial document store. Amounts and coarse kind codes are visible by design. Reference hashes commit to private records kept in the off-chain finance system; the mapping from hash to statement line remains ACL-controlled off-chain.

## Dependencies and rollout

Depends on `capstone-identifiers`, the milestone payment and expense claim contracts for event-driven entry hints, and the tenant role registry. Consumed by backend reconciliation jobs, fraud/anomaly signals, and audit export services.

Rollout: deploy, grant roles, open a single pilot period with a small entry limit, verify mismatch handling, then allow more periods. Rollback: stop creating new periods and leave open ones readable; no funds are held by this contract, so there is no asset unwind. Version `1` is stable; adding fields to stored structs is compatible, removing or reordering fields is not.

## Other repositories

- Backend: statement ingestion, hashing service, match suggestions, tolerance policy, and close attestation workflow.
- Frontend: period dashboard, mismatch queue, variance chart, and resolution upload form.
- Audit/analytics: indexer for `LEDMATCH` and `LEDCLOSE`, anomaly feeds, and immutable export of event history.

## Tests

Deterministic tests cover all eight entry kinds, matched and mismatched deltas, close blocking and resolution, tolerance validation, missing counterpart, replay/conflict/version errors, per-period and per-tenant bounds, cross-tenant denial, event topics and typed data, timestamps, budget ceilings, one-time initialization, and the stable version constant.
