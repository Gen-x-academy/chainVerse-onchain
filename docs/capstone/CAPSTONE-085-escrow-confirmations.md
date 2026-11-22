# CAPSTONE-085: Confirm Project Escrow Funding

**Track:** Funding | **Component:** on-chain | **Issue:** #1433 | **Contract:** `contracts/capstone-escrow-confirmations`

## Scope

Confirm that an escrow funding intent is fully funded and eligible for release. The contract
reconciles provider or ledger finality, underpayment, overpayment, asset, destination, agreement,
and expiry before setting `funds_available = true`. It stores the decision, not the provider report
or payment details.

## On-chain stores vs off-chain content

| On-chain | Off-chain (backend / provider adapter / indexer) |
| --- | --- |
| `EscrowConfirmation`: intent id/hash, request id, finality proof hash, expected and observed amounts, asset hash, destination hash, agreement hash, outcome enums, state, version, timestamps | Provider name, ledger transaction payload, destination account label, agreement text, receipts |
| `EscrowRejection`: reason enum and hashes | Rejection explanation, support tickets, notifications |
| `RequestMarker`, role grants, bounded intent index | Provider polling, canonicalization, audit UI |
| Events with IDs, amounts, asset hash, reason enum | Reporting, reconciliation jobs, customer support |

`intent_hash` binds the confirmation to the CAPSTONE-084 intent snapshot. `finality_hash` is the
commitment to a provider/ledger proof. `destination_hash` and `agreement_hash` are commitments to
the destination account and release agreement.

## Authorization

- One-time `initialize` requires the administrator's `require_auth`.
- Tenant-scoped stored roles: `Attestor` confirms funding, `SettlementAuditor` records rejection.
- Every mutating call invokes `require_auth` and checks the stored role for the exact `tenant_id`.
  Missing or revoked roles return `Unauthorized`; deny is the default and there is no admin
  override.
- All keys include `tenant_id`, `project_id`, and `intent_id`; cross-tenant calls fail authorization
  before any write, and cross-tenant reads return `NotFound`.

## Idempotency and versioning

- `request_id` plus `expected_version` is the replay key for confirmation and rejection. `0` is the
  only valid base version because an intent has at most one terminal outcome.
- A retry with the same `request_id`, `intent_hash`, and `expected_version` returns the stored
  outcome and emits no second event. A reused key with different fields returns `RequestConflict`.
- A second request for the same intent returns `AlreadyConfirmed`, `AlreadyRejected`, or
  `InvalidState`; no duplicate availability flag or transfer is created.
- Reconciliation order: expiry, finality, amounts, amount-outcome consistency, asset, destination,
  agreement. `Finality::Final`, exact amounts, and `Reconciliation::Match` on all three commitments
  are required before `funds_available` becomes true.
- Bounds: `MAX_CONFIRMATIONS_PER_PROJECT = 16`; version creation uses `checked_add`; persistent keys
  extend TTL on read/write.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`WrongTenant=4`, `NotFound=5`, `InvalidAmount=6`, `InvalidExpiry=7`, `NotFinal=8`,
`Underpayment=9`, `Overpayment=10`, `AssetMismatch=11`, `DestinationMismatch=12`,
`AgreementMismatch=13`, `VersionConflict=14`, `RequestConflict=15`, `InvalidState=16`,
`CapacityExceeded=17`, `Overflow=18`, `Expired=19`, `InvalidOutcome=20`, `AlreadyConfirmed=21`,
`AlreadyRejected=22`. New codes append only.

## Events

| Symbol | Data |
| --- | --- |
| `CBPROLG` / `CBPRVRK` | `(tenant_id, account, role)` |
| `CBECCD` | `(tenant_id, project_id, intent_id, observed_amount, asset)` |
| `CBEREJ` | `(tenant_id, project_id, intent_id, ReasonCode)` |

Events carry only IDs, amounts, asset identifiers, and enums. Finality and agreement proofs are
stored as hashes and are never emitted as free text.

## Privacy boundaries

Provider reports, ledger memo fields, destination labels, agreement text, and support explanations
stay off-chain. The ledger sees only hashes and public reconciliation outcomes. Amounts and asset
identifiers are public because they must match the intent. Stellar addresses are public accounts and
must not encode private identity. No event topic or data field contains prose.

## Dependencies

- CAPSTONE-084 supplies `intent_id`, `intent_hash`, expected amount, asset, and expiry.
- CAPSTONE-082/083 supply the approved plan and reservation context off-chain.
- The provider adapter supplies the finality proof that the backend hashes into `finality_hash`;
  this contract does not call an external oracle.
- Tenant roles are mirrored from the shared access-control service.

## Rollout and rollback

Deploy behind the capstone funding flag, run in shadow mode against provider data for one cohort,
and compare `funds_available` with the backend ledger before making it authoritative. Rollback
stops new confirmations; stored confirmations remain readable and no private payload needs purging.
Because the contract never holds funds, rollback does not require an on-chain refund path; the
existing escrow/payment contract remains the settlement authority. Renew TTL for active intent ids
across upgrades.

## Other-repository work

- Backend: provider polling, canonical proof hashing, tenant role sync, idempotency keys, and
  reconciliation between escrow balance and confirmation records.
- Frontend: confirmation status, under/overpayment explanation, and funding-available indicator.
- Indexer/ops: monitor `CBECCD`/`CBEREJ`, alert on reconciliation errors, and manage TTL renewal.
