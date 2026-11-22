# CAPSTONE-054: Complete or Terminate Projects

**Track:** Governance | **Component:** on-chain | **Issue:** #1402 | **Contract:** `contracts/capstone-project-closures`

## Scope

Record a structured, auditable closure for a capstone project. A closure carries a reason
(`Completed`, `MutualTermination`, `Inactivity`, `PolicyAction`, `SponsorWithdrawal`), an outcome
hash, an effective time, and an ordered approval workflow: `Requested -> Approved -> Closed`.
Rejection and cancellation are terminal alternatives. Exactly one open closure is allowed per
`(tenant_id, project_id)` pair.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Closure`: tenant, project, closure id, reason enum, outcome/evidence hashes, state, approver set, effective time, version, timestamps | Closure report, retrospective narrative, sponsor letters, personal data, attachments |
| Role grants and approval markers | Signer identity data and notification payloads |
| Events with IDs, enums, versions, timestamps | Audit UI rendering and email content |

Outcome and evidence fields are SHA-256 commitments over canonical off-chain documents. No prose,
URI, memo, or private reference is stored on the ledger.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants or revokes `Coordinator` and `Participant` roles scoped to an exact
  `(tenant_id, project_id)` pair.
- Every mutating call requires `require_auth` from the caller and a matching active role. A missing
  role, a revoked role, or a different tenant/project returns `Unauthorized`; deny is the default.
- Approval is one address per `(closure_id, approver)`; `SelfApproval` and duplicate approvals are
  rejected, and the approver set is bounded by `MAX_APPROVERS = 8`.
- There is no admin bypass for requesting, approving, or closing.

## Idempotency and versioning

- Replaying a closure request with the same payload and expected version returns the stored record
  and emits no second event; a different payload or stale version returns `VersionMismatch`.
- Approvals are idempotent per approver. A second, distinct approval after the threshold is reached
  returns `InvalidState` rather than changing the outcome.
- `close_closure` only succeeds once the closure is `Approved` and `effective_at` has passed;
  otherwise `InvalidState` or `EffectiveAtNotReached` is returned.
- All persistent records extend TTL on read and write, with a bounded TTL range.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `ClosureNotFound=5`, `ClosureAlreadyExists=6`, `OpenClosureExists=7`,
`InvalidState=8`, `VersionMismatch=9`, `ZeroHash=10`, `InvalidEffectiveAt=11`,
`DuplicateApprover=12`, `ApprovalLimitReached=13`, `SelfApproval=14`, `EffectiveAtNotReached=15`.
Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `CLROLE` | `(tenant, project, account, role, active)` |
| `CLREQ` | `(tenant, project, closure_id, reason)` |
| `CLAPP` / `CLREJ` | `(tenant, project, closure_id, approver)` |
| `CLCAN` | `(tenant, project, closure_id)` |
| `CLCLS` | `(tenant, project, closure_id, reason, effective_at)` |

Payloads contain only IDs, enums, versions, and timestamps.

## Privacy boundaries

Closure reasons and outcome hashes are public because they drive downstream escrow and governance
flows. Sponsor names, personnel details, and retrospective text stay off-chain; only bounded hashes
are published. Events never contain notes or document contents. Stellar addresses are public
accounts and must not be derived from private identity data.

## Dependencies

- CAPSTONE-053 pause/resume supplies the suspended state that may precede an inactivity closure.
- CAPSTONE-086 milestone payments and CAPSTONE-088 financial ledger consume the closed outcome;
  CAPSTONE-087 expense claims are finalized against it.
- CAPSTONE-056 governance export includes the closure summary.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the project-lifecycle flag on testnet with a single cohort, dual-writing closure
records to the service database and the contract. Reconcile state and approvals before making the
contract authoritative. Rollback stops new closure calls; stored records are append-only and contain
no private content to purge, and a later deployment resumes from the last stored version.

## Other-repository work

- Backend: canonicalize and hash the closure report, manage request/approval ids, mirror tenant
  roles, and expose reconciliation plus audit export.
- Frontend: closure request, approval queue, and read-only outcome display using hashes for
  private documents.
- Indexer/ops: monitor `CLREQ`/`CLCLS`, alert on `OpenClosureExists`, and renew TTLs for active
  closures.
