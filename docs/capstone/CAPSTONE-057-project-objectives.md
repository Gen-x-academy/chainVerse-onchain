# CAPSTONE-057: Create Project Objectives and Success Criteria

**Track:** Planning | **Component:** on-chain | **Issue:** #1405 | **Contract:** `contracts/capstone-objectives`

## Scope

Register prioritized, measurable project objectives with owners, baselines, targets, dates,
visibility, and a charter linkage. Each objective carries up to 16 success criteria with bounded
weights and values, moves through `Draft -> Active -> Closed`, and keeps an append-only revision
history. Objectives are limited to 64 per project and revisions to 32 per objective.

## On-chain stores vs off-chain content

| On-chain | Off-chain (capstone service) |
| --- | --- |
| `Objective`: tenant, project, objective id, owner/delegate, priority, visibility, dates, baseline/target, state, version, criteria hash | Objective narrative, risk analysis, stakeholder notes |
| `Criterion`: bounded id, weight, measured value, target, spec hash | Measurement methodology, source documents |
| `Revision` markers, role grants, events | Charter document, private context, notifications |

Baseline, target, and specification fields are SHA-256 commitments or bounded integers; no prose,
URI, memo, or private reference is published.

## Authorization

- `initialize` is one-time and requires the administrator's `require_auth`.
- The administrator grants and revokes tenant/project-scoped `Participant` and `Coordinator` roles.
- Objective creation and amendment require a coordinator role for the exact tenant/project and the
  caller's `require_auth`. Owner or delegate approval is checked for objective-scoped mutations;
  an outsider gets `Unauthorized`.
- State changes require the same scoped authorization. Restricted objectives remain readable only
  through the off-chain service; the ledger stores visibility as an enum.
- Deny is the default: an account with no matching role cannot create, amend, or transition.

## Idempotency and versioning

- Re-registering the same objective id with the same payload and expected version returns the stored
  record and emits no second event; a stale or different payload returns `VersionMismatch`.
- Amendments append one revision with a new expected version. Replaying an amendment is a no-op;
  a stale version returns `VersionMismatch`.
- `Draft -> Active -> Closed` is the only valid transition path; invalid jumps return `InvalidState`.
  Closed objectives are terminal.
- Bounds: `MAX_OBJECTIVES = 64`, `MAX_CRITERIA = 16`, `MAX_REVISIONS = 32`; zero hashes, empty
  criteria, and invalid priorities are rejected with stable errors.

## Errors

Append-only `ContractError` codes: `NotInitialized=1`, `AlreadyInitialized=2`, `Unauthorized=3`,
`InvalidRole=4`, `ObjectiveNotFound=5`, `ObjectiveAlreadyExists=6`, `ZeroHash=7`, `NoCriteria=8`,
`InvalidPriority=9`, `CriteriaLimitReached=10`, `VersionMismatch=11`, `InvalidVersion=12`,
`InvalidState=13`, `ObjectiveLimitReached=14`, `RevisionLimitReached=15`. Codes are never renumbered.

## Events

| Symbol | Data |
| --- | --- |
| `OBJROLE` | `(tenant, project, account, role, active)` |
| `OBJCREATE` | `(tenant, project, objective_id, priority, visibility)` |
| `OBJAMEND` | `(tenant, project, objective_id, version, criteria_hash)` |
| `OBJSTATE` | `(tenant, project, objective_id, state, version)` |

Only IDs, enums, hashes, versions, and bounded values are emitted.

## Privacy boundaries

Objectives may describe sensitive strategy. Visibility is recorded as a public enum, while the
narrative and private context stay off-chain. Only hashes and bounded numeric baselines/targets are
stored on-chain, and events never carry prose or references. Addresses are public Stellar accounts
and must not be derived from private identity data.

## Dependencies

- CAPSTONE-050 charter acceptance supplies the charter linkage for each objective.
- CAPSTONE-058 work breakdown aligns task progress weights with objective criteria.
- CAPSTONE-061 objective check-ins consume the active objective and owner/delegate configuration.
- CAPSTONE-060 milestones link deliverables to objective ids.
- Role grants are mirrored from the shared tenant access-control service by the backend.

## Rollout and rollback

Enable behind the planning-objectives flag on testnet for one cohort, dual-writing objectives to the
service database and the contract. Promote after reconciling criteria hashes and versions. Rollback
stops new objective writes; stored revisions are append-only and hash-only, so no private content
needs purging and a later deployment can resume from the last stored version.

## Other-repository work

- Backend: canonicalize and hash objectives and criteria, mirror roles, manage revisions, and expose
  reconciliation and audit views.
- Frontend: objective editor, criteria weighting, state transitions, and read-only revision history.
- Indexer/ops: monitor `OBJCREATE`/`OBJSTATE`, alert on `VersionMismatch`, and renew TTLs for active
  objectives and revisions.
